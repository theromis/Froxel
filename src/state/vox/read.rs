use std::collections::{HashMap, HashSet};

use super::*;

pub(crate) fn load_vox(bytes: &[u8]) -> Result<Loaded, String> {
    if bytes.len() < 8 || &bytes[0..4] != b"VOX " {
        return Err("not a .vox file".into());
    }
    let mut p = 8usize;
    let id = read_id(bytes, &mut p)?;
    if &id != b"MAIN" {
        return Err("expected MAIN chunk".into());
    }
    let main_content = read_u32(bytes, &mut p)? as usize;
    let main_children = read_u32(bytes, &mut p)? as usize;
    p = p.saturating_add(main_content);
    if p + main_children > bytes.len() {
        return Err("truncated MAIN chunk".into());
    }
    let end = p + main_children;

    let mut models: Vec<([i32; 3], Vec<(i32, i32, i32, u8)>)> = Vec::new();
    let mut pending: Option<[i32; 3]> = None;
    let mut nodes: HashMap<i32, VoxNode> = HashMap::new();
    let mut order = Vec::new();
    let mut layers = Vec::new();
    let mut palette = None;
    let mut materials = [Material::default(); 256];
    let mut rig: Option<(Skeleton, Vec<crate::rig::StoredBind>)> = None;
    let mut clips = None;
    let mut scripts = None;

    while p + 12 <= end {
        let id = read_id(bytes, &mut p)?;
        let content = read_u32(bytes, &mut p)? as usize;
        let children = read_u32(bytes, &mut p)? as usize;
        if p + content + children > end {
            return Err("chunk extends past MAIN".into());
        }
        let body = &bytes[p..p + content];
        if &id == b"SIZE" {
            if body.len() != 12 {
                return Err("SIZE chunk has the wrong length".into());
            }
            pending = Some([
                u32::from_le_bytes(body[0..4].try_into().unwrap()) as i32,
                u32::from_le_bytes(body[4..8].try_into().unwrap()) as i32,
                u32::from_le_bytes(body[8..12].try_into().unwrap()) as i32,
            ]);
        } else if &id == b"XYZI" {
            let Some(dims) = pending.take() else {
                return Err("XYZI chunk before SIZE".into());
            };
            if body.len() < 4 {
                return Err("truncated XYZI".into());
            }
            let n = u32::from_le_bytes(body[0..4].try_into().unwrap()) as usize;
            if body.len() < 4 + n * 4 {
                return Err("XYZI voxel data is truncated".into());
            }
            let mut voxels = Vec::with_capacity(n);
            for i in 0..n {
                let o = 4 + i * 4;
                voxels.push((
                    body[o] as i32,
                    body[o + 1] as i32,
                    body[o + 2] as i32,
                    body[o + 3],
                ));
            }
            models.push((dims, voxels));
        } else if &id == b"nTRN" {
            if let Some(node) = parse_trn(body) {
                nodes.insert(node.id, node.clone());
                order.push(node);
            }
        } else if &id == b"nGRP" {
            if let Some(node) = parse_grp(body) {
                nodes.insert(node.id, node.clone());
                order.push(node);
            }
        } else if &id == b"nSHP" {
            if let Some(node) = parse_shp(body) {
                nodes.insert(node.id, node.clone());
                order.push(node);
            }
        } else if &id == b"LAYR" {
            if let Some(layer) = parse_layr(body) {
                layers.push(layer);
            }
        } else if &id == b"RGBA" {
            if body.len() < 1024 {
                return Err("RGBA chunk is shorter than 1024 bytes".into());
            }
            let mut pal = default_palette();
            for i in 0..255 {
                pal[i + 1] = [
                    body[i * 4],
                    body[i * 4 + 1],
                    body[i * 4 + 2],
                    body[i * 4 + 3],
                ];
            }
            pal[0] = [0, 0, 0, 0];
            palette = Some(pal);
        } else if &id == b"MATL" {
            if let Some((slot, material)) = parse_matl(body) {
                materials[slot] = material;
            }
        } else if &id == b"MVRG" {
            if rig.is_some() {
                return Err("duplicate MVRG chunk".into());
            }
            rig = Some(crate::rig::decode_chunk(body)?);
        } else if &id == b"MVAN" {
            if clips.is_some() {
                return Err("duplicate MVAN chunk".into());
            }
            clips = Some(crate::rig::decode_clips(body)?);
        } else if &id == b"MVSC" {
            if scripts.is_some() {
                return Err("duplicate MVSC chunk".into());
            }
            scripts = Some(crate::script::decode_chunk(body)?);
        }
        p += content + children;
    }

    if models.is_empty() {
        return Err("file has no models".into());
    }
    let (mut objects, shift) = place_models(&models, &nodes);
    let skeleton = if let Some((mut skeleton, binds)) = rig {
        // Pivots are world positions: they move with the objects.
        crate::rig::shift_pivots(&mut skeleton, shift);
        let view: Vec<(i32, &str)> = objects
            .iter()
            .map(|object| {
                (
                    object.file.as_ref().map(|link| link.trn).unwrap_or(-1),
                    object.name.as_str(),
                )
            })
            .collect();
        let assigned = crate::rig::assign_bones(&view, &binds, &skeleton)?;
        for (object, bone) in objects.iter_mut().zip(assigned) {
            object.bone = bone;
        }
        skeleton
    } else {
        Skeleton::default()
    };
    if let Some(scripts) = scripts {
        for crate::script::StoredScript {
            name,
            script,
            local_min,
        } in scripts
        {
            if let Some(object) = objects.iter_mut().find(|object| object.name == name) {
                if let (Some(saved), Some((loaded, _))) =
                    (local_min, crate::state::occupied_bounds(&object.cells))
                {
                    let delta = (
                        saved.0.checked_sub(loaded.0),
                        saved.1.checked_sub(loaded.1),
                        saved.2.checked_sub(loaded.2),
                    );
                    let (Some(dx), Some(dy), Some(dz)) = delta else {
                        return Err("MVSC local origin overflow".into());
                    };
                    let add = |p: Cell| -> Result<Cell, String> {
                        match (
                            p.0.checked_add(dx),
                            p.1.checked_add(dy),
                            p.2.checked_add(dz),
                        ) {
                            (Some(x), Some(y), Some(z)) => Ok((x, y, z)),
                            _ => Err("MVSC local origin overflow".into()),
                        }
                    };
                    object.cells = object
                        .cells
                        .iter()
                        .map(|(cell, slot)| Ok((add(*cell)?, *slot)))
                        .collect::<Result<HashMap<_, _>, String>>()?;
                    object.box_min = add(object.box_min)?;
                    object.translation = match (
                        object.translation.0.checked_sub(dx),
                        object.translation.1.checked_sub(dy),
                        object.translation.2.checked_sub(dz),
                    ) {
                        (Some(x), Some(y), Some(z)) => (x, y, z),
                        _ => return Err("MVSC local origin overflow".into()),
                    };
                }
                object.script = script;
            } else {
                objects.push(LoadedObject {
                    name,
                    cells: HashMap::new(),
                    translation: (0, 0, 0),
                    box_min: (0, 0, 0),
                    dims: (0, 0, 0),
                    rot: ROT_IDENTITY,
                    pivot: (0, 0, 0),
                    model_dims: (0, 0, 0),
                    hidden: false,
                    file: None,
                    bone: None,
                    script,
                });
            }
        }
    }
    let graph = if order.is_empty() && layers.is_empty() {
        None
    } else {
        Some(VoxGraph {
            nodes: order,
            models: models
                .iter()
                .map(|(dims, voxels)| StoredModel {
                    dims: *dims,
                    voxels: voxels.clone(),
                })
                .collect(),
            layers,
            shift,
        })
    };
    Ok(Loaded {
        objects,
        palette,
        materials,
        graph,
        skeleton,
        clips: clips.unwrap_or_default(),
    })
}

