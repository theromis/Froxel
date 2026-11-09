//! glTF 2.0 (`.gltf` with embedded or external buffers and images, and
//! binary `.glb`) read into a [`Mesh`].
//!
//! Every triangle primitive of every mesh node of the default scene is
//! taken under its node's world transform. A material gives its base-colour
//! factor and texture (with `KHR_texture_transform`), or the diffuse of
//! `KHR_materials_pbrSpecularGlossiness`; `COLOR_0` multiplies it. Skins and
//! morph targets are ignored: the mesh is read in its bind pose. Draco and
//! meshopt compression are refused.

use std::collections::HashMap;
use std::path::Path;

use glam::{DMat4, DQuat, DVec3};
use serde_json::Value;

use super::{image, Material, Mesh, Texture, Wrap};

const GLB_JSON: u32 = 0x4E4F_534A;
const GLB_BIN: u32 = 0x004E_4942;
const UNSUPPORTED: &[&str] = &[
    "KHR_draco_mesh_compression",
    "EXT_meshopt_compression",
    "KHR_meshopt_compression",
];

pub(super) fn load(path: &Path) -> Result<Mesh, String> {
    let bytes =
        std::fs::read(path).map_err(|err| format!("Could not read {}: {err}", path.display()))?;
    let name = path.display().to_string();
    let (doc, bin) = if bytes.starts_with(b"glTF") {
        glb(&bytes, &name)?
    } else {
        let doc: Value = serde_json::from_slice(&bytes)
            .map_err(|err| format!("{name} is not glTF JSON: {err}"))?;
        (doc, None)
    };
    let dir = path.parent().unwrap_or(Path::new("."));
    parse(&doc, bin, dir)
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn glb(bytes: &[u8], name: &str) -> Result<(Value, Option<Vec<u8>>), String> {
    let bad = || format!("{name} is not a valid .glb");
    if u32_at(bytes, 4) != Some(2) {
        return Err(format!("{name} is not glTF 2.0"));
    }
    let mut at = 12;
    let mut doc = None;
    let mut bin = None;
    while at + 8 <= bytes.len() {
        let len = u32_at(bytes, at).ok_or_else(bad)? as usize;
        let kind = u32_at(bytes, at + 4).ok_or_else(bad)?;
        let body = bytes.get(at + 8..at + 8 + len).ok_or_else(bad)?;
        match kind {
            GLB_JSON if doc.is_none() => {
                doc = Some(
                    serde_json::from_slice(body)
                        .map_err(|err| format!("{name} has bad glTF JSON: {err}"))?,
                )
            }
            GLB_BIN if bin.is_none() => bin = Some(body.to_vec()),
            _ => {}
        }
        at += 8 + len.div_ceil(4) * 4;
    }
    Ok((doc.ok_or_else(bad)?, bin))
}

fn index(value: &Value, key: &str) -> Option<usize> {
    value.get(key).and_then(Value::as_u64).map(|i| i as usize)
}

fn list<'a>(doc: &'a Value, key: &str) -> &'a [Value] {
    doc.get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn numbers<const N: usize>(value: Option<&Value>, fallback: [f64; N]) -> [f64; N] {
    let Some(array) = value.and_then(Value::as_array) else {
        return fallback;
    };
    let mut out = fallback;
    for (slot, item) in out.iter_mut().zip(array) {
        if let Some(v) = item.as_f64() {
            *slot = v;
        }
    }
    out
}

/// The bytes a `uri` names: a base64 `data:` URI or a file beside the glTF.
fn uri_bytes(uri: &str, dir: &Path) -> Result<Vec<u8>, String> {
    if let Some(rest) = uri.strip_prefix("data:") {
        let (head, data) = rest.split_once(',').ok_or("a data URI has no comma")?;
        if !head.ends_with(";base64") {
            return Err("a data URI is not base64".into());
        }
        return image::base64(data);
    }
    let file = dir.join(image::percent_decode(uri));
    std::fs::read(&file).map_err(|err| format!("Could not read {}: {err}", file.display()))
}

struct Doc<'a> {
    doc: &'a Value,
    buffers: Vec<Vec<u8>>,
}

