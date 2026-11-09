//! MCP `import_mesh`: a `.glb`, `.gltf`, or `.obj` voxelized into the active
//! object or a new one, its colours quantized into the palette.
//!
//! [`import_plan`] turns a loaded mesh and the tool's options into cell
//! writes and palette slots. The MCP handler applies them as one undo step;
//! Rhai `voxelize` runs the same plan on a mesh given inline, since scripts
//! read no files.

use std::collections::HashMap;
use std::path::Path;

use serde_json::{json, Map, Value};

use super::{add_mode, opt_bool, opt_f64, opt_str, req_str, slot_list, FillMode};
use crate::import::{Anchor, Colors, Fill, Fit, Mesh, Up, Voxelize};
use crate::state::{Cell, PaletteChange, Session};

/// The default `size`: cells along the longest axis.
pub(crate) const DEFAULT_IMPORT_SIZE: f64 = 64.0;
pub(crate) const DEFAULT_MAX_NEW_SLOTS: usize = 32;
pub(crate) const DEFAULT_TOLERANCE: f64 = 12.0;

pub(crate) struct ImportPlan {
    pub(crate) writes: Vec<(i32, i32, i32, i32)>,
    /// Palette slots to write, with their sRGB colours.
    pub(crate) new_slots: Vec<(u8, [u8; 3])>,
    pub(crate) reply: Map<String, Value>,
}

fn number3(params: &Value, key: &str) -> Result<Option<[f64; 3]>, String> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(list)) if list.len() == 3 => {
            let mut out = [0.0; 3];
            for (slot, item) in out.iter_mut().zip(list) {
                *slot = item
                    .as_f64()
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| format!("{key} must be [x, y, z] numbers"))?;
            }
            Ok(Some(out))
        }
        Some(_) => Err(format!("{key} must be [x, y, z]")),
    }
}

/// The placement and fill options shared by MCP `import_mesh` and Rhai
/// `voxelize`. `up` is the up axis when `upAxis` is not given.
pub(crate) fn voxelize_options(params: &Value, up: Up) -> Result<Voxelize, String> {
    let size = opt_f64(params, "size")?;
    let scale = opt_f64(params, "scale")?;
    let voxel_size = opt_f64(params, "voxelSize")?;
    let fit = match (size, scale, voxel_size) {
        (Some(size), None, None) => Fit::Cells(size.round()),
        (None, Some(scale), None) => Fit::Scale(scale),
        (None, None, Some(metres)) => {
            if !(metres > 0.0 && metres.is_finite()) {
                return Err("voxelSize must be a positive number of metres".into());
            }
            Fit::Scale(1.0 / metres)
        }
        (None, None, None) => Fit::Cells(DEFAULT_IMPORT_SIZE),
        _ => return Err("pass one of size, scale, and voxelSize".into()),
    };
    let up = match opt_str(params, "upAxis")?.as_deref() {
        None => up,
        Some("y") => Up::Y,
        Some("z") => Up::Z,
        Some(other) => return Err(format!("upAxis must be y or z, not `{other}`")),
    };
    let anchor = match opt_str(params, "anchor")?.as_deref() {
        None | Some("min") => Anchor::Min,
        Some("center") => Anchor::Center,
        Some("bottom") => Anchor::Bottom,
        Some(other) => {
            return Err(format!(
                "anchor must be min, center, or bottom, not `{other}`"
            ))
        }
    };
    let fill = match opt_str(params, "fill")?.as_deref() {
        None | Some("solid") => Fill::Solid,
        Some("parity") => Fill::Parity,
        Some("shell") => Fill::Shell,
        Some(other) => {
            return Err(format!(
                "fill must be solid, parity, or shell, not `{other}`"
            ))
        }
    };
    let colors = match opt_str(params, "colors")?.as_deref() {
        None | Some("auto") => Colors::Auto,
        Some("vertex") => Colors::Vertex,
        Some(other) => return Err(format!("colors must be auto or vertex, not `{other}`")),
    };
    let colors = if params.get("palette").is_some() {
        Colors::None
    } else {
        colors
    };
    Ok(Voxelize {
        fit,
        up,
        rotation: number3(params, "rotation")?.unwrap_or([0.0; 3]),
        anchor,
        position: number3(params, "position")?.unwrap_or([0.0; 3]),
        fill,
        colors,
    })
}