pub(crate) fn parse_trn(body: &[u8]) -> Option<VoxNode> {
    let mut cursor = body;
    let id = take_i32(&mut cursor)?;
    let attrs = read_pairs(&mut cursor)?;
    let child = take_i32(&mut cursor)?;
    let _reserved = take_i32(&mut cursor)?;
    let layer = take_i32(&mut cursor)?;
    let frame_count = take_i32(&mut cursor)?;
    if frame_count < 0 {
        return None;
    }
    let mut frames = Vec::with_capacity(frame_count as usize);
    let mut t = [0, 0, 0];
    let mut rot = 4u8;
    for index in 0..frame_count {
        let frame = read_pairs(&mut cursor)?;
        if index == 0 {
            if let Some(text) = pair(&frame, "_t") {
                t = parse_translation(text).unwrap_or(t);
            }
            if let Some(text) = pair(&frame, "_r") {
                rot = text.parse().unwrap_or(4);
            }
        }
        frames.push(frame);
    }
    Some(VoxNode {
        id,
        kind: VoxKind::Transform,
        child,
        children: Vec::new(),
        model_id: -1,
        models: Vec::new(),
        t,
        rot,
        layer,
        name: pair(&attrs, "_name").unwrap_or("").to_string(),
        attrs,
        frames,
    })
}

