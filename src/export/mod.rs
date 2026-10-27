//! glTF 2.0 binary (.glb) of exposed faces.
//!
//! Coplanar faces of one palette slot become one rectangle when merging is
//! on. Translations are baked into the vertices. Positions are glTF Y-up
//! `(x, z, -y)`. The default material is a 256-wide palette texture;
//! `vertex_colors` stores a linear `COLOR_0` instead.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{json, Value};

use crate::state::{Cell, Session};

mod quads;

use quads::build_with;

pub struct MeshExport {
    /// `COLOR_0` on each vertex. Otherwise a palette texture and `TEXCOORD_0`.
    pub vertex_colors: bool,
    /// Greedy-merge coplanar faces that share a palette slot.
    pub merge: bool,
    /// Include objects marked hidden. Default export leaves them out.
    pub export_hidden: bool,
    /// One mesh node per object, still sharing one skin. Default is one mesh.
    pub split_objects: bool,
}

struct Built {
    positions: Vec<f32>,
    normals: Vec<f32>,
    /// Vertex colors (`VEC4`) or texture coordinates (`VEC2`).
    extra: Vec<f32>,
    indices: Vec<u32>,
    /// Four joint indices per vertex. Empty when the document has no bones,
    /// so the rigid exporter does not read it.
    joints: Vec<u16>,
    /// `TEXCOORD_1` pairs (sway, bob). Empty when no slot has motion, so the
    /// glTF bytes stay the same as a still document.
    motion: Vec<f32>,
    min: [f32; 3],
    max: [f32; 3],
    blend: bool,
}

fn encode(session: &Session, options: &MeshExport) -> Result<Vec<u8>, String> {
    if options.split_objects {
        return encode_split(session, options);
    }
    if !session.skeleton.bones.is_empty() {
        return encode_skinned(session, options);
    }
    let built = build_with(session, options);
    let quads = (built.indices.len() / 6) as u32;
    if quads == 0 {
        return Err("Nothing to export.".into());
    }
    let verts = (built.positions.len() / 3) as u32;
    let pos_bytes = built.positions.len() * 4;
    let nrm_bytes = built.normals.len() * 4;
    let extra_bytes = built.extra.len() * 4;
    let motion_bytes = built.motion.len() * 4;
    let idx_bytes = built.indices.len() * 4;
    let png = if options.vertex_colors {
        Vec::new()
    } else {
        palette_png(&session.palette)?
    };
    let mut bin = Vec::with_capacity(
        pos_bytes + nrm_bytes + extra_bytes + motion_bytes + idx_bytes + png.len(),
    );
    push_f32s(&mut bin, &built.positions);
    push_f32s(&mut bin, &built.normals);
    push_f32s(&mut bin, &built.extra);
    if motion_bytes > 0 {
        push_f32s(&mut bin, &built.motion);
    }
    for index in &built.indices {
        bin.extend_from_slice(&index.to_le_bytes());
    }
    let image_off = bin.len();
    bin.extend_from_slice(&png);
    pad(&mut bin, 0);

    let nrm_off = pos_bytes;
    let extra_off = nrm_off + nrm_bytes;
    let idx_off = extra_off + extra_bytes + motion_bytes;
    let color_name = if options.vertex_colors {
        "COLOR_0"
    } else {
        "TEXCOORD_0"
    };
    let extra_type = if options.vertex_colors {
        "VEC4"
    } else {
        "VEC2"
    };
    let alpha = if built.blend { "BLEND" } else { "OPAQUE" };
    let material_name = if options.vertex_colors {
        "voxel-vertex-color"
    } else {
        "voxel-palette"
    };
    let mut material = json!({
        "name": material_name,
        "pbrMetallicRoughness": {
            "baseColorFactor": [1.0, 1.0, 1.0, 1.0],
            "metallicFactor": 0.0,
            "roughnessFactor": 1.0
        },
        "alphaMode": alpha
    });
    if !options.vertex_colors {
        material["pbrMetallicRoughness"]["baseColorTexture"] = json!({"index": 0});
    }
    let mut attributes = serde_json::Map::new();
    attributes.insert("POSITION".into(), json!(0));
    attributes.insert("NORMAL".into(), json!(1));
    attributes.insert(color_name.into(), json!(2));
    if motion_bytes > 0 {
        attributes.insert("TEXCOORD_1".into(), json!(4));
    }
    let mut root = json!({
        "asset": asset_json(session),
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"name": "voxels", "mesh": 0}],
        "materials": [material],
        "meshes": [{
            "name": "voxels",
            "primitives": [{
                "attributes": attributes,
                "indices": 3,
                "material": 0,
                "mode": 4
            }]
        }],
        "accessors": [
            {
                "bufferView": 0,
                "componentType": 5126,
                "count": verts,
                "type": "VEC3",
                "min": built.min,
                "max": built.max
            },
            {"bufferView": 1, "componentType": 5126, "count": verts, "type": "VEC3"},
            {"bufferView": 2, "componentType": 5126, "count": verts, "type": extra_type},
            {
                "bufferView": 3,
                "componentType": 5125,
                "count": built.indices.len(),
                "type": "SCALAR"
            }
        ],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": pos_bytes, "target": 34962},
            {"buffer": 0, "byteOffset": nrm_off, "byteLength": nrm_bytes, "target": 34962},
            {"buffer": 0, "byteOffset": extra_off, "byteLength": extra_bytes, "target": 34962},
            {"buffer": 0, "byteOffset": idx_off, "byteLength": idx_bytes, "target": 34963}
        ],
        "buffers": [{"byteLength": bin.len()}]
    });
    if motion_bytes > 0 {
        root["accessors"]
            .as_array_mut()
            .expect("accessors")
            .push(json!({
                "bufferView": 4,
                "componentType": 5126,
                "count": verts,
                "type": "VEC2"
            }));
        root["bufferViews"]
            .as_array_mut()
            .expect("buffer views")
            .push(json!({
                "buffer": 0,
                "byteOffset": extra_off + extra_bytes,
                "byteLength": motion_bytes,
                "target": 34962
            }));
    }
    if !options.vertex_colors {
        root["textures"] = json!([{"sampler": 0, "source": 0}]);
        root["samplers"] = json!([{
            "magFilter": 9728,
            "minFilter": 9728,
            "wrapS": 33071,
            "wrapT": 33071
        }]);
        let image_view = if motion_bytes > 0 { 5 } else { 4 };
        root["images"] = json!([{"bufferView": image_view, "mimeType": "image/png"}]);
        root["bufferViews"]
            .as_array_mut()
            .expect("buffer views")
            .push(json!({
                "buffer": 0,
                "byteOffset": image_off,
                "byteLength": png.len()
            }));
    }
    let json = root;
    let mut json_bytes = serde_json::to_vec(&json).map_err(|err| err.to_string())?;
    pad(&mut json_bytes, b' ');

    let total = 12 + 8 + json_bytes.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    Ok(out)
}