/// The writes and palette slots of `mesh` voxelized against `cells`.
/// `used` marks the slots some cell of the document holds; the others may be
/// overwritten with new colours, from 255 down.
pub(crate) fn import_plan(
    mesh: &Mesh,
    params: &Value,
    up: Up,
    cells: &HashMap<Cell, u8>,
    palette: &[[u8; 4]; 256],
    used: &[bool; 256],
    files: bool,
) -> Result<ImportPlan, String> {
    let mut options = voxelize_options(params, up)?;
    let oriented = match params.get("orient") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) if text == "upright" => {
            let mut reply = Map::new();
            reply.insert(
                "upright".into(),
                upright_mesh(mesh, &mut options, DEFAULT_MAX_TILT)?,
            );
            Some(reply)
        }
        Some(orient) if orient.is_object() => {
            let stand = match orient.get("upright") {
                None | Some(Value::Null) => false,
                Some(found) => found.as_bool().ok_or("orient upright must be true or false")?,
            };
            let max_tilt = opt_f64(orient, "maxTilt")?.unwrap_or(DEFAULT_MAX_TILT);
            let first = if stand {
                Some(upright_mesh(mesh, &mut options, max_tilt)?)
            } else {
                None
            };
            let has_views = orient.get("views").is_some_and(|v| !v.is_null())
                || orient.get("path").is_some_and(|v| !v.is_null());
            let mut reply = if has_views {
                let mut search = orient.clone();
                if let Some(map) = search.as_object_mut() {
                    map.remove("upright");
                    map.remove("maxTilt");
                }
                if stand && search.get("turns").is_none_or(Value::is_null) {
                    // Standing up already chose up: search the yaw only.
                    search["turns"] = json!("upright");
                }
                orient_mesh(mesh, &mut options, &search, files)
                    .map_err(|err| format!("orient: {err}"))?
            } else if stand {
                Map::new()
            } else {
                return Err("orient needs views or a sheet path, or upright: true".into());
            };
            if let Some(first) = first {
                reply.insert("upright".into(), first);
            }
            Some(reply)
        }
        Some(_) => {
            return Err(
                "orient must be \"upright\" or {upright?, views | path, turns?, yawStep?, yawRange?, resolution?}"
                    .into(),
            )
        }
    };
    let mode = add_mode(params)?;
    let slots = slot_list(params, "slots")?;
    let one = match params.get("palette") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let slot = value
                .as_i64()
                .filter(|s| (1..=255).contains(s))
                .ok_or("palette must be a palette index 1..=255")?;
            if !slots.is_empty() {
                return Err("pass palette or slots, not both".into());
            }
            Some(slot as u8)
        }
    };
    let max_new = match opt_f64(params, "maxNewSlots")? {
        None => DEFAULT_MAX_NEW_SLOTS,
        Some(n) if (0.0..=255.0).contains(&n) && n.fract() == 0.0 => n as usize,
        Some(_) => return Err("maxNewSlots must be a whole number 0..=255".into()),
    };
    let tolerance = opt_f64(params, "tolerance")?.unwrap_or(DEFAULT_TOLERANCE);
    if !(0.0..=442.0).contains(&tolerance) {
        return Err("tolerance must be 0..=442 (sRGB distance)".into());
    }
    let voxels = crate::import::voxelize(mesh, &options)?;
    let (map, new_slots) = match one {
        Some(_) => (HashMap::new(), Vec::new()),
        None => {
            let mut counts: HashMap<[u8; 3], usize> = HashMap::new();
            for (_, rgb) in &voxels.cells {
                *counts.entry(*rgb).or_default() += 1;
            }
            let mut colors: Vec<([u8; 3], usize)> = counts.into_iter().collect();
            colors.sort_unstable();
            let quantized = crate::import::quantize(
                &colors,
                &crate::import::PaletteFit {
                    palette,
                    free: (1..=255u8).rev().filter(|s| !used[*s as usize]).collect(),
                    slots: (!slots.is_empty()).then_some(slots),
                    max_new,
                    tolerance,
                },
            )?;
            (quantized.map, quantized.new_slots)
        }
    };
    let slot_of = |rgb: &[u8; 3]| one.unwrap_or_else(|| map[rgb]);
    let mut written: Vec<u8> = Vec::new();
    let writes: Vec<(i32, i32, i32, i32)> = voxels
        .cells
        .iter()
        .filter(|(cell, _)| match mode {
            FillMode::Intersect => unreachable!("add_mode refuses intersect"),
            FillMode::Replace => true,
            FillMode::Empty => !cells.contains_key(cell),
            FillMode::Paint => cells.contains_key(cell),
        })
        .map(|((x, y, z), rgb)| {
            let slot = slot_of(rgb);
            written.push(slot);
            (*x, *y, *z, i32::from(slot))
        })
        .collect();
    written.sort_unstable();
    written.dedup();
    let size = [
        voxels.max.0 - voxels.min.0 + 1,
        voxels.max.1 - voxels.min.1 + 1,
        voxels.max.2 - voxels.min.2 + 1,
    ];
    let mut reply = Map::new();
    reply.insert("cells".into(), json!(voxels.cells.len()));
    reply.insert("shell".into(), json!(voxels.shell));
    reply.insert("interior".into(), json!(voxels.cells.len() - voxels.shell));
    reply.insert("openEdges".into(), json!(voxels.open_edges));
    reply.insert("written".into(), json!(writes.len()));
    reply.insert("slots".into(), json!(written));
    reply.insert(
        "newSlots".into(),
        json!(new_slots
            .iter()
            .filter(|(slot, _)| written.contains(slot))
            .map(|(slot, rgb)| json!({"index": slot, "color": rgb}))
            .collect::<Vec<_>>()),
    );
    reply.insert(
        "bounds".into(),
        json!({
            "min": [voxels.min.0, voxels.min.1, voxels.min.2],
            "max": [voxels.max.0, voxels.max.1, voxels.max.2],
            "size": size,
        }),
    );
    reply.insert("scale".into(), json!(round(voxels.scale, 6)));
    reply.insert("unitsPerCell".into(), json!(round(1.0 / voxels.scale, 6)));
    reply.insert("triangles".into(), json!(mesh.triangles.len()));
    reply.insert("primitives".into(), json!(mesh.primitives));
    reply.insert("textures".into(), json!(mesh.textures.len()));
    if !mesh.warnings.is_empty() {
        reply.insert("warnings".into(), json!(mesh.warnings));
    }
    if let Some(oriented) = oriented {
        reply.insert("orientation".into(), Value::Object(oriented));
    }
    let (writes, new_slots) = match params.get("clean") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => (writes, new_slots),
        Some(clean) => {
            let (writes, new_slots, report) = clean_import(clean, writes, new_slots, palette, used)
                .map_err(|err| format!("clean: {err}"))?;
            let mut written: Vec<u8> = writes.iter().map(|w| w.3 as u8).collect();
            written.sort_unstable();
            written.dedup();
            reply.insert("slots".into(), json!(written));
            reply.insert(
                "newSlots".into(),
                json!(new_slots
                    .iter()
                    .filter(|(slot, _)| written.contains(slot))
                    .map(|(slot, rgb)| json!({"index": slot, "color": rgb}))
                    .collect::<Vec<_>>()),
            );
            reply.insert("clean".into(), report);
            (writes, new_slots)
        }
    };
    // Only slots a cell lands on are written to the palette.
    let mut landed: Vec<u8> = writes.iter().map(|w| w.3 as u8).collect();
    landed.sort_unstable();
    landed.dedup();
    let new_slots = new_slots
        .into_iter()
        .filter(|(slot, _)| landed.contains(slot))
        .collect();
    Ok(ImportPlan {
        writes,
        new_slots,
        reply,
    })
}

