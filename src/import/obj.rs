//! Wavefront `.obj` with its `.mtl` read into a [`Mesh`].
//!
//! Faces of any size are fanned into triangles; negative indices count back
//! from the end. `v x y z r g b` vertex colours, `Kd`, `d`, and `map_Kd` are
//! sRGB, as the tools that write them show them. Texture coordinates have
//! their origin at the bottom; they are flipped to glTF's top-left origin.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::{image, to_linear, Material, Mesh};

fn linear(value: f64) -> f64 {
    to_linear((value.clamp(0.0, 1.0) * 255.0).round() as u8)
}

pub(super) fn load(path: &Path) -> Result<Mesh, String> {
    let bytes =
        std::fs::read(path).map_err(|err| format!("Could not read {}: {err}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut mesh = Mesh::default();
    let mut positions: Vec<[f64; 3]> = Vec::new();
    let mut vertex_colors: Vec<Option<[f64; 3]>> = Vec::new();
    let mut uvs: Vec<[f64; 2]> = Vec::new();
    let mut named: HashMap<String, u32> = HashMap::new();
    let mut textures: HashMap<PathBuf, Option<usize>> = HashMap::new();
    let fallback = 0u32;
    mesh.materials.push(Material::default());
    let mut current = fallback;
    let mut corners: HashMap<(usize, Option<usize>), u32> = HashMap::new();
    let mut groups: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut unknown: Vec<String> = Vec::new();
    for (number, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        let mut parts = line.split_whitespace();
        let Some(keyword) = parts.next() else {
            continue;
        };
        let rest = line[keyword.len()..].trim();
        let bad = |what: &str| format!("{} line {}: {what}", path.display(), number + 1);
        let floats = |text: &str| -> Result<Vec<f64>, String> {
            text.split_whitespace()
                .map(|n| {
                    n.parse::<f64>()
                        .map_err(|_| bad(&format!("`{n}` is not a number")))
                })
                .collect()
        };
        match keyword {
            "v" => {
                let n = floats(rest)?;
                if n.len() < 3 {
                    return Err(bad("v needs x y z"));
                }
                positions.push([n[0], n[1], n[2]]);
                vertex_colors.push((n.len() >= 6).then(|| [n[3], n[4], n[5]]));
            }
            "vt" => {
                let n = floats(rest)?;
                if n.is_empty() {
                    return Err(bad("vt needs u"));
                }
                uvs.push([n[0], 1.0 - n.get(1).copied().unwrap_or(0.0)]);
            }
            "f" => {
                let mut face = Vec::new();
                for corner in parts {
                    let mut fields = corner.split('/');
                    let resolve =
                        |field: Option<&str>, len: usize| -> Result<Option<usize>, String> {
                            match field.filter(|f| !f.is_empty()) {
                                None => Ok(None),
                                Some(f) => {
                                    let i: i64 = f
                                        .parse()
                                        .map_err(|_| bad(&format!("`{f}` is not an index")))?;
                                    let at = if i < 0 { len as i64 + i } else { i - 1 };
                                    if at < 0 || at as usize >= len {
                                        return Err(bad(&format!("index {i} is out of range")));
                                    }
                                    Ok(Some(at as usize))
                                }
                            }
                        };
                    let v = resolve(fields.next(), positions.len())?
                        .ok_or_else(|| bad("a face corner has no vertex"))?;
                    let t = resolve(fields.next(), uvs.len())?;
                    let key = (v, t);
                    let vertex = match corners.get(&key) {
                        Some(&vertex) => vertex,
                        None => {
                            let color = vertex_colors[v]
                                .map(|c| [linear(c[0]), linear(c[1]), linear(c[2]), 1.0])
                                .unwrap_or([1.0; 4]);
                            let uv = t.map(|t| uvs[t]).unwrap_or([0.0, 0.0]);
                            let vertex = mesh.push_vertex(positions[v], uv, color);
                            corners.insert(key, vertex);
                            vertex
                        }
                    };
                    face.push(vertex);
                }
                for i in 1..face.len().saturating_sub(1) {
                    mesh.push_triangle([face[0], face[i], face[i + 1]], current)?;
                }
                if face.len() >= 3 {
                    groups.insert(current);
                }
            }
            "mtllib" => {
                let file = dir.join(rest.replace('\\', "/"));
                match std::fs::read(&file) {
                    Ok(body) => {
                        let mtl_dir = file.parent().unwrap_or(dir).to_path_buf();
                        materials(
                            &String::from_utf8_lossy(&body),
                            &mtl_dir,
                            &mut mesh,
                            &mut named,
                            &mut textures,
                        );
                    }
                    Err(err) => mesh
                        .warnings
                        .push(format!("Could not read {}: {err}", file.display())),
                }
            }
            "usemtl" => {
                current = match named.get(rest) {
                    Some(&index) => index,
                    None => {
                        if !unknown.iter().any(|name| name == rest) {
                            unknown.push(rest.to_string());
                        }
                        fallback
                    }
                };
            }
            _ => {}
        }
    }
    for name in unknown {
        mesh.warnings
            .push(format!("material `{name}` is not in a .mtl; it is white"));
    }
    mesh.primitives = groups.len();
    Ok(mesh)
}

fn materials(
    text: &str,
    dir: &Path,
    mesh: &mut Mesh,
    named: &mut HashMap<String, u32>,
    textures: &mut HashMap<PathBuf, Option<usize>>,
) {
    let mut current: Option<usize> = None;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        let Some(keyword) = line.split_whitespace().next() else {
            continue;
        };
        let rest = line[keyword.len()..].trim();
        if keyword == "newmtl" {
            mesh.materials.push(Material::default());
            let index = mesh.materials.len() - 1;
            named.insert(rest.to_string(), index as u32);
            current = Some(index);
            continue;
        }
        let Some(index) = current else {
            continue;
        };
        let numbers: Vec<f64> = rest
            .split_whitespace()
            .filter_map(|n| n.parse().ok())
            .collect();
        let material = &mut mesh.materials[index];
        match keyword {
            "Kd" if numbers.len() >= 3 && material.texture.is_none() => {
                material.color[0] = linear(numbers[0]);
                material.color[1] = linear(numbers[1]);
                material.color[2] = linear(numbers[2]);
            }
            "d" if !numbers.is_empty() => material.color[3] = numbers[0].clamp(0.0, 1.0),
            "Tr" if !numbers.is_empty() => material.color[3] = 1.0 - numbers[0].clamp(0.0, 1.0),
            "map_Kd" => {
                let file = dir.join(texture_name(rest).replace('\\', "/"));
                let slot = match textures.get(&file) {
                    Some(slot) => *slot,
                    None => {
                        let decoded = std::fs::read(&file)
                            .map_err(|err| format!("Could not read {}: {err}", file.display()))
                            .and_then(|bytes| image::decode(&bytes, &file.display().to_string()));
                        let slot = match decoded {
                            Ok(texture) => {
                                mesh.textures.push(texture);
                                Some(mesh.textures.len() - 1)
                            }
                            Err(err) => {
                                mesh.warnings.push(err);
                                None
                            }
                        };
                        textures.insert(file, slot);
                        slot
                    }
                };
                mesh.materials[index].texture = slot;
                // A texture in MTL replaces Kd rather than multiplying it
                // in most exporters' previews.
                if slot.is_some() {
                    let alpha = mesh.materials[index].color[3];
                    mesh.materials[index].color = [1.0, 1.0, 1.0, alpha];
                }
            }
            _ => {}
        }
    }
}

/// The file name of a `map_Kd` line without its `-o 0 0`-style options.
fn texture_name(rest: &str) -> String {
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    let mut i = 0;
    while i < tokens.len() && tokens[i].starts_with('-') {
        let takes = match tokens[i] {
            "-o" | "-s" | "-t" => 3,
            "-mm" => 2,
            _ => 1,
        };
        i += 1;
        let mut taken = 0;
        while taken < takes
            && i < tokens.len() - 1
            && (tokens[i].parse::<f64>().is_ok() || tokens[i] == "on" || tokens[i] == "off")
        {
            i += 1;
            taken += 1;
        }
    }
    tokens[i.min(tokens.len().saturating_sub(1))..].join(" ")
}