fn encode_skinned(session: &Session, options: &MeshExport) -> Result<Vec<u8>, String> {
    for object in &session.objects {
        if object.cells.values().all(|slot| *slot == 0) {
            continue;
        }
        if object.hidden && !options.export_hidden {
            continue;
        }
        session.skeleton.joint_or_root(object.bone.as_deref())?;
    }
    let built = build_with(session, options);
    let quads = (built.indices.len() / 6) as u32;
    if quads == 0 {
        return Err("Nothing to export.".into());
    }
    let verts = (built.positions.len() / 3) as u32;
    let poses = crate::rig::bind_pose(&session.skeleton)?;
    let pos_bytes = built.positions.len() * 4;
    let nrm_bytes = built.normals.len() * 4;
    let extra_bytes = built.extra.len() * 4;
    let motion_bytes = built.motion.len() * 4;
    let joint_bytes = built.joints.len() * 2;
    let weight_bytes = verts as usize * 16;
    let idx_bytes = built.indices.len() * 4;
    let ibm_bytes = poses.len() * 64;
    let png = if options.vertex_colors {
        Vec::new()
    } else {
        palette_png(&session.palette)?
    };
    let mut bin = Vec::new();
    push_f32s(&mut bin, &built.positions);
    push_f32s(&mut bin, &built.normals);
    push_f32s(&mut bin, &built.extra);
    if motion_bytes > 0 {
        push_f32s(&mut bin, &built.motion);
    }
    for joint in &built.joints {
        bin.extend_from_slice(&joint.to_le_bytes());
    }
    for _ in 0..verts {
        push_f32s(&mut bin, &[1.0, 0.0, 0.0, 0.0]);
    }
    for index in &built.indices {
        bin.extend_from_slice(&index.to_le_bytes());
    }
    for pose in &poses {
        push_f32s(&mut bin, &pose.inverse_bind);
    }
    let image_off = bin.len();
    bin.extend_from_slice(&png);
    pad(&mut bin, 0);

    let nrm_off = pos_bytes;
    let extra_off = nrm_off + nrm_bytes;
    let joint_off = extra_off + extra_bytes + motion_bytes;
    let weight_off = joint_off + joint_bytes;
    let idx_off = weight_off + weight_bytes;
    let ibm_off = idx_off + idx_bytes;
    let color_name = if options.vertex_colors {
        "COLOR_0"
    } else {
        "TEXCOORD_0"
    };
    let extra_type = if options.vertex_colors {
        "VEC4"
    } else {
        "VEC2"
    };
    let alpha = if built.blend { "BLEND" } else { "OPAQUE" };
    let material_name = if options.vertex_colors {
        "voxel-vertex-color"
    } else {
        "voxel-palette"
    };
    let mut material = json!({
        "name": material_name,
        "pbrMetallicRoughness": {
            "baseColorFactor": [1.0, 1.0, 1.0, 1.0],
            "metallicFactor": 0.0,
            "roughnessFactor": 1.0
        },
        "alphaMode": alpha
    });
    if !options.vertex_colors {
        material["pbrMetallicRoughness"]["baseColorTexture"] = json!({"index": 0});
    }
    let mut attributes = serde_json::Map::new();
    attributes.insert("POSITION".into(), json!(0));
    attributes.insert("NORMAL".into(), json!(1));
    attributes.insert(color_name.into(), json!(2));
    attributes.insert("JOINTS_0".into(), json!(3));
    attributes.insert("WEIGHTS_0".into(), json!(4));
    if motion_bytes > 0 {
        attributes.insert("TEXCOORD_1".into(), json!(7));
    }

    let mut nodes = vec![json!({"name": "voxels", "mesh": 0, "skin": 0})];
    for (index, pose) in poses.iter().enumerate() {
        let mut node = json!({
            "name": session.skeleton.bones[index].name,
            "translation": pose.translation,
            "rotation": pose.rotation
        });
        let children: Vec<usize> = session
            .skeleton
            .child_indices(index)
            .into_iter()
            .map(|child| child + 1)
            .collect();
        if !children.is_empty() {
            node["children"] = json!(children);
        }
        nodes.push(node);
    }
    let mut scene_nodes = vec![0usize];
    let roots: Vec<usize> = session
        .skeleton
        .root_indices()
        .into_iter()
        .map(|index| index + 1)
        .collect();
    scene_nodes.extend(roots.iter().copied());
    let joints: Vec<usize> = (1..=session.skeleton.bones.len()).collect();
    let mut skin = json!({
        "joints": joints,
        "inverseBindMatrices": 6
    });
    if let Some(root) = roots.first() {
        skin["skeleton"] = json!(root);
    }

    let mut root = json!({
        "asset": asset_json(session),
        "scene": 0,
        "scenes": [{"nodes": scene_nodes}],
        "nodes": nodes,
        "skins": [skin],
        "materials": [material],
        "meshes": [{
            "name": "voxels",
            "primitives": [{
                "attributes": attributes,
                "indices": 5,
                "material": 0,
                "mode": 4
            }]
        }],
        "accessors": [
            {
                "bufferView": 0,
                "componentType": 5126,
                "count": verts,
                "type": "VEC3",
                "min": built.min,
                "max": built.max
            },
            {"bufferView": 1, "componentType": 5126, "count": verts, "type": "VEC3"},
            {"bufferView": 2, "componentType": 5126, "count": verts, "type": extra_type},
            {"bufferView": 3, "componentType": 5123, "count": verts, "type": "VEC4"},
            {"bufferView": 4, "componentType": 5126, "count": verts, "type": "VEC4"},
            {
                "bufferView": 5,
                "componentType": 5125,
                "count": built.indices.len(),
                "type": "SCALAR"
            },
            {
                "bufferView": 6,
                "componentType": 5126,
                "count": poses.len(),
                "type": "MAT4"
            }
        ],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": pos_bytes, "target": 34962},
            {"buffer": 0, "byteOffset": nrm_off, "byteLength": nrm_bytes, "target": 34962},
            {"buffer": 0, "byteOffset": extra_off, "byteLength": extra_bytes, "target": 34962},
            {"buffer": 0, "byteOffset": joint_off, "byteLength": joint_bytes, "target": 34962},
            {"buffer": 0, "byteOffset": weight_off, "byteLength": weight_bytes, "target": 34962},
            {"buffer": 0, "byteOffset": idx_off, "byteLength": idx_bytes, "target": 34963},
            {"buffer": 0, "byteOffset": ibm_off, "byteLength": ibm_bytes}
        ],
        "buffers": [{"byteLength": bin.len()}]
    });
    if motion_bytes > 0 {
        root["accessors"]
            .as_array_mut()
            .expect("accessors")
            .push(json!({
                "bufferView": 7,
                "componentType": 5126,
                "count": verts,
                "type": "VEC2"
            }));
        root["bufferViews"]
            .as_array_mut()
            .expect("buffer views")
            .push(json!({
                "buffer": 0,
                "byteOffset": extra_off + extra_bytes,
                "byteLength": motion_bytes,
                "target": 34962
            }));
    }
    if !options.vertex_colors {
        root["textures"] = json!([{"sampler": 0, "source": 0}]);
        root["samplers"] = json!([{
            "magFilter": 9728,
            "minFilter": 9728,
            "wrapS": 33071,
            "wrapT": 33071
        }]);
        let image_view = if motion_bytes > 0 { 8 } else { 7 };
        root["images"] = json!([{"bufferView": image_view, "mimeType": "image/png"}]);
        root["bufferViews"]
            .as_array_mut()
            .expect("buffer views")
            .push(json!({
                "buffer": 0,
                "byteOffset": image_off,
                "byteLength": png.len()
            }));
    }
    append_animations(&mut root, &mut bin, session, 1)?;
    let mut json_bytes = serde_json::to_vec(&root).map_err(|err| err.to_string())?;
    pad(&mut json_bytes, b' ');
    let total = 12 + 8 + json_bytes.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    Ok(out)
}