pub(crate) fn parse_grp(body: &[u8]) -> Option<VoxNode> {
    let mut cursor = body;
    let id = take_i32(&mut cursor)?;
    let attrs = read_pairs(&mut cursor)?;
    let count = take_i32(&mut cursor)?;
    if count < 0 {
        return None;
    }
    let mut children = Vec::with_capacity(count as usize);
    for _ in 0..count {
        children.push(take_i32(&mut cursor)?);
    }
    Some(VoxNode {
        id,
        kind: VoxKind::Group,
        child: -1,
        children,
        model_id: -1,
        models: Vec::new(),
        t: [0, 0, 0],
        rot: 4,
        layer: -1,
        name: pair(&attrs, "_name").unwrap_or("").to_string(),
        attrs,
        frames: Vec::new(),
    })
}

pub(crate) fn parse_shp(body: &[u8]) -> Option<VoxNode> {
    let mut cursor = body;
    let id = take_i32(&mut cursor)?;
    let attrs = read_pairs(&mut cursor)?;
    let count = take_i32(&mut cursor)?;
    if count < 0 {
        return None;
    }
    let mut models = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let model = take_i32(&mut cursor)?;
        let model_attrs = read_pairs(&mut cursor)?;
        models.push((model, model_attrs));
    }
    Some(VoxNode {
        id,
        kind: VoxKind::Shape,
        child: -1,
        children: Vec::new(),
        model_id: models.first().map(|(model, _)| *model).unwrap_or(-1),
        models,
        t: [0, 0, 0],
        rot: 4,
        layer: -1,
        name: pair(&attrs, "_name").unwrap_or("").to_string(),
        attrs,
        frames: Vec::new(),
    })
}

pub(crate) fn parse_layr(body: &[u8]) -> Option<VoxLayer> {
    let mut cursor = body;
    let id = take_i32(&mut cursor)?;
    let attrs = read_pairs(&mut cursor)?;
    let _reserved = take_i32(&mut cursor)?;
    Some(VoxLayer { id, attrs })
}

pub(crate) fn parse_translation(text: &str) -> Option<[i32; 3]> {
    let mut parts = text.split_whitespace();
    Some([
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ])
}