impl Doc<'_> {
    /// The bytes of a buffer view.
    fn view(&self, view: usize) -> Result<(&[u8], Option<usize>), String> {
        let view = list(self.doc, "bufferViews")
            .get(view)
            .ok_or_else(|| format!("bufferView {view} does not exist"))?;
        let buffer = index(view, "buffer").unwrap_or(0);
        let data = self
            .buffers
            .get(buffer)
            .ok_or_else(|| format!("buffer {buffer} does not exist"))?;
        let offset = index(view, "byteOffset").unwrap_or(0);
        let length = index(view, "byteLength").unwrap_or(0);
        let bytes = data
            .get(offset..offset + length)
            .ok_or_else(|| format!("a bufferView runs past buffer {buffer}"))?;
        Ok((bytes, index(view, "byteStride")))
    }

    /// An accessor as `count` rows of `width` numbers, normalized integers
    /// scaled to 0..=1 (or -1..=1).
    fn accessor(&self, at: usize) -> Result<(Vec<f64>, usize), String> {
        let accessor = list(self.doc, "accessors")
            .get(at)
            .ok_or_else(|| format!("accessor {at} does not exist"))?;
        let count = index(accessor, "count").unwrap_or(0);
        let width = match accessor.get("type").and_then(Value::as_str) {
            Some("SCALAR") => 1,
            Some("VEC2") => 2,
            Some("VEC3") => 3,
            Some("VEC4") => 4,
            Some("MAT2") => 4,
            Some("MAT3") => 9,
            Some("MAT4") => 16,
            other => return Err(format!("accessor {at} has type {other:?}")),
        };
        let kind = index(accessor, "componentType").unwrap_or(0);
        let normalized = accessor
            .get("normalized")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut out = vec![0.0; count * width];
        if let Some(view) = index(accessor, "bufferView") {
            let (bytes, stride) = self.view(view)?;
            let offset = index(accessor, "byteOffset").unwrap_or(0);
            read_rows(
                bytes, offset, stride, kind, normalized, width, count, &mut out,
            )
            .map_err(|err| format!("accessor {at}: {err}"))?;
        }
        if let Some(sparse) = accessor.get("sparse") {
            let n = index(sparse, "count").unwrap_or(0);
            let indices = sparse.get("indices").ok_or("sparse has no indices")?;
            let values = sparse.get("values").ok_or("sparse has no values")?;
            let mut rows = vec![0.0; n];
            let (bytes, _) = self.view(index(indices, "bufferView").unwrap_or(0))?;
            read_rows(
                bytes,
                index(indices, "byteOffset").unwrap_or(0),
                None,
                index(indices, "componentType").unwrap_or(5125),
                false,
                1,
                n,
                &mut rows,
            )?;
            let mut replaced = vec![0.0; n * width];
            let (bytes, _) = self.view(index(values, "bufferView").unwrap_or(0))?;
            read_rows(
                bytes,
                index(values, "byteOffset").unwrap_or(0),
                None,
                kind,
                normalized,
                width,
                n,
                &mut replaced,
            )?;
            for (k, row) in rows.iter().enumerate() {
                let row = *row as usize;
                if row < count {
                    out[row * width..row * width + width]
                        .copy_from_slice(&replaced[k * width..k * width + width]);
                }
            }
        }
        Ok((out, width))
    }
}

#[allow(clippy::too_many_arguments)]
fn read_rows(
    bytes: &[u8],
    offset: usize,
    stride: Option<usize>,
    kind: usize,
    normalized: bool,
    width: usize,
    count: usize,
    out: &mut [f64],
) -> Result<(), String> {
    let size = match kind {
        5120 | 5121 => 1,
        5122 | 5123 => 2,
        5125 | 5126 => 4,
        other => return Err(format!("component type {other} is not supported")),
    };
    let stride = stride.unwrap_or(size * width);
    for row in 0..count {
        for c in 0..width {
            let at = offset + row * stride + c * size;
            let b = bytes
                .get(at..at + size)
                .ok_or("an accessor runs past its bufferView")?;
            let value = match kind {
                5120 => {
                    let v = f64::from(b[0] as i8);
                    if normalized {
                        (v / 127.0).max(-1.0)
                    } else {
                        v
                    }
                }
                5121 => {
                    let v = f64::from(b[0]);
                    if normalized {
                        v / 255.0
                    } else {
                        v
                    }
                }
                5122 => {
                    let v = f64::from(i16::from_le_bytes([b[0], b[1]]));
                    if normalized {
                        (v / 32767.0).max(-1.0)
                    } else {
                        v
                    }
                }
                5123 => {
                    let v = f64::from(u16::from_le_bytes([b[0], b[1]]));
                    if normalized {
                        v / 65535.0
                    } else {
                        v
                    }
                }
                5125 => f64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
                _ => f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            };
            out[row * width + c] = value;
        }
    }
    Ok(())
}