fn encode_split(session: &Session, options: &MeshExport) -> Result<Vec<u8>, String> {
    let skinned = !session.skeleton.bones.is_empty();
    let mut parts = Vec::new();
    for index in 0..session.objects.len() {
        let object = &session.objects[index];
        if object.cells.values().all(|slot| *slot == 0) {
            continue;
        }
        if object.hidden && !options.export_hidden {
            continue;
        }
        if skinned {
            session.skeleton.joint_or_root(object.bone.as_deref())?;
        }
        let built = quads::build_one(session, options, index);
        if built.indices.is_empty() {
            continue;
        }
        parts.push((object.name.clone(), built));
    }
    if parts.is_empty() {
        return Err("Nothing to export.".into());
    }
    let poses = if skinned {
        crate::rig::bind_pose(&session.skeleton)?
    } else {
        Vec::new()
    };
    let png = if options.vertex_colors {
        Vec::new()
    } else {
        palette_png(&session.palette)?
    };
    let with_motion = parts.iter().any(|(_, built)| !built.motion.is_empty());
    let color_name = if options.vertex_colors {
        "COLOR_0"
    } else {
        "TEXCOORD_0"
    };
    let extra_type = if options.vertex_colors {
        "VEC4"
    } else {
        "VEC2"
    };
    let mut bin = Vec::new();
    let mut accessors = Vec::new();
    let mut views = Vec::new();
    let mut meshes = Vec::new();
    let mut nodes = Vec::new();
    let mut blend = false;
    // Node names must not repeat a joint's or another part's: Godot makes every node name in
    // the scene unique and would rename the bone (`jaw` -> `jaw_2`), so the mesh node gives way.
    let mut taken: std::collections::HashSet<String> = if skinned {
        session
            .skeleton
            .bones
            .iter()
            .map(|bone| bone.name.clone())
            .collect()
    } else {
        Default::default()
    };
    for (name, built) in &parts {
        blend |= built.blend;
        let verts = (built.positions.len() / 3) as u32;
        let pos_off = bin.len();
        push_f32s(&mut bin, &built.positions);
        let nrm_off = bin.len();
        push_f32s(&mut bin, &built.normals);
        let extra_off = bin.len();
        push_f32s(&mut bin, &built.extra);
        let motion_off = bin.len();
        if with_motion {
            push_f32s(&mut bin, &built.motion);
        }
        let joint_off = bin.len();
        if skinned {
            for joint in &built.joints {
                bin.extend_from_slice(&joint.to_le_bytes());
            }
        }
        let weight_off = bin.len();
        if skinned {
            for _ in 0..verts {
                push_f32s(&mut bin, &[1.0, 0.0, 0.0, 0.0]);
            }
        }
        let idx_off = bin.len();
        for index in &built.indices {
            bin.extend_from_slice(&index.to_le_bytes());
        }
        let mut attributes = serde_json::Map::new();
        attributes.insert("POSITION".into(), json!(accessors.len()));
        views.push(json!({"buffer": 0, "byteOffset": pos_off, "byteLength": built.positions.len() * 4, "target": 34962}));
        accessors.push(json!({
            "bufferView": views.len() - 1,
            "componentType": 5126,
            "count": verts,
            "type": "VEC3",
            "min": built.min,
            "max": built.max
        }));
        attributes.insert("NORMAL".into(), json!(accessors.len()));
        views.push(json!({"buffer": 0, "byteOffset": nrm_off, "byteLength": built.normals.len() * 4, "target": 34962}));
        accessors.push(json!({"bufferView": views.len() - 1, "componentType": 5126, "count": verts, "type": "VEC3"}));
        attributes.insert(color_name.into(), json!(accessors.len()));
        views.push(json!({"buffer": 0, "byteOffset": extra_off, "byteLength": built.extra.len() * 4, "target": 34962}));
        accessors.push(json!({"bufferView": views.len() - 1, "componentType": 5126, "count": verts, "type": extra_type}));
        if skinned {
            attributes.insert("JOINTS_0".into(), json!(accessors.len()));
            views.push(json!({"buffer": 0, "byteOffset": joint_off, "byteLength": built.joints.len() * 2, "target": 34962}));
            accessors.push(json!({"bufferView": views.len() - 1, "componentType": 5123, "count": verts, "type": "VEC4"}));
            attributes.insert("WEIGHTS_0".into(), json!(accessors.len()));
            views.push(json!({"buffer": 0, "byteOffset": weight_off, "byteLength": verts as usize * 16, "target": 34962}));
            accessors.push(json!({"bufferView": views.len() - 1, "componentType": 5126, "count": verts, "type": "VEC4"}));
        }
        if with_motion {
            attributes.insert("TEXCOORD_1".into(), json!(accessors.len()));
            views.push(json!({"buffer": 0, "byteOffset": motion_off, "byteLength": built.motion.len() * 4, "target": 34962}));
            accessors.push(json!({"bufferView": views.len() - 1, "componentType": 5126, "count": verts, "type": "VEC2"}));
        }
        let indices = accessors.len();
        views.push(json!({"buffer": 0, "byteOffset": idx_off, "byteLength": built.indices.len() * 4, "target": 34963}));
        accessors.push(json!({"bufferView": views.len() - 1, "componentType": 5125, "count": built.indices.len(), "type": "SCALAR"}));
        meshes.push(json!({
            "name": name,
            "primitives": [{
                "attributes": attributes,
                "indices": indices,
                "material": 0,
                "mode": 4
            }]
        }));
        let mut node = json!({"name": part_node_name(name, &mut taken), "mesh": nodes.len()});
        if skinned {
            node["skin"] = json!(0);
        }
        nodes.push(node);
    }
    let mut skins = Vec::new();
    let mut scene_nodes: Vec<usize> = (0..parts.len()).collect();
    if skinned {
        let ibm_off = bin.len();
        for pose in &poses {
            push_f32s(&mut bin, &pose.inverse_bind);
        }
        let ibm = accessors.len();
        views.push(json!({"buffer": 0, "byteOffset": ibm_off, "byteLength": poses.len() * 64}));
        accessors.push(json!({"bufferView": views.len() - 1, "componentType": 5126, "count": poses.len(), "type": "MAT4"}));
        for (index, pose) in poses.iter().enumerate() {
            let mut node = json!({
                "name": session.skeleton.bones[index].name,
                "translation": pose.translation,
                "rotation": pose.rotation
            });
            let children: Vec<usize> = session
                .skeleton
                .child_indices(index)
                .into_iter()
                .map(|child| parts.len() + child)
                .collect();
            if !children.is_empty() {
                node["children"] = json!(children);
            }
            nodes.push(node);
        }
        let joints: Vec<usize> = (parts.len()..parts.len() + poses.len()).collect();
        let roots: Vec<usize> = session
            .skeleton
            .root_indices()
            .into_iter()
            .map(|index| parts.len() + index)
            .collect();
        let mut skin = json!({"joints": joints, "inverseBindMatrices": ibm});
        if let Some(root) = roots.first() {
            skin["skeleton"] = json!(root);
        }
        skins.push(skin);
        scene_nodes.extend(roots);
    }
    let image_off = bin.len();
    bin.extend_from_slice(&png);
    pad(&mut bin, 0);
    let alpha = if blend { "BLEND" } else { "OPAQUE" };
    let material_name = if options.vertex_colors {
        "voxel-vertex-color"
    } else {
        "voxel-palette"
    };
    let mut material = json!({
        "name": material_name,
        "pbrMetallicRoughness": {
            "baseColorFactor": [1.0, 1.0, 1.0, 1.0],
            "metallicFactor": 0.0,
            "roughnessFactor": 1.0
        },
        "alphaMode": alpha
    });
    if !options.vertex_colors {
        material["pbrMetallicRoughness"]["baseColorTexture"] = json!({"index": 0});
        views.push(json!({"buffer": 0, "byteOffset": image_off, "byteLength": png.len()}));
    }
    let mut root = json!({
        "asset": asset_json(session),
        "scene": 0,
        "scenes": [{"nodes": scene_nodes}],
        "nodes": nodes,
        "materials": [material],
        "meshes": meshes,
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{"byteLength": bin.len()}]
    });
    if skinned {
        root["skins"] = json!(skins);
    }
    if !options.vertex_colors {
        let image_view = views.len() - 1;
        root["textures"] = json!([{"sampler": 0, "source": 0}]);
        root["samplers"] = json!([{
            "magFilter": 9728,
            "minFilter": 9728,
            "wrapS": 33071,
            "wrapT": 33071
        }]);
        root["images"] = json!([{"bufferView": image_view, "mimeType": "image/png"}]);
    }
    if skinned {
        append_animations(&mut root, &mut bin, session, parts.len())?;
    }
    let mut json_bytes = serde_json::to_vec(&root).map_err(|err| err.to_string())?;
    pad(&mut json_bytes, b' ');
    let total = 12 + 8 + json_bytes.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    Ok(out)
}