pub(crate) fn place_models(
    models: &[([i32; 3], Vec<(i32, i32, i32, u8)>)],
    nodes: &HashMap<i32, VoxNode>,
) -> (Vec<LoadedObject>, [i32; 3]) {
    let placements = if nodes.is_empty() {
        Vec::new()
    } else {
        let root = if nodes.contains_key(&0) {
            0
        } else {
            *nodes.keys().min().unwrap_or(&0)
        };
        let mut out = Vec::new();
        let mut path = HashSet::new();
        walk_nodes(
            nodes,
            root,
            [0, 0, 0],
            ROT_IDENTITY,
            false,
            &mut path,
            &mut out,
        );
        out
    };
    let placements = if placements.is_empty() {
        let dims = models.first().map(|(dims, _)| *dims).unwrap_or([0, 0, 0]);
        vec![Placement {
            model_id: 0,
            world_t: [dims[0] / 2, dims[1] / 2, dims[2] / 2],
            world_r: ROT_IDENTITY,
            name: String::new(),
            hidden: false,
            trn: -1,
            parent_rot: ROT_IDENTITY,
            parent_t: [0, 0, 0],
        }]
    } else {
        placements
    };
    let mut world = Vec::new();
    for placement in placements {
        let Some((dims, voxels)) = models.get(placement.model_id as usize) else {
            continue;
        };
        let half = [dims[0] / 2, dims[1] / 2, dims[2] / 2];
        // nTRN `_t` is the model centre. Keep a world copy so the scene
        // can still be recentered, and a corner copy for the editor.
        let mut cells = HashMap::new();
        for (x, y, z, color) in voxels {
            if *color == 0 {
                continue;
            }
            let spun = rot_apply(placement.world_r, [x - half[0], y - half[1], z - half[2]]);
            cells.insert(
                (
                    spun[0] + placement.world_t[0],
                    spun[1] + placement.world_t[1],
                    spun[2] + placement.world_t[2],
                ),
                *color,
            );
        }
        let centre = rot_apply(placement.world_r, half);
        let corner = [
            placement.world_t[0] - centre[0],
            placement.world_t[1] - centre[1],
            placement.world_t[2] - centre[2],
        ];
        world.push((
            placement.name,
            cells,
            voxels.clone(),
            corner,
            placement.world_t,
            placement.world_r,
            *dims,
            placement.hidden,
            placement.trn,
            placement.parent_rot,
            placement.parent_t,
            placement.model_id,
        ));
    }
    if world.is_empty() {
        return (
            vec![LoadedObject {
                name: String::new(),
                cells: HashMap::new(),
                translation: (0, 0, 0),
                box_min: (0, 0, 0),
                dims: (0, 0, 0),
                rot: ROT_IDENTITY,
                pivot: (0, 0, 0),
                model_dims: (0, 0, 0),
                hidden: false,
                file: None,
                bone: None,
                script: Default::default(),
            }],
            [0, 0, 0],
        );
    }
    // The first object is centred across, so a saved scene reopens in place;
    // the lowest voxel of the whole scene goes on the floor.
    let mut shift0 = world
        .iter()
        .find_map(|(_, cells, ..)| display_shift(cells.keys()))
        .unwrap_or((0, 0, 0));
    if let Some(all) = display_shift(world.iter().flat_map(|(_, cells, ..)| cells.keys())) {
        shift0.2 = all.2;
    }
    let objects = world
        .into_iter()
        .map(
            |(
                name,
                _world_cells,
                file_cells,
                corner,
                anchor,
                world_r,
                file_dims,
                hidden,
                trn,
                parent_rot,
                parent_t,
                model_id,
            )| {
                // Local (0, 0, 0) is the SIZE corner. A rotation is baked
                // into the cells, so a later edit still uses world axes.
                let mut local = HashMap::new();
                for (x, y, z, color) in file_cells {
                    if color == 0 {
                        continue;
                    }
                    let spun = rot_apply(world_r, [x, y, z]);
                    local.insert((spun[0], spun[1], spun[2]), color);
                }
                let link_name = name.clone();
                let translation = (
                    corner[0] + shift0.0,
                    corner[1] + shift0.1,
                    corner[2] + shift0.2,
                );
                let (box_min, box_dims) = corner_box(file_dims, world_r);
                LoadedObject {
                    name,
                    cells: local,
                    translation,
                    box_min,
                    dims: box_dims,
                    rot: if rot_is_identity(world_r) {
                        ROT_IDENTITY
                    } else {
                        world_r
                    },
                    pivot: if rot_is_identity(world_r) {
                        (0, 0, 0)
                    } else {
                        (
                            anchor[0] + shift0.0,
                            anchor[1] + shift0.1,
                            anchor[2] + shift0.2,
                        )
                    },
                    model_dims: if rot_is_identity(world_r) {
                        (0, 0, 0)
                    } else {
                        (file_dims[0], file_dims[1], file_dims[2])
                    },
                    hidden,
                    file: (trn >= 0).then(|| FileLink {
                        trn,
                        model: model_id,
                        parent_rot,
                        parent_t,
                        name: link_name,
                    }),
                    bone: None,
                    script: Default::default(),
                }
            },
        )
        .collect();
    (objects, [shift0.0, shift0.1, shift0.2])
}

pub(crate) fn size_box_world(
    dims: [i32; 3],
    half: [i32; 3],
    world_t: [i32; 3],
    world_r: [[i32; 3]; 3],
) -> (Cell, (i32, i32, i32)) {
    let mut lo = [i32::MAX; 3];
    let mut hi = [i32::MIN; 3];
    for mask in 0..8 {
        let corner = [
            if mask & 1 == 0 { 0 } else { dims[0] },
            if mask & 2 == 0 { 0 } else { dims[1] },
            if mask & 4 == 0 { 0 } else { dims[2] },
        ];
        let spun = rot_apply(
            world_r,
            [
                corner[0] - half[0],
                corner[1] - half[1],
                corner[2] - half[2],
            ],
        );
        let world = [
            spun[0] + world_t[0],
            spun[1] + world_t[1],
            spun[2] + world_t[2],
        ];
        for axis in 0..3 {
            lo[axis] = lo[axis].min(world[axis]);
            hi[axis] = hi[axis].max(world[axis]);
        }
    }
    (
        (lo[0], lo[1], lo[2]),
        (hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]),
    )
}

