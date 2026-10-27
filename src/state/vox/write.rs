use std::collections::HashSet;

use super::*;

/// Graphless single-model writer. The editor saves a scene; these tests
/// check palette and material bytes without a node graph.
#[cfg(test)]
pub(crate) fn save_vox(
    cells: &HashMap<Cell, u8>,
    palette: &[[u8; 4]; 256],
    materials: &[Material; 256],
) -> Result<Vec<u8>, String> {
    let mut voxels: Vec<(i32, i32, i32, u8)> = cells
        .iter()
        .filter(|(_, index)| **index != 0)
        .map(|(cell, index)| (cell.0, cell.1, cell.2, *index))
        .collect();
    if voxels.is_empty() {
        return Err("cannot save an empty grid".into());
    }
    let mut min = (voxels[0].0, voxels[0].1, voxels[0].2);
    let mut max = min;
    for voxel in &voxels {
        min.0 = min.0.min(voxel.0);
        min.1 = min.1.min(voxel.1);
        min.2 = min.2.min(voxel.2);
        max.0 = max.0.max(voxel.0);
        max.1 = max.1.max(voxel.1);
        max.2 = max.2.max(voxel.2);
    }
    let sx = max.0 - min.0 + 1;
    let sy = max.1 - min.1 + 1;
    let sz = max.2 - min.2 + 1;
    if sx > 256 || sy > 256 || sz > 256 {
        return Err(format!(
            "scene exceeds .vox single-model limit of 256 per axis (bounds {sx}x{sy}x{sz})"
        ));
    }
    for voxel in &mut voxels {
        voxel.0 -= min.0;
        voxel.1 -= min.1;
        voxel.2 -= min.2;
    }
    voxels.sort_unstable();

    let mut children = Vec::new();
    let mut size = Vec::new();
    size.extend_from_slice(&(sx as u32).to_le_bytes());
    size.extend_from_slice(&(sy as u32).to_le_bytes());
    size.extend_from_slice(&(sz as u32).to_le_bytes());
    push_chunk(&mut children, b"SIZE", &size);

    let mut xyzi = Vec::with_capacity(4 + voxels.len() * 4);
    xyzi.extend_from_slice(&(voxels.len() as u32).to_le_bytes());
    for (x, y, z, index) in voxels {
        xyzi.extend_from_slice(&[x as u8, y as u8, z as u8, index]);
    }
    push_chunk(&mut children, b"XYZI", &xyzi);
    push_rgba(&mut children, palette);
    push_matl(&mut children, materials);
    Ok(wrap_vox(children))
}

pub(crate) fn save_document(
    objects: &[Volume],
    palette: &[[u8; 4]; 256],
    materials: &[Material; 256],
    graph: Option<&VoxGraph>,
    skeleton: &Skeleton,
    clips: &[crate::rig::Clip],
    method: Option<Method>,
) -> Result<Vec<u8>, String> {
    let mut bytes = match graph {
        Some(graph) => save_graph(objects, palette, materials, graph, skeleton, clips),
        None => save_scene(objects, palette, materials, skeleton, clips),
    }?;
    if let Some(method) = method {
        append_chunk(&mut bytes, b"MVPR", &provenance_body(method));
    }
    Ok(bytes)
}

/// `MVPR` body. A document with no label gets no chunk.
pub(crate) fn provenance_body(method: Method) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&PROVENANCE_VERSION.to_le_bytes());
    push_dict(&mut body, &[("method", method.as_str())]);
    body
}