/// `name`, or `name_mesh`, `name_mesh2`, ... when a joint or an earlier part already has it.
fn part_node_name(name: &str, taken: &mut std::collections::HashSet<String>) -> String {
    let mut candidate = name.to_string();
    let mut n = 1;
    while taken.contains(&candidate) {
        candidate = if n == 1 {
            format!("{name}_mesh")
        } else {
            format!("{name}_mesh{n}")
        };
        n += 1;
    }
    taken.insert(candidate.clone());
    candidate
}

fn append_animations(
    root: &mut serde_json::Value,
    bin: &mut Vec<u8>,
    session: &Session,
    joint_base: usize,
) -> Result<(), String> {
    if session.clips.is_empty() {
        return Ok(());
    }
    let bind = crate::rig::bind_pose(&session.skeleton)?;
    let mut animations = Vec::new();
    for clip in &session.clips {
        let mut samplers = Vec::new();
        let mut channels = Vec::new();
        for track in &clip.tracks {
            let bone = session.skeleton.index_of(&track.bone).ok_or_else(|| {
                format!("clip `{}` names unknown bone `{}`", clip.name, track.bone)
            })?;
            if track.keys.is_empty() {
                continue;
            }
            let times: Vec<f32> = track.keys.iter().map(|key| key.time).collect();
            let mut rotation = Vec::new();
            let mut translation = Vec::new();
            let mut scale = Vec::new();
            // A track whose keys never set a translation (or a rotation) leaves the joint at
            // its bind offset (or rotation): no channel, so an importer keeps the rest value.
            // A key without one in a track that has some repeats the bind value.
            let moves = track.keys.iter().any(|key| key.translation.is_some());
            let turns = track.keys.iter().any(|key| key.rotation.is_some());
            for key in &track.keys {
                let turn = key
                    .rotation
                    .map_or(bind[bone].rotation, crate::rig::gltf_rotation);
                rotation.extend_from_slice(&turn);
                let offset = key
                    .translation
                    .map_or(bind[bone].translation, crate::rig::gltf_vector);
                translation.extend_from_slice(&offset);
                scale.extend_from_slice(&crate::rig::gltf_scale(key.scale));
            }
            let time_acc = push_anim(root, bin, &times, "SCALAR", times.len(), true);
            let node = joint_base + bone;
            if moves {
                let trans_acc = push_anim(root, bin, &translation, "VEC3", track.keys.len(), false);
                samplers.push(
                    json!({"input": time_acc, "output": trans_acc, "interpolation": "LINEAR"}),
                );
                channels.push(json!({"sampler": samplers.len() - 1, "target": {"node": node, "path": "translation"}}));
            }
            if turns {
                let rot_acc = push_anim(root, bin, &rotation, "VEC4", track.keys.len(), false);
                samplers
                    .push(json!({"input": time_acc, "output": rot_acc, "interpolation": "LINEAR"}));
                channels.push(json!({"sampler": samplers.len() - 1, "target": {"node": node, "path": "rotation"}}));
            }
            let scale_acc = push_anim(root, bin, &scale, "VEC3", track.keys.len(), false);
            samplers
                .push(json!({"input": time_acc, "output": scale_acc, "interpolation": "LINEAR"}));
            channels.push(
                json!({"sampler": samplers.len() - 1, "target": {"node": node, "path": "scale"}}),
            );
        }
        animations.push(json!({
            "name": clip.name,
            "samplers": samplers,
            "channels": channels
        }));
    }
    root["animations"] = json!(animations);
    root["buffers"][0]["byteLength"] = json!(bin.len());
    Ok(())
}