/// SIZE box in corner-local cells. Identity keeps it on the origin.
pub(crate) fn corner_box(dims: [i32; 3], rot: [[i32; 3]; 3]) -> (Cell, (i32, i32, i32)) {
    let mut lo = [i32::MAX; 3];
    let mut hi = [i32::MIN; 3];
    for mask in 0..8 {
        let corner = [
            if mask & 1 == 0 { 0 } else { dims[0] },
            if mask & 2 == 0 { 0 } else { dims[1] },
            if mask & 4 == 0 { 0 } else { dims[2] },
        ];
        let spun = rot_apply(rot, corner);
        for axis in 0..3 {
            lo[axis] = lo[axis].min(spun[axis]);
            hi[axis] = hi[axis].max(spun[axis]);
        }
    }
    (
        (lo[0], lo[1], lo[2]),
        (hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]),
    )
}

pub(crate) fn display_shift<'a>(cells: impl IntoIterator<Item = &'a Cell>) -> Option<Cell> {
    let mut cells = cells.into_iter().peekable();
    let first = **cells.peek()?;
    let mut min = first;
    let mut max = first;
    for cell in cells {
        min.0 = min.0.min(cell.0);
        min.1 = min.1.min(cell.1);
        min.2 = min.2.min(cell.2);
        max.0 = max.0.max(cell.0);
        max.1 = max.1.max(cell.1);
        max.2 = max.2.max(cell.2);
    }
    Some((-((min.0 + max.0) / 2), -((min.1 + max.1) / 2), -min.2))
}

pub(crate) fn walk_nodes(
    nodes: &HashMap<i32, VoxNode>,
    id: i32,
    t: [i32; 3],
    r: [[i32; 3]; 3],
    hidden: bool,
    path: &mut HashSet<i32>,
    out: &mut Vec<Placement>,
) {
    if !path.insert(id) {
        return;
    }
    if let Some(node) = nodes.get(&id).cloned() {
        match node.kind {
            VoxKind::Transform => {
                let hidden = hidden || attr_hidden(&node.attrs);
                let spun = rot_apply(r, node.t);
                let ct = [t[0] + spun[0], t[1] + spun[1], t[2] + spun[2]];
                let cr = rot_mul(r, rot_from_byte(node.rot));
                let child_is_shape = nodes
                    .get(&node.child)
                    .is_some_and(|child| child.kind == VoxKind::Shape);
                if child_is_shape {
                    if let Some(shape) = nodes.get(&node.child) {
                        if shape.model_id >= 0 {
                            let name = if node.name.is_empty() {
                                shape.name.clone()
                            } else {
                                node.name.clone()
                            };
                            out.push(Placement {
                                model_id: shape.model_id,
                                world_t: ct,
                                world_r: cr,
                                name,
                                hidden: hidden || attr_hidden(&shape.attrs),
                                trn: node.id,
                                parent_rot: r,
                                parent_t: t,
                            });
                        }
                    }
                } else {
                    walk_nodes(nodes, node.child, ct, cr, hidden, path, out);
                }
            }
            VoxKind::Group => {
                let hidden = hidden || attr_hidden(&node.attrs);
                for child in &node.children {
                    walk_nodes(nodes, *child, t, r, hidden, path, out);
                }
            }
            VoxKind::Shape => {
                if node.model_id >= 0 {
                    out.push(Placement {
                        model_id: node.model_id,
                        world_t: t,
                        world_r: r,
                        name: node.name.clone(),
                        hidden: hidden || attr_hidden(&node.attrs),
                        trn: -1,
                        parent_rot: ROT_IDENTITY,
                        parent_t: [0, 0, 0],
                    });
                }
            }
        }
    }
    path.remove(&id);
}