/// The turn of `orient` (`orient_to_views`' search) on a coarse shell of
/// the mesh, folded into `options.rotation`, so the mesh is voxelized once
/// already turned. Returns the reply's `orientation`.
fn orient_mesh(
    mesh: &Mesh,
    options: &mut Voxelize,
    orient: &Value,
    files: bool,
) -> Result<Map<String, Value>, String> {
    let spec = super::silhouette::orient_spec(orient, files)?;
    let mut coarse = options.clone();
    coarse.fit = Fit::Cells(f64::from(spec.resolution.clamp(16, 128)));
    coarse.fill = Fill::Shell;
    coarse.colors = Colors::None;
    let voxels = crate::import::voxelize(mesh, &coarse)?;
    let cells: HashMap<Cell, u8> = voxels.cells.iter().map(|(c, _)| (*c, 1)).collect();
    let found = crate::tools::orient(&cells, &spec)?;
    let before = crate::tools::from_euler_xyz(options.rotation);
    let total = crate::tools::orient::mul(&found.best.turn, &before);
    options.rotation = crate::tools::euler_xyz(&total);
    let mut reply = super::silhouette::orient_reply(&spec, &found);
    reply.insert("rotation".into(), {
        let mut rotation = reply["rotation"].clone();
        rotation["applied"] = json!(options.rotation);
        rotation
    });
    Ok(reply)
}