fn push_anim(
    root: &mut serde_json::Value,
    bin: &mut Vec<u8>,
    values: &[f32],
    ty: &str,
    count: usize,
    bounds: bool,
) -> usize {
    let offset = bin.len();
    let mut min = f32::MAX;
    let mut max = f32::MIN;
    for value in values {
        min = min.min(*value);
        max = max.max(*value);
        bin.extend_from_slice(&value.to_le_bytes());
    }
    let views = root["bufferViews"].as_array().expect("buffer views").len();
    let accessors = root["accessors"].as_array().expect("accessors").len();
    root["bufferViews"]
        .as_array_mut()
        .expect("buffer views")
        .push(json!({"buffer": 0, "byteOffset": offset, "byteLength": values.len() * 4}));
    let mut accessor = json!({
        "bufferView": views,
        "componentType": 5126,
        "count": count,
        "type": ty
    });
    if bounds {
        accessor["min"] = json!([min]);
        accessor["max"] = json!([max]);
    }
    root["accessors"]
        .as_array_mut()
        .expect("accessors")
        .push(accessor);
    accessors
}

fn push_f32s(out: &mut Vec<u8>, values: &[f32]) {
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

fn pad(buf: &mut Vec<u8>, fill: u8) {
    while buf.len() % 4 != 0 {
        buf.push(fill);
    }
}

/// glTF `asset`. A labelled document adds `extras.method`.
fn asset_json(session: &Session) -> Value {
    let mut asset = json!({"version": "2.0", "generator": "Froxel"});
    if let Some(method) = session.method() {
        asset["extras"] = json!({"method": method.as_str()});
    }
    asset
}

/// The menu export, with the options from the Export panel. A new session
/// writes a palette texture, greedy-merged.
pub fn export_glb(session: &Session, path: &Path) -> Result<u32, String> {
    export_with(session, path, &session.export)
}

/// Nodes, meshes, skins and animations in a `.glb`, plus warnings for an empty
/// mesh, a skin with no joints, or an animation with no channels. This reads
/// the JSON chunk only. It does not load the vertex buffer.
pub fn gltf_structure(bytes: &[u8]) -> Result<Value, String> {
    let root = gltf_json(bytes)?;
    let mut warnings = Vec::new();
    let nodes = root
        .get("nodes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if nodes.is_empty() {
        warnings.push("the glb has no nodes".to_string());
    }
    let mut names = BTreeMap::<String, usize>::new();
    let mut node_report = Vec::new();
    for node in &nodes {
        let name = node
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            warnings.push("a node has no name".to_string());
        } else {
            *names.entry(name.clone()).or_default() += 1;
        }
        let children = node
            .get("children")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        node_report.push(json!({
            "name": name,
            "mesh": node.get("mesh").cloned().unwrap_or(Value::Null),
            "skin": node.get("skin").cloned().unwrap_or(Value::Null),
            "children": children,
        }));
    }
    for (name, count) in &names {
        if *count > 1 {
            warnings.push(format!("node name `{name}` is used {count} times"));
        }
    }
    let meshes = root
        .get("meshes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if meshes.is_empty() {
        warnings.push("the glb has no meshes".to_string());
    }
    let accessors = root.get("accessors").and_then(Value::as_array);
    let mut mesh_report = Vec::new();
    for mesh in &meshes {
        let primitives = mesh
            .get("primitives")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if primitives.is_empty() {
            warnings.push(format!(
                "mesh `{}` has no primitives",
                mesh.get("name").and_then(Value::as_str).unwrap_or("")
            ));
        }
        let mut primitive_report = Vec::new();
        for primitive in &primitives {
            let position = primitive
                .pointer("/attributes/POSITION")
                .and_then(Value::as_u64);
            let Some(position) = position else {
                warnings.push("a primitive has no POSITION".to_string());
                primitive_report.push(json!({"vertices": 0, "indices": 0}));
                continue;
            };
            let vertices = accessor_count(accessors, position);
            if vertices == 0 {
                warnings.push("a primitive has no vertices".to_string());
            }
            let indices = primitive
                .get("indices")
                .and_then(Value::as_u64)
                .map(|index| accessor_count(accessors, index))
                .unwrap_or(0);
            if indices == 0 {
                warnings.push("a primitive has no indices".to_string());
            }
            let bounds = accessors
                .and_then(|items| items.get(position as usize))
                .map(|accessor| json!({"min": accessor.get("min"), "max": accessor.get("max")}))
                .unwrap_or(Value::Null);
            primitive_report
                .push(json!({"vertices": vertices, "indices": indices, "bounds": bounds}));
        }
        mesh_report.push(json!({
            "name": mesh.get("name").cloned().unwrap_or(Value::Null),
            "primitives": primitive_report,
        }));
    }
    let mut skins = Vec::new();
    for skin in root
        .get("skins")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let joints = skin
            .get("joints")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        if joints == 0 {
            warnings.push("a skin has no joints".to_string());
        }
        skins.push(json!({"joints": joints}));
    }
    let mut animations = Vec::new();
    for animation in root
        .get("animations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let channels = animation
            .get("channels")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        if channels == 0 {
            warnings.push(format!(
                "animation `{}` has no channels",
                animation.get("name").and_then(Value::as_str).unwrap_or("")
            ));
        }
        animations.push(json!({
            "name": animation.get("name").cloned().unwrap_or(Value::Null),
            "channels": channels,
        }));
    }
    Ok(json!({
        "generator": root.pointer("/asset/generator").cloned().unwrap_or(Value::Null),
        "method": root.pointer("/asset/extras/method").cloned().unwrap_or(Value::Null),
        "nodes": node_report,
        "meshes": mesh_report,
        "skins": skins,
        "animations": animations,
        "images": root.get("images").and_then(Value::as_array).map(Vec::len).unwrap_or(0),
        "warnings": warnings,
    }))
}