pub(crate) fn parse_matl(body: &[u8]) -> Option<(usize, Material)> {
    let mut cursor = body;
    let id = take_u32(&mut cursor)? as i32;
    if !(1..=255).contains(&id) {
        return None;
    }
    let count = take_u32(&mut cursor)? as usize;
    let mut dict = HashMap::with_capacity(count);
    for _ in 0..count {
        let key = take_string(&mut cursor)?;
        let value = take_string(&mut cursor)?;
        dict.insert(key, value);
    }
    Some((id as usize, material_from_dict(&dict)))
}

pub(crate) fn take_i32(bytes: &mut &[u8]) -> Option<i32> {
    take_u32(bytes).map(|value| value as i32)
}

pub(crate) fn read_pairs(bytes: &mut &[u8]) -> Option<Vec<(String, String)>> {
    let count = take_u32(bytes)? as usize;
    let mut pairs = Vec::with_capacity(count);
    for _ in 0..count {
        pairs.push((take_string(bytes)?, take_string(bytes)?));
    }
    Some(pairs)
}

pub(crate) fn pair<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(stored, _)| stored == key)
        .map(|(_, value)| value.as_str())
}

pub(crate) fn attr_hidden(pairs: &[(String, String)]) -> bool {
    pair(pairs, "_hidden") == Some("1")
}

pub(crate) fn upsert_pair(pairs: &mut Vec<(String, String)>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        if let Some(slot) = pairs.iter_mut().find(|(stored, _)| stored == key) {
            slot.1 = value;
        } else {
            pairs.push((key.to_string(), value));
        }
    } else {
        pairs.retain(|(stored, _)| stored != key);
    }
}

pub(crate) fn take_u32(bytes: &mut &[u8]) -> Option<u32> {
    if bytes.len() < 4 {
        return None;
    }
    let value = u32::from_le_bytes(bytes[..4].try_into().ok()?);
    *bytes = &bytes[4..];
    Some(value)
}

pub(crate) fn take_string(bytes: &mut &[u8]) -> Option<String> {
    let len = take_u32(bytes)? as usize;
    if bytes.len() < len {
        return None;
    }
    let text = std::str::from_utf8(&bytes[..len]).ok()?.to_string();
    *bytes = &bytes[len..];
    Some(text)
}

pub(crate) fn dict_float(dict: &HashMap<String, String>, key: &str, fallback: f32) -> f32 {
    match dict.get(key) {
        Some(text) => text.parse().unwrap_or(0.0),
        None => fallback,
    }
}

pub(crate) fn material_from_dict(dict: &HashMap<String, String>) -> Material {
    let mut material = Material::default();
    if let Some(ty) = dict.get("_type") {
        material.ty = match ty.as_str() {
            "_metal" => MaterialType::Metal,
            "_glass" => MaterialType::Glass,
            "_emit" => MaterialType::Emissive,
            _ => MaterialType::Diffuse,
        };
    }
    material.roughness = dict_float(dict, "_rough", material.roughness);
    material.metallic = dict_float(dict, "_metal", material.metallic);
    material.ior = 1.0 + dict_float(dict, "_ior", material.ior - 1.0);
    if let Some(alpha) = dict.get("_alpha").or_else(|| dict.get("_trans")) {
        material.transparency = alpha.parse().unwrap_or(0.0);
    }
    if material.ty == MaterialType::Emissive {
        material.emission = dict_float(dict, "_emit", material.emission);
        let flux = dict_float(dict, "_flux", 0.0);
        material.emission_power = (1 + flux as i32).clamp(1, 5);
    }
    if let Some(kind) = dict.get("_motion") {
        material.motion = Motion::parse(kind).unwrap_or(Motion::None);
        if material.motion != Motion::None {
            material.motion_amount = dict_float(dict, "_motion_amount", 1.0).clamp(0.0, 4.0);
        }
    }
    material
}

pub(crate) fn read_id(bytes: &[u8], p: &mut usize) -> Result<[u8; 4], String> {
    if *p + 4 > bytes.len() {
        return Err("truncated chunk id".into());
    }
    let id = [bytes[*p], bytes[*p + 1], bytes[*p + 2], bytes[*p + 3]];
    *p += 4;
    Ok(id)
}

pub(crate) fn read_u32(bytes: &[u8], p: &mut usize) -> Result<u32, String> {
    if *p + 4 > bytes.len() {
        return Err("truncated chunk header".into());
    }
    let v = u32::from_le_bytes(bytes[*p..*p + 4].try_into().unwrap());
    *p += 4;
    Ok(v)
}