fn parse(doc: &Value, bin: Option<Vec<u8>>, dir: &Path) -> Result<Mesh, String> {
    for required in list(doc, "extensionsRequired") {
        if let Some(name) = required.as_str() {
            if UNSUPPORTED.contains(&name) {
                return Err(format!(
                    "the file needs {name}; export it again without mesh compression"
                ));
            }
        }
    }
    let mut buffers = Vec::new();
    let mut bin = bin;
    for (at, buffer) in list(doc, "buffers").iter().enumerate() {
        let bytes = match buffer.get("uri").and_then(Value::as_str) {
            Some(uri) => uri_bytes(uri, dir).map_err(|err| format!("buffer {at}: {err}"))?,
            None => bin
                .take()
                .ok_or_else(|| format!("buffer {at} has no uri and there is no .glb BIN chunk"))?,
        };
        buffers.push(bytes);
    }
    let doc = Doc { doc, buffers };
    let mut mesh = Mesh::default();
    let mut textures: HashMap<usize, Option<usize>> = HashMap::new();
    // One material per glTF material, then the default one.
    let materials = list(doc.doc, "materials");
    let mut tex_coords = Vec::with_capacity(materials.len() + 1);
    for material in materials {
        let pbr = material.get("pbrMetallicRoughness");
        let gloss = material
            .get("extensions")
            .and_then(|e| e.get("KHR_materials_pbrSpecularGlossiness"));
        let (color, info) = match (pbr, gloss) {
            (_, Some(gloss)) => (
                numbers(gloss.get("diffuseFactor"), [1.0; 4]),
                gloss.get("diffuseTexture"),
            ),
            (Some(pbr), None) => (
                numbers(pbr.get("baseColorFactor"), [1.0; 4]),
                pbr.get("baseColorTexture"),
            ),
            (None, None) => ([1.0; 4], None),
        };
        let mut out = Material {
            color,
            texture: None,
            transform: None,
        };
        let mut tex_coord = 0;
        if let Some(info) = info {
            tex_coord = index(info, "texCoord").unwrap_or(0);
            if let Some(texture) = index(info, "index") {
                let slot = match textures.get(&texture) {
                    Some(slot) => *slot,
                    None => {
                        let slot = match texture_of(&doc, texture, dir) {
                            Ok(found) => {
                                mesh.textures.push(found);
                                Some(mesh.textures.len() - 1)
                            }
                            Err(err) => {
                                mesh.warnings
                                    .push(format!("texture {texture} is not used: {err}"));
                                None
                            }
                        };
                        textures.insert(texture, slot);
                        slot
                    }
                };
                out.texture = slot;
            }
            if let Some(transform) = info
                .get("extensions")
                .and_then(|e| e.get("KHR_texture_transform"))
            {
                let [ou, ov] = numbers(transform.get("offset"), [0.0, 0.0]);
                let [su, sv] = numbers(transform.get("scale"), [1.0, 1.0]);
                let r = transform
                    .get("rotation")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                let (sin, cos) = r.sin_cos();
                out.transform = Some([cos * su, sin * sv, -sin * su, cos * sv, ou, ov]);
                if let Some(set) = index(transform, "texCoord") {
                    tex_coord = set;
                }
            }
        }
        mesh.materials.push(out);
        tex_coords.push(tex_coord);
    }
    let fallback = mesh.materials.len() as u32;
    mesh.materials.push(Material::default());
    tex_coords.push(0);

    // The default scene's roots, or every node no other node holds.
    let nodes = list(doc.doc, "nodes");
    let scenes = list(doc.doc, "scenes");
    let roots: Vec<usize> = match scenes.get(index(doc.doc, "scene").unwrap_or(0)) {
        Some(scene) => list(scene, "nodes")
            .iter()
            .filter_map(Value::as_u64)
            .map(|i| i as usize)
            .collect(),
        None => {
            let mut held = vec![false; nodes.len()];
            for node in nodes {
                for child in list(node, "children").iter().filter_map(Value::as_u64) {
                    if let Some(flag) = held.get_mut(child as usize) {
                        *flag = true;
                    }
                }
            }
            (0..nodes.len()).filter(|&i| !held[i]).collect()
        }
    };
    let mut stack: Vec<(usize, DMat4, usize)> = roots
        .into_iter()
        .rev()
        .map(|i| (i, DMat4::IDENTITY, 0))
        .collect();
    let mut visits = 0;
    while let Some((at, parent, depth)) = stack.pop() {
        visits += 1;
        if depth > 64 || visits > 1_000_000 {
            return Err("the node hierarchy is too deep or has a cycle".into());
        }
        let node = nodes
            .get(at)
            .ok_or_else(|| format!("node {at} does not exist"))?;
        let world = parent * local(node);
        if let Some(at_mesh) = index(node, "mesh") {
            let gltf_mesh = list(doc.doc, "meshes")
                .get(at_mesh)
                .ok_or_else(|| format!("mesh {at_mesh} does not exist"))?;
            for primitive in list(gltf_mesh, "primitives") {
                let material = index(primitive, "material")
                    .filter(|&m| m < materials.len())
                    .map_or(fallback, |m| m as u32);
                add_primitive(
                    &doc,
                    &mut mesh,
                    primitive,
                    &world,
                    material,
                    tex_coords[material as usize],
                )?;
            }
        }
        for child in list(node, "children")
            .iter()
            .rev()
            .filter_map(Value::as_u64)
        {
            stack.push((child as usize, world, depth + 1));
        }
    }
    Ok(mesh)
}