/// `import_mesh`'s `clean`: MCP `reduce_palette` (`true` is `{colors: 8,
/// despeckle: 1}`) over the cells the import writes, before anything is
/// written. The import's new slots are its own, so clusters take them over
/// in place; colours it needs beyond them take free slots.
fn clean_import(
    clean: &Value,
    writes: Vec<(i32, i32, i32, i32)>,
    mut new_slots: Vec<(u8, [u8; 3])>,
    palette: &[[u8; 4]; 256],
    used: &[bool; 256],
) -> Result<(Vec<(i32, i32, i32, i32)>, Vec<(u8, [u8; 3])>, Value), String> {
    let clean = match clean {
        Value::Bool(true) => json!({"colors": 8, "despeckle": 1}),
        Value::Object(_) => clean.clone(),
        _ => return Err("clean must be true or the options of reduce_palette".into()),
    };
    let mut rgb: [[u8; 3]; 256] =
        std::array::from_fn(|s| [palette[s][0], palette[s][1], palette[s][2]]);
    let mut taken = *used;
    let mut exclusive = [false; 256];
    for (slot, colour) in &new_slots {
        rgb[*slot as usize] = *colour;
        taken[*slot as usize] = true;
        exclusive[*slot as usize] = true;
    }
    for w in &writes {
        taken[w.3 as usize] = true;
    }
    taken[0] = true;
    let mut shades = crate::tools::Shades {
        colors: std::array::from_fn(|s| [rgb[s][0], rgb[s][1], rgb[s][2], palette[s][3].max(1)]),
        materials: [crate::state::Material::default(); 256],
        used: taken,
        created: Vec::new(),
    };
    let cells: HashMap<Cell, u8> = writes
        .iter()
        .map(|w| ((w.0, w.1, w.2), w.3 as u8))
        .collect();
    let (changes, recoloured, report) =
        super::edit::reduce_writes(&cells, &rgb, &clean, &mut shades, &exclusive)?;
    let mut next = cells;
    for (x, y, z, slot) in changes {
        next.insert((x, y, z), slot as u8);
    }
    for (slot, colour) in recoloured {
        if let Some(found) = new_slots.iter_mut().find(|(s, _)| *s == slot) {
            found.1 = colour;
        }
    }
    for shade in &shades.created {
        new_slots.push((shade.slot, [shade.rgba[0], shade.rgba[1], shade.rgba[2]]));
    }
    let mut writes: Vec<(i32, i32, i32, i32)> = next
        .into_iter()
        .map(|(c, s)| (c.0, c.1, c.2, i32::from(s)))
        .collect();
    writes.sort_unstable();
    Ok((writes, new_slots, report))
}