fn accessor_count(accessors: Option<&Vec<Value>>, index: u64) -> u64 {
    accessors
        .and_then(|items| items.get(index as usize))
        .and_then(|accessor| accessor.get("count"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

fn gltf_json(bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() < 20 || &bytes[0..4] != b"glTF" {
        return Err("not a glb".into());
    }
    let json_len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
    if &bytes[16..20] != b"JSON" {
        return Err("a glb JSON chunk is missing".into());
    }
    let end = 20 + json_len;
    if end > bytes.len() {
        return Err("the glb JSON chunk is truncated".into());
    }
    let mut json_end = end;
    while json_end > 20 && matches!(bytes[json_end - 1], 0x00 | b' ') {
        json_end -= 1;
    }
    serde_json::from_slice(&bytes[20..json_end]).map_err(|err| format!("glb JSON: {err}"))
}

/// Write a `.glb` and return the quad count. An empty scene is an error
/// and does not create the file.
pub fn export_with(session: &Session, path: &Path, options: &MeshExport) -> Result<u32, String> {
    let built = build_with(session, options);
    let quads = (built.indices.len() / 6) as u32;
    if quads == 0 {
        return Err("Nothing to export.".into());
    }
    let bytes = encode(session, options)?;
    std::fs::write(path, &bytes)
        .map_err(|err| format!("Could not write {}: {err}", path.display()))?;
    Ok(quads)
}

/// 256×1 sRGB PNG. glTF samples `baseColorTexture` as sRGB, so the swatch
/// bytes are stored unchanged.
fn palette_png(palette: &[[u8; 4]; 256]) -> Result<Vec<u8>, String> {
    let mut pixels = Vec::with_capacity(256 * 4);
    for color in palette {
        pixels.extend_from_slice(color);
    }
    let mut buf = Vec::new();
    let mut encoder = png::Encoder::new(&mut buf, 256, 1);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder
        .write_header()
        .map_err(|err| format!("palette image: {err}"))?;
    writer
        .write_image_data(&pixels)
        .map_err(|err| format!("palette image: {err}"))?;
    writer
        .finish()
        .map_err(|err| format!("palette image: {err}"))?;
    Ok(buf)
}

#[cfg(test)]
#[path = "../../tests/export.rs"]
mod tests;