fn local(node: &Value) -> DMat4 {
    if let Some(matrix) = node.get("matrix").and_then(Value::as_array) {
        if matrix.len() == 16 {
            let cols: [f64; 16] = numbers(node.get("matrix"), [0.0; 16]);
            return DMat4::from_cols_array(&cols);
        }
    }
    let [tx, ty, tz] = numbers(node.get("translation"), [0.0; 3]);
    let [qx, qy, qz, qw] = numbers(node.get("rotation"), [0.0, 0.0, 0.0, 1.0]);
    let [sx, sy, sz] = numbers(node.get("scale"), [1.0; 3]);
    let rotation = DQuat::from_xyzw(qx, qy, qz, qw);
    let rotation = if rotation.length_squared() > 0.0 {
        rotation.normalize()
    } else {
        DQuat::IDENTITY
    };
    DMat4::from_scale_rotation_translation(DVec3::new(sx, sy, sz), rotation, DVec3::new(tx, ty, tz))
}

fn add_primitive(
    doc: &Doc,
    mesh: &mut Mesh,
    primitive: &Value,
    world: &DMat4,
    material: u32,
    tex_coord: usize,
) -> Result<(), String> {
    let mode = index(primitive, "mode").unwrap_or(4);
    if !(4..=6).contains(&mode) {
        mesh.warnings.push(format!(
            "a primitive of mode {mode} (points or lines) is skipped"
        ));
        return Ok(());
    }
    let attributes = primitive
        .get("attributes")
        .ok_or("a primitive has no attributes")?;
    let Some(position) = index(attributes, "POSITION") else {
        return Ok(());
    };
    let (positions, width) = doc.accessor(position)?;
    if width != 3 {
        return Err("POSITION is not VEC3".into());
    }
    let count = positions.len() / 3;
    let uvs = match index(attributes, &format!("TEXCOORD_{tex_coord}")) {
        Some(at) => Some(doc.accessor(at)?),
        None => None,
    };
    let colors = match index(attributes, "COLOR_0") {
        Some(at) => Some(doc.accessor(at)?),
        None => None,
    };
    let base = mesh.positions.len() as u32;
    for v in 0..count {
        let p = world.transform_point3(DVec3::new(
            positions[v * 3],
            positions[v * 3 + 1],
            positions[v * 3 + 2],
        ));
        let uv = match &uvs {
            Some((values, w)) if *w >= 2 && values.len() >= (v + 1) * w => {
                [values[v * w], values[v * w + 1]]
            }
            _ => [0.0, 0.0],
        };
        let color = match &colors {
            Some((values, w)) if (3..=4).contains(w) && values.len() >= (v + 1) * w => [
                values[v * w],
                values[v * w + 1],
                values[v * w + 2],
                if *w == 4 { values[v * w + 3] } else { 1.0 },
            ],
            _ => [1.0; 4],
        };
        mesh.push_vertex(p.to_array(), uv, color);
    }
    let indices: Vec<u32> = match index(primitive, "indices") {
        Some(at) => {
            let (values, _) = doc.accessor(at)?;
            values.into_iter().map(|i| i as u32).collect()
        }
        None => (0..count as u32).collect(),
    };
    if let Some(&bad) = indices.iter().find(|&&i| i as usize >= count) {
        return Err(format!(
            "index {bad} is past the {count} vertices of a primitive"
        ));
    }
    let before = mesh.triangles.len();
    let corner = |i: u32| base + i;
    match mode {
        4 => {
            for tri in indices.chunks_exact(3) {
                mesh.push_triangle([corner(tri[0]), corner(tri[1]), corner(tri[2])], material)?;
            }
        }
        5 => {
            for i in 0..indices.len().saturating_sub(2) {
                let (a, b) = if i % 2 == 0 { (i, i + 1) } else { (i + 1, i) };
                mesh.push_triangle(
                    [
                        corner(indices[a]),
                        corner(indices[b]),
                        corner(indices[i + 2]),
                    ],
                    material,
                )?;
            }
        }
        _ => {
            for i in 1..indices.len().saturating_sub(1) {
                mesh.push_triangle(
                    [
                        corner(indices[0]),
                        corner(indices[i]),
                        corner(indices[i + 1]),
                    ],
                    material,
                )?;
            }
        }
    }
    if mesh.triangles.len() > before {
        mesh.primitives += 1;
    }
    Ok(())
}