/// How far, in degrees, `orient` `upright` may tilt the mesh's up by
/// default: a moderately tilted import is stood level, a fish (whose
/// thinnest axis lies flat) is not laid on its side.
const DEFAULT_MAX_TILT: f64 = 40.0;

/// `orient: "upright"`: the PCA turn of the mesh's vertices, as placed by
/// `upAxis` and `rotation`, folded into `options.rotation` (thinnest axis
/// up, the form narrowing upward, the taller end of the long axis to −Y).
/// Returns the reply's `upright`.
fn upright_mesh(mesh: &Mesh, options: &mut Voxelize, max_tilt: f64) -> Result<Value, String> {
    let before = crate::tools::from_euler_xyz(options.rotation);
    let mut seen = std::collections::HashSet::new();
    let mut points = Vec::new();
    for t in &mesh.triangles {
        for &v in &t.v {
            if !seen.insert(v) {
                continue;
            }
            let [x, y, z] = mesh.positions[v as usize];
            let p = match options.up {
                Up::Y => [x, -z, y],
                Up::Z => [x, y, z],
            };
            points.push(crate::tools::turned(&before, p));
        }
    }
    let found = crate::tools::upright(&points, max_tilt).map_err(|err| format!("orient: {err}"))?;
    let total = crate::tools::orient::mul(&found.turn, &before);
    options.rotation = crate::tools::euler_xyz(&total);
    let r = |v: f64| round(v, 4);
    Ok(json!({
        "up": found.up.map(r),
        "spread": found.spread.map(|s| round(s.max(0.0).sqrt(), 4)),
        "skew": r(found.skew),
        "headRise": r(found.head_rise),
        "keptUp": found.kept_up,
        "keptYaw": found.kept_yaw,
        "rotation": options.rotation.map(|v| round(v, 3)),
        "axes": crate::tools::axis_names(&total),
    }))
}

fn round(value: f64, places: i32) -> f64 {
    let k = 10f64.powi(places);
    (value * k).round() / k
}

/// Slots any cell of the document holds.
pub(crate) fn used_slots(session: &Session) -> [bool; 256] {
    let mut used = [false; 256];
    for object in &session.objects {
        for slot in object.cells.values() {
            used[*slot as usize] = true;
        }
    }
    used
}

/// MCP handler: read the file, then create the object (with `newObject`),
/// write the palette slots, and write the cells, as one undo step.
pub(super) fn apply_import(session: &mut Session, params: &Value) -> Result<Value, String> {
    let path = req_str(params, "path")?;
    let new_object = opt_bool(params, "newObject")?.unwrap_or(false);
    let name = opt_str(params, "name")?;
    if name.is_some() && !new_object {
        return Err("name is for newObject; rename the active object with set_object".into());
    }
    let mesh = crate::import::load(Path::new(&path))?;
    session.transaction(|session| {
        let mut params = params.clone();
        if new_object {
            // The object sits at `position` in the world; the mesh's anchor
            // is its local origin.
            let at = number3(&params, "position")?.unwrap_or([0.0; 3]);
            let position = at.map(|v| v.round() as i32);
            session.add_object(name.as_deref(), position, None, true)?;
            params["position"] = json!([0, 0, 0]);
        }
        let used = used_slots(session);
        let plan = import_plan(
            &mesh,
            &params,
            Up::Y,
            &session.active().cells,
            &session.palette,
            &used,
            true,
        )?;
        let changes: Vec<PaletteChange> = plan
            .new_slots
            .iter()
            .map(|(slot, rgb)| PaletteChange {
                index: *slot,
                rgba: [rgb[0], rgb[1], rgb[2], 255],
                motion: None,
                amount: None,
            })
            .collect();
        session.write_palette(&changes);
        let changed = session.write_cells(&plan.writes)?;
        let mut reply = plan.reply;
        reply.insert("changed".into(), json!(changed));
        reply.insert("object".into(), json!(session.selected));
        reply.insert("name".into(), json!(session.active().name));
        reply.insert("voxels".into(), json!(session.voxel_count()));
        Ok(Value::Object(reply))
    })
}