/// Write the loaded graph back. Frame 0 picks up the edited translation,
/// rotation, name, and `_hidden`. Later animation frames, groups, shape
/// model lists, and layers stay as they were read.
pub(crate) fn save_graph(
    objects: &[Volume],
    palette: &[[u8; 4]; 256],
    materials: &[Material; 256],
    graph: &VoxGraph,
    skeleton: &Skeleton,
    clips: &[crate::rig::Clip],
) -> Result<Vec<u8>, String> {
    let mut models = graph.models.clone();
    let mut nodes = graph.nodes.clone();
    let mut next_id = nodes.iter().map(|node| node.id).max().unwrap_or(0) + 1;
    let index_of = |nodes: &[VoxNode], id: i32| nodes.iter().position(|node| node.id == id);
    let mut rig_saved: Vec<(i32, String, Option<String>)> = Vec::new();

    for object in objects {
        let Some(link) = &object.file else {
            continue;
        };
        let Some((packed, keep_rotation)) = pack_model(object)? else {
            continue;
        };
        if (0..models.len()).contains(&(link.model as usize)) {
            models[link.model as usize] = packed;
        }
        if let Some(index) = index_of(&nodes, link.trn) {
            rewrite_leaf(&mut nodes[index], object, link, graph.shift, keep_rotation);
        }
        rig_saved.push((link.trn, object.name.clone(), object.bone.clone()));
    }

    let mut added = Vec::new();
    for object in objects {
        if object.file.is_some() {
            continue;
        }
        let Some((packed, _)) = pack_model(object)? else {
            continue;
        };
        let model = models.len() as i32;
        models.push(packed);
        let (parent_t, parent_rot, _) = group_space(&nodes);
        let shape_id = next_id;
        next_id += 1;
        let trn_id = next_id;
        next_id += 1;
        let keep_rotation = stored_rotation(object) && unbake_model(object).is_some();
        nodes.push(shape_node(shape_id, model));
        nodes.push(transform_node(
            trn_id,
            shape_id,
            object,
            parent_t,
            parent_rot,
            graph.shift,
            keep_rotation,
        ));
        added.push(trn_id);
        rig_saved.push((trn_id, object.name.clone(), object.bone.clone()));
    }
    if !added.is_empty() {
        let group_id = ensure_group(&mut nodes, &mut next_id);
        if let Some(group) = nodes.iter_mut().find(|node| node.id == group_id) {
            group.children.extend(added);
        }
    }

    let live: HashSet<i32> = objects
        .iter()
        .filter_map(|object| object.file.as_ref().map(|link| link.trn))
        .collect();
    let mut skip = HashSet::new();
    for node in &graph.nodes {
        if node.kind != VoxKind::Transform || live.contains(&node.id) {
            continue;
        }
        let child_is_shape = graph
            .nodes
            .iter()
            .any(|other| other.id == node.child && other.kind == VoxKind::Shape);
        if child_is_shape {
            skip.insert(node.id);
        }
    }
    loop {
        let mut grew = false;
        for node in &nodes {
            if node.kind != VoxKind::Transform || skip.contains(&node.id) {
                continue;
            }
            let child_skipped = nodes.iter().any(|other| {
                other.id == node.child
                    && other.kind == VoxKind::Transform
                    && skip.contains(&other.id)
            });
            if child_skipped {
                skip.insert(node.id);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }

    if models.is_empty() {
        return Err("cannot save an empty grid".into());
    }
    let mut children = Vec::new();
    for model in &models {
        push_model(&mut children, model)?;
    }
    for node in &nodes {
        if skip.contains(&node.id) {
            continue;
        }
        match node.kind {
            VoxKind::Transform => {
                push_chunk(&mut children, b"nTRN", &transform_body(node));
            }
            VoxKind::Group => {
                let kids: Vec<i32> = node
                    .children
                    .iter()
                    .copied()
                    .filter(|id| !skip.contains(id))
                    .collect();
                push_chunk(&mut children, b"nGRP", &group_body(node, &kids));
            }
            VoxKind::Shape => {
                let used: Vec<_> = node
                    .models
                    .iter()
                    .filter(|(model, _)| (0..models.len()).contains(&(*model as usize)))
                    .cloned()
                    .collect();
                if used.is_empty() {
                    continue;
                }
                push_chunk(&mut children, b"nSHP", &shape_body(node, &used));
            }
        }
    }
    for layer in &graph.layers {
        push_chunk(&mut children, b"LAYR", &layer_body(layer));
    }
    push_rgba(&mut children, palette);
    push_matl(&mut children, materials);
    // The objects went back to file space above; the pivots go with them.
    let mut skeleton = skeleton.clone();
    crate::rig::shift_pivots(&mut skeleton, graph.shift.map(|axis| -axis));
    push_rig(&mut children, &rig_saved, &skeleton)?;
    push_anims(&mut children, clips)?;
    if let Some(body) = crate::script::encode_chunk(objects) {
        push_chunk(&mut children, b"MVSC", &body);
    }
    Ok(wrap_vox(children))
}

pub(crate) fn pack_model(object: &Volume) -> Result<Option<(StoredModel, bool)>, String> {
    let empty = object.cells.values().all(|slot| *slot == 0);
    if empty {
        if object.file.is_none() {
            return Ok(None);
        }
        let dims = if object.model_dims != (0, 0, 0) {
            [
                object.model_dims.0,
                object.model_dims.1,
                object.model_dims.2,
            ]
        } else if object.dims != (0, 0, 0) {
            [object.dims.0, object.dims.1, object.dims.2]
        } else {
            [1, 1, 1]
        };
        return Ok(Some((
            StoredModel {
                dims: [dims[0].max(1), dims[1].max(1), dims[2].max(1)],
                voxels: Vec::new(),
            },
            stored_rotation(object),
        )));
    }
    if let Some(voxels) = unbake_model(object) {
        return Ok(Some((
            StoredModel {
                dims: [
                    object.model_dims.0,
                    object.model_dims.1,
                    object.model_dims.2,
                ],
                voxels,
            },
            true,
        )));
    }
    let Some((min, dims)) = resolved_box(object) else {
        return Ok(None);
    };
    if dims[0] > 256 || dims[1] > 256 || dims[2] > 256 {
        return Err(format!(
            "scene exceeds .vox single-model limit of 256 per axis (bounds {}x{}x{})",
            dims[0], dims[1], dims[2]
        ));
    }
    let mut voxels: Vec<(i32, i32, i32, u8)> = object
        .cells
        .iter()
        .filter(|(_, slot)| **slot != 0)
        .map(|(cell, slot)| (cell.0 - min.0, cell.1 - min.1, cell.2 - min.2, *slot))
        .collect();
    voxels.sort_unstable();
    Ok(Some((StoredModel { dims, voxels }, false)))
}

pub(crate) fn editor_center(object: &Volume, keep_rotation: bool) -> Option<[i32; 3]> {
    if keep_rotation {
        return Some([object.pivot.0, object.pivot.1, object.pivot.2]);
    }
    let (min, dims) = resolved_box(object)?;
    let half = [dims[0] / 2, dims[1] / 2, dims[2] / 2];
    Some([
        object.translation.0 + min.0 + half[0],
        object.translation.1 + min.1 + half[1],
        object.translation.2 + min.2 + half[2],
    ])
}

pub(crate) fn file_local(
    object: &Volume,
    parent_t: [i32; 3],
    parent_rot: [[i32; 3]; 3],
    shift: [i32; 3],
    keep_rotation: bool,
) -> Option<([i32; 3], [[i32; 3]; 3])> {
    let center = editor_center(object, keep_rotation)?;
    let file_center = [
        center[0] - shift[0],
        center[1] - shift[1],
        center[2] - shift[2],
    ];
    let delta = [
        file_center[0] - parent_t[0],
        file_center[1] - parent_t[1],
        file_center[2] - parent_t[2],
    ];
    let local_t = rot_apply(rot_transpose(parent_rot), delta);
    let local_r = if keep_rotation {
        rot_mul(rot_transpose(parent_rot), object.rot)
    } else {
        ROT_IDENTITY
    };
    Some((local_t, local_r))
}

pub(crate) fn rewrite_leaf(
    node: &mut VoxNode,
    object: &Volume,
    link: &FileLink,
    shift: [i32; 3],
    keep_rotation: bool,
) {
    upsert_pair(
        &mut node.attrs,
        "_hidden",
        object.hidden.then(|| "1".to_string()),
    );
    if object.name != link.name {
        upsert_pair(
            &mut node.attrs,
            "_name",
            (!object.name.is_empty()).then(|| object.name.clone()),
        );
    }
    let Some((local_t, local_r)) =
        file_local(object, link.parent_t, link.parent_rot, shift, keep_rotation)
    else {
        return;
    };
    if node.frames.is_empty() {
        node.frames.push(Vec::new());
    }
    let frame = &mut node.frames[0];
    upsert_pair(
        frame,
        "_t",
        (local_t != [0, 0, 0]).then(|| format!("{} {} {}", local_t[0], local_t[1], local_t[2])),
    );
    upsert_pair(
        frame,
        "_r",
        (!rot_is_identity(local_r)).then(|| rot_to_byte(local_r).to_string()),
    );
    node.t = local_t;
    node.rot = rot_to_byte(local_r);
}

pub(crate) fn group_space(nodes: &[VoxNode]) -> ([i32; 3], [[i32; 3]; 3], Option<i32>) {
    if let Some(root) = nodes
        .iter()
        .find(|node| node.id == 0 && node.kind == VoxKind::Transform)
    {
        if nodes
            .iter()
            .any(|node| node.id == root.child && node.kind == VoxKind::Group)
        {
            return (root.t, rot_from_byte(root.rot), Some(root.child));
        }
    }
    if let Some(group) = nodes.iter().find(|node| node.kind == VoxKind::Group) {
        return ([0, 0, 0], ROT_IDENTITY, Some(group.id));
    }
    ([0, 0, 0], ROT_IDENTITY, None)
}

pub(crate) fn ensure_group(nodes: &mut Vec<VoxNode>, next_id: &mut i32) -> i32 {
    if let Some(id) = group_space(nodes).2 {
        return id;
    }
    let group_id = *next_id;
    *next_id += 1;
    let previous = nodes
        .iter()
        .find(|node| node.id == 0 && node.kind == VoxKind::Transform)
        .map(|node| node.child);
    if let Some(root) = nodes
        .iter_mut()
        .find(|node| node.id == 0 && node.kind == VoxKind::Transform)
    {
        root.child = group_id;
    }
    nodes.push(VoxNode {
        id: group_id,
        kind: VoxKind::Group,
        child: -1,
        children: previous.into_iter().collect(),
        model_id: -1,
        models: Vec::new(),
        t: [0, 0, 0],
        rot: 4,
        layer: -1,
        name: String::new(),
        attrs: Vec::new(),
        frames: Vec::new(),
    });
    group_id
}

pub(crate) fn shape_node(id: i32, model: i32) -> VoxNode {
    VoxNode {
        id,
        kind: VoxKind::Shape,
        child: -1,
        children: Vec::new(),
        model_id: model,
        models: vec![(model, Vec::new())],
        t: [0, 0, 0],
        rot: 4,
        layer: -1,
        name: String::new(),
        attrs: Vec::new(),
        frames: Vec::new(),
    }
}

pub(crate) fn transform_node(
    id: i32,
    child: i32,
    object: &Volume,
    parent_t: [i32; 3],
    parent_rot: [[i32; 3]; 3],
    shift: [i32; 3],
    keep_rotation: bool,
) -> VoxNode {
    let (local_t, local_r) = file_local(object, parent_t, parent_rot, shift, keep_rotation)
        .unwrap_or(([0, 0, 0], ROT_IDENTITY));
    let mut attrs = Vec::new();
    if !object.name.is_empty() {
        attrs.push(("_name".to_string(), object.name.clone()));
    }
    if object.hidden {
        attrs.push(("_hidden".to_string(), "1".to_string()));
    }
    let mut frame = Vec::new();
    if local_t != [0, 0, 0] {
        frame.push((
            "_t".to_string(),
            format!("{} {} {}", local_t[0], local_t[1], local_t[2]),
        ));
    }
    if !rot_is_identity(local_r) {
        frame.push(("_r".to_string(), rot_to_byte(local_r).to_string()));
    }
    VoxNode {
        id,
        kind: VoxKind::Transform,
        child,
        children: Vec::new(),
        model_id: -1,
        models: Vec::new(),
        t: local_t,
        rot: rot_to_byte(local_r),
        layer: -1,
        name: object.name.clone(),
        attrs,
        frames: vec![frame],
    }
}

pub(crate) fn push_model(out: &mut Vec<u8>, model: &StoredModel) -> Result<(), String> {
    let dims = [
        model.dims[0].max(1),
        model.dims[1].max(1),
        model.dims[2].max(1),
    ];
    if dims[0] > 256 || dims[1] > 256 || dims[2] > 256 {
        return Err(format!(
            "scene exceeds .vox single-model limit of 256 per axis (bounds {}x{}x{})",
            dims[0], dims[1], dims[2]
        ));
    }
    let mut size = Vec::new();
    for dim in dims {
        size.extend_from_slice(&(dim as u32).to_le_bytes());
    }
    push_chunk(out, b"SIZE", &size);
    let mut xyzi = Vec::with_capacity(4 + model.voxels.len() * 4);
    xyzi.extend_from_slice(&(model.voxels.len() as u32).to_le_bytes());
    for (x, y, z, slot) in &model.voxels {
        if !(0..256).contains(x) || !(0..256).contains(y) || !(0..256).contains(z) {
            return Err("model voxels escape the .vox 256-per-axis local box".into());
        }
        xyzi.extend_from_slice(&[*x as u8, *y as u8, *z as u8, *slot]);
    }
    push_chunk(out, b"XYZI", &xyzi);
    Ok(())
}

pub(crate) fn push_pairs(out: &mut Vec<u8>, pairs: &[(String, String)]) {
    push_i32(out, pairs.len() as i32);
    for (key, value) in pairs {
        push_string(out, key);
        push_string(out, value);
    }
}

pub(crate) fn transform_body(node: &VoxNode) -> Vec<u8> {
    let mut body = Vec::new();
    push_i32(&mut body, node.id);
    push_pairs(&mut body, &node.attrs);
    push_i32(&mut body, node.child);
    push_i32(&mut body, -1);
    push_i32(&mut body, node.layer);
    let frames = if node.frames.is_empty() {
        vec![Vec::new()]
    } else {
        node.frames.clone()
    };
    push_i32(&mut body, frames.len() as i32);
    for frame in &frames {
        push_pairs(&mut body, frame);
    }
    body
}

pub(crate) fn group_body(node: &VoxNode, children: &[i32]) -> Vec<u8> {
    let mut body = Vec::new();
    push_i32(&mut body, node.id);
    push_pairs(&mut body, &node.attrs);
    push_i32(&mut body, children.len() as i32);
    for child in children {
        push_i32(&mut body, *child);
    }
    body
}

pub(crate) fn shape_body(node: &VoxNode, models: &[(i32, Vec<(String, String)>)]) -> Vec<u8> {
    let mut body = Vec::new();
    push_i32(&mut body, node.id);
    push_pairs(&mut body, &node.attrs);
    push_i32(&mut body, models.len() as i32);
    for (model, attrs) in models {
        push_i32(&mut body, *model);
        push_pairs(&mut body, attrs);
    }
    body
}

pub(crate) fn layer_body(layer: &VoxLayer) -> Vec<u8> {
    let mut body = Vec::new();
    push_i32(&mut body, layer.id);
    push_pairs(&mut body, &layer.attrs);
    push_i32(&mut body, -1);
    body
}

/// Every non-empty object, each as its own model. Local cells are shifted
/// so the SIZE box origin is zero. That box keeps an empty margin when the
/// editor grew the object. The leaf translation is the model centre
/// (`SIZE / 2`, integer division), which is what MagicaVoxel stores in
/// `nTRN`. The editor's position is that centre minus the rotated offset,
/// the world cell of local `(0, 0, 0)`. A loaded rotation is written back
/// as `_r`.
pub(crate) fn save_scene(
    objects: &[Volume],
    palette: &[[u8; 4]; 256],
    materials: &[Material; 256],
    skeleton: &Skeleton,
    clips: &[crate::rig::Clip],
) -> Result<Vec<u8>, String> {
    struct Packed {
        name: String,
        voxels: Vec<(i32, i32, i32, u8)>,
        dims: [i32; 3],
        leaf_t: [i32; 3],
        rot: [[i32; 3]; 3],
        trn: i32,
        bone: Option<String>,
        hidden: bool,
    }
    let mut packed = Vec::new();
    for object in objects {
        let mut voxels: Vec<(i32, i32, i32, u8)> = object
            .cells
            .iter()
            .filter(|(_, index)| **index != 0)
            .map(|(cell, index)| (cell.0, cell.1, cell.2, *index))
            .collect();
        if voxels.is_empty() {
            continue;
        }
        if let Some(file_voxels) = unbake_model(object) {
            let dims = [
                object.model_dims.0,
                object.model_dims.1,
                object.model_dims.2,
            ];
            packed.push(Packed {
                name: object.name.clone(),
                voxels: file_voxels,
                dims,
                leaf_t: [object.pivot.0, object.pivot.1, object.pivot.2],
                rot: object.rot,
                trn: (2 + packed.len() * 2) as i32,
                bone: object.bone.clone(),
                hidden: object.hidden,
            });
            continue;
        }
        let Some((min, dims)) = resolved_box(object) else {
            continue;
        };
        if dims[0] > 256 || dims[1] > 256 || dims[2] > 256 {
            return Err(format!(
                "scene exceeds .vox single-model limit of 256 per axis (bounds {}x{}x{})",
                dims[0], dims[1], dims[2]
            ));
        }
        for voxel in &mut voxels {
            voxel.0 -= min.0;
            voxel.1 -= min.1;
            voxel.2 -= min.2;
        }
        voxels.sort_unstable();
        let half = [dims[0] / 2, dims[1] / 2, dims[2] / 2];
        packed.push(Packed {
            name: object.name.clone(),
            voxels,
            dims,
            leaf_t: [
                object.translation.0 + min.0 + half[0],
                object.translation.1 + min.1 + half[1],
                object.translation.2 + min.2 + half[2],
            ],
            rot: ROT_IDENTITY,
            trn: (2 + packed.len() * 2) as i32,
            bone: object.bone.clone(),
            hidden: object.hidden,
        });
    }
    if packed.is_empty() {
        return Err("cannot save an empty grid".into());
    }

    let mut children = Vec::new();
    for model in &packed {
        let mut size = Vec::new();
        size.extend_from_slice(&(model.dims[0] as u32).to_le_bytes());
        size.extend_from_slice(&(model.dims[1] as u32).to_le_bytes());
        size.extend_from_slice(&(model.dims[2] as u32).to_le_bytes());
        push_chunk(&mut children, b"SIZE", &size);
        let mut xyzi = Vec::with_capacity(4 + model.voxels.len() * 4);
        xyzi.extend_from_slice(&(model.voxels.len() as u32).to_le_bytes());
        for (x, y, z, index) in &model.voxels {
            xyzi.extend_from_slice(&[*x as u8, *y as u8, *z as u8, *index]);
        }
        push_chunk(&mut children, b"XYZI", &xyzi);
    }

    let mut kids = Vec::new();
    for index in 0..packed.len() {
        kids.push((2 + index * 2) as i32);
    }
    push_chunk(
        &mut children,
        b"nTRN",
        &trn_body(0, 1, [0, 0, 0], ROT_IDENTITY, "", false),
    );
    push_chunk(&mut children, b"nGRP", &grp_body(1, &kids));
    for (index, model) in packed.iter().enumerate() {
        let trn = (2 + index * 2) as i32;
        let shp = trn + 1;
        push_chunk(
            &mut children,
            b"nTRN",
            &trn_body(trn, shp, model.leaf_t, model.rot, &model.name, model.hidden),
        );
        push_chunk(&mut children, b"nSHP", &shp_body(shp, index as i32));
    }
    push_rgba(&mut children, palette);
    push_matl(&mut children, materials);
    let rig_saved: Vec<(i32, String, Option<String>)> = packed
        .iter()
        .map(|model| (model.trn, model.name.clone(), model.bone.clone()))
        .collect();
    push_rig(&mut children, &rig_saved, skeleton)?;
    push_anims(&mut children, clips)?;
    if let Some(body) = crate::script::encode_chunk(objects) {
        push_chunk(&mut children, b"MVSC", &body);
    }
    Ok(wrap_vox(children))
}

fn push_anims(children: &mut Vec<u8>, clips: &[crate::rig::Clip]) -> Result<(), String> {
    if let Some(body) = crate::rig::encode_clips(clips)? {
        push_chunk(children, b"MVAN", &body);
    }
    Ok(())
}

pub(crate) fn push_rig(
    children: &mut Vec<u8>,
    saved: &[(i32, String, Option<String>)],
    skeleton: &Skeleton,
) -> Result<(), String> {
    let objects: Vec<crate::rig::ObjectRef> = saved
        .iter()
        .map(|(trn, name, bone)| crate::rig::ObjectRef {
            trn: *trn,
            name,
            bone: bone.as_deref(),
        })
        .collect();
    if let Some(body) = crate::rig::encode_chunk(&objects, skeleton)? {
        push_chunk(children, b"MVRG", &body);
    }
    Ok(())
}

pub(crate) fn wrap_vox(children: Vec<u8>) -> Vec<u8> {
    let mut file = Vec::new();
    file.extend_from_slice(b"VOX ");
    file.extend_from_slice(&150u32.to_le_bytes());
    file.extend_from_slice(b"MAIN");
    file.extend_from_slice(&0u32.to_le_bytes());
    file.extend_from_slice(&(children.len() as u32).to_le_bytes());
    file.extend(children);
    file
}

/// Add a chunk to the end of a file `wrap_vox` made and grow the MAIN
/// children size to match.
pub(crate) fn append_chunk(file: &mut Vec<u8>, id: &[u8; 4], content: &[u8]) {
    let before = file.len();
    push_chunk(file, id, content);
    let children = u32::from_le_bytes(file[16..20].try_into().unwrap());
    let grown = children + (file.len() - before) as u32;
    file[16..20].copy_from_slice(&grown.to_le_bytes());
}

pub(crate) fn push_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn push_dict(out: &mut Vec<u8>, pairs: &[(&str, &str)]) {
    push_i32(out, pairs.len() as i32);
    for (key, value) in pairs {
        push_string(out, key);
        push_string(out, value);
    }
}

pub(crate) fn trn_body(
    id: i32,
    child: i32,
    t: [i32; 3],
    rot: [[i32; 3]; 3],
    name: &str,
    hidden: bool,
) -> Vec<u8> {
    let mut body = Vec::new();
    push_i32(&mut body, id);
    let mut attrs: Vec<(&str, &str)> = Vec::new();
    if !name.is_empty() {
        attrs.push(("_name", name));
    }
    if hidden {
        attrs.push(("_hidden", "1"));
    }
    push_dict(&mut body, &attrs);
    push_i32(&mut body, child);
    push_i32(&mut body, -1);
    push_i32(&mut body, -1);
    push_i32(&mut body, 1);
    let translation = format!("{} {} {}", t[0], t[1], t[2]);
    let byte = rot_to_byte(rot).to_string();
    let mut frame: Vec<(&str, &str)> = Vec::new();
    if t != [0, 0, 0] {
        frame.push(("_t", &translation));
    }
    if !rot_is_identity(rot) {
        frame.push(("_r", &byte));
    }
    push_dict(&mut body, &frame);
    body
}

pub(crate) fn stored_rotation(object: &Volume) -> bool {
    !rot_is_identity(object.rot) && object.model_dims != (0, 0, 0)
}

/// File-space voxels for a rotated model. `None` when the orientation is
/// identity, or when a later edit no longer fits the original SIZE.
pub(crate) fn unbake_model(object: &Volume) -> Option<Vec<(i32, i32, i32, u8)>> {
    if rot_is_identity(object.rot) || object.model_dims == (0, 0, 0) {
        return None;
    }
    let half = [
        object.model_dims.0 / 2,
        object.model_dims.1 / 2,
        object.model_dims.2 / 2,
    ];
    let inverse = rot_transpose(object.rot);
    let mut voxels = Vec::new();
    for (cell, slot) in &object.cells {
        if *slot == 0 {
            continue;
        }
        let delta = [
            cell.0 + object.translation.0 - object.pivot.0,
            cell.1 + object.translation.1 - object.pivot.1,
            cell.2 + object.translation.2 - object.pivot.2,
        ];
        let spun = rot_apply(inverse, delta);
        let local = [spun[0] + half[0], spun[1] + half[1], spun[2] + half[2]];
        if local[0] < 0
            || local[1] < 0
            || local[2] < 0
            || local[0] >= object.model_dims.0
            || local[1] >= object.model_dims.1
            || local[2] >= object.model_dims.2
        {
            return None;
        }
        voxels.push((local[0], local[1], local[2], *slot));
    }
    if voxels.is_empty() {
        return None;
    }
    voxels.sort_unstable();
    Some(voxels)
}

pub(crate) fn grp_body(id: i32, children: &[i32]) -> Vec<u8> {
    let mut body = Vec::new();
    push_i32(&mut body, id);
    push_dict(&mut body, &[]);
    push_i32(&mut body, children.len() as i32);
    for child in children {
        push_i32(&mut body, *child);
    }
    body
}

pub(crate) fn shp_body(id: i32, model: i32) -> Vec<u8> {
    let mut body = Vec::new();
    push_i32(&mut body, id);
    push_dict(&mut body, &[]);
    push_i32(&mut body, 1);
    push_i32(&mut body, model);
    push_dict(&mut body, &[]);
    body
}

pub(crate) fn push_chunk(out: &mut Vec<u8>, id: &[u8; 4], content: &[u8]) {
    out.extend_from_slice(id);
    out.extend_from_slice(&(content.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(content);
}

pub(crate) fn push_rgba(out: &mut Vec<u8>, palette: &[[u8; 4]; 256]) {
    let mut body = Vec::with_capacity(1024);
    for slot in 1..=255 {
        body.extend_from_slice(&palette[slot]);
    }
    body.extend_from_slice(&[0, 0, 0, 0]);
    push_chunk(out, b"RGBA", &body);
}

pub(crate) fn material_is_default(material: &Material) -> bool {
    material.motion == Motion::None
        && Material {
            motion_amount: 1.0,
            ..*material
        } == Material::default()
}

pub(crate) fn format_float(value: f32) -> String {
    // printf("%g") for the values the editor writes. Display round-trips
    // through strtof for those (1, 0.5, 0.25); bit-identical files are not
    // required, a reload of the same numbers is.
    format!("{value}")
}

pub(crate) fn push_string(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
}

pub(crate) fn matl_type(ty: MaterialType) -> &'static str {
    match ty {
        MaterialType::Metal => "_metal",
        MaterialType::Glass => "_glass",
        MaterialType::Emissive => "_emit",
        MaterialType::Diffuse => "_diffuse",
    }
}

pub(crate) fn push_matl(out: &mut Vec<u8>, materials: &[Material; 256]) {
    for id in 1..=255 {
        let material = materials[id];
        if material_is_default(&material) {
            continue;
        }
        let mut pairs = vec![
            ("_type", matl_type(material.ty).to_string()),
            ("_rough", format_float(material.roughness)),
            ("_metal", format_float(material.metallic)),
            ("_ior", format_float(material.ior - 1.0)),
        ];
        if material.transparency != 0.0 {
            pairs.push(("_alpha", format_float(material.transparency)));
        }
        if material.ty == MaterialType::Emissive {
            pairs.push(("_emit", format_float(material.emission)));
            pairs.push(("_flux", format_float((material.emission_power - 1) as f32)));
        }
        if material.motion != Motion::None {
            pairs.push(("_motion", material.motion.as_str().to_string()));
            pairs.push(("_motion_amount", format_float(material.motion_amount)));
        }
        let mut body = Vec::new();
        body.extend_from_slice(&(id as u32).to_le_bytes());
        body.extend_from_slice(&(pairs.len() as u32).to_le_bytes());
        for (key, value) in &pairs {
            push_string(&mut body, key);
            push_string(&mut body, value);
        }
        push_chunk(out, b"MATL", &body);
    }
}