fn texture_of(doc: &Doc, texture: usize, dir: &Path) -> Result<Texture, String> {
    let entry = list(doc.doc, "textures")
        .get(texture)
        .ok_or("it does not exist")?;
    let webp = entry
        .get("extensions")
        .and_then(|e| e.get("EXT_texture_webp"))
        .and_then(|e| index(e, "source"));
    let basisu = entry
        .get("extensions")
        .and_then(|e| e.get("KHR_texture_basisu"))
        .is_some();
    let source = match (webp, index(entry, "source")) {
        (Some(source), _) | (None, Some(source)) => source,
        (None, None) if basisu => {
            return Err("its only source is KHR_texture_basisu (KTX2/Basis), which import does not read; export the texture as PNG, JPEG, or WebP".into())
        }
        (None, None) => return Err("it has no image source".into()),
    };
    let image = list(doc.doc, "images")
        .get(source)
        .ok_or_else(|| format!("image {source} does not exist"))?;
    let bytes = match (
        image.get("uri").and_then(Value::as_str),
        index(image, "bufferView"),
    ) {
        (Some(uri), _) => uri_bytes(uri, dir)?,
        (None, Some(view)) => doc.view(view)?.0.to_vec(),
        (None, None) => return Err(format!("image {source} has no uri or bufferView")),
    };
    let mut decoded = image::decode(&bytes, &format!("image {source}"))?;
    if let Some(sampler) = index(entry, "sampler").and_then(|s| list(doc.doc, "samplers").get(s)) {
        let mode = |key: &str| match index(sampler, key) {
            Some(33071) => Wrap::Clamp,
            Some(33648) => Wrap::Mirror,
            _ => Wrap::Repeat,
        };
        decoded.wrap = [mode("wrapS"), mode("wrapT")];
    }
    Ok(decoded)
}
