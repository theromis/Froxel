//! The stone and surface tools: MCP `joints`, `column_relief`, `weather`,
//! `moss`, `algae`, `vines`, `sweep`, `bark`, and `cavity_tint`, their
//! arguments, and their replies.
//!
//! Each tool turns its MCP arguments into writes against a cell map and a
//! reply. The MCP handler writes them to the active object as one undo step;
//! a Rhai script runs the same function on the object it builds, and the
//! editor's panels build the same arguments.

use std::collections::HashMap;

use serde_json::{json, Map, Value};

use super::{
    box_corners, exclude_boxes, fill_mode, opt_f64, opt_str, range_pair, req_i32, slot_list,
    FillMode,
};
use crate::state::{Cell, Session};

/// A stone tool, by its `voxel.*` method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stone {
    Joints,
    Relief,
    Weather,
    Moss,
    Algae,
    Vines,
    Sweep,
    Bark,
}

pub(crate) type Writes = Vec<(i32, i32, i32, i32)>;

/// MCP handler: the tool's writes on the active object, one undo step.
pub(super) fn apply_stone(
    session: &mut Session,
    params: &Value,
    tool: Stone,
) -> Result<Value, String> {
    let (writes, mut reply) = stone_writes(&session.active().cells, params, tool)?;
    let changed = session.write_cells(&writes)?;
    reply.insert("changed".into(), json!(changed));
    reply.insert("voxels".into(), json!(session.voxel_count()));
    Ok(Value::Object(reply))
}

/// The writes of a stone tool against `cells` and the counts of its reply.
pub(crate) fn stone_writes(
    cells: &HashMap<Cell, u8>,
    params: &Value,
    tool: Stone,
) -> Result<(Writes, Map<String, Value>), String> {
    let mut reply = Map::new();
    let writes = match tool {
        Stone::Joints => {
            let done = crate::tools::joints(cells, &joints_spec(params)?)?;
            reply.insert("removed".into(), json!(done.removed));
            reply.insert("dirtied".into(), json!(done.dirtied));
            done.writes
        }
        Stone::Relief => {
            let done = crate::tools::column_relief(cells, &relief_spec(params)?)?;
            reply.insert("columns".into(), json!(done.columns));
            reply.insert("added".into(), json!(done.added));
            reply.insert("removed".into(), json!(done.removed));
            reply.insert("crown".into(), json!(done.crown));
            done.writes
        }
        Stone::Weather => {
            let done = crate::tools::weather(cells, &weather_spec(params)?)?;
            reply.insert("chips".into(), json!(done.chips));
            reply.insert("pits".into(), json!(done.pits));
            reply.insert("removed".into(), json!(done.removed));
            reply.insert("broken".into(), json!(done.broken));
            reply.insert("recoloured".into(), json!(done.recoloured));
            done.writes
        }
        Stone::Moss | Stone::Algae | Stone::Vines => {
            let done = crate::tools::grow(cells, &growth_spec(params, tool)?)?;
            reply.insert("added".into(), json!(done.added));
            reply.insert("painted".into(), json!(done.painted));
            match tool {
                Stone::Moss if is_cushions(params) => {
                    reply.insert("cushions".into(), json!(done.cushions));
                    reply.insert("drapes".into(), json!(done.strands))
                }
                Stone::Moss => reply.insert("curtains".into(), json!(done.strands)),
                Stone::Vines => {
                    reply.insert("blooms".into(), json!(done.blooms));
                    reply.insert("strands".into(), json!(done.strands))
                }
                _ => None,
            };
            if let Some(note) = done.note {
                reply.insert("warning".into(), json!(note));
            }
            done.writes
        }
        Stone::Bark => {
            let done = crate::tools::bark(cells, &bark_spec(params)?)?;
            reply.insert("plates".into(), json!(done.plates));
            reply.insert("removed".into(), json!(done.removed));
            reply.insert("dirtied".into(), json!(done.dirtied));
            reply.insert("painted".into(), json!(done.painted));
            reply.insert("cracks".into(), json!(done.cracks));
            done.writes
        }
        Stone::Sweep => {
            let (writes, cells_in, length) = sweep_writes(cells, params)?;
            reply.insert("cells".into(), json!(cells_in));
            reply.insert("length".into(), json!((length * 100.0).round() / 100.0));
            writes
        }
    };
    Ok((writes, reply))
}

fn region(params: &Value, tool: &str) -> Result<crate::tools::Region, String> {
    let (x0, y0, z0, x1, y1, z1) = box_corners(params)?;
    crate::tools::Region::new((x0, y0, z0), (x1, y1, z1), tool)
}

fn seed(params: &Value) -> Result<u64, String> {
    match params.get("seed") {
        None | Some(Value::Null) => Ok(0),
        Some(found) => found
            .as_u64()
            .ok_or_else(|| "seed must be a whole number of at least 0".into()),
    }
}

fn whole(params: &Value, key: &str, default: i64, lo: i64, hi: i64) -> Result<i64, String> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(found) => found
            .as_i64()
            .filter(|v| (lo..=hi).contains(v))
            .ok_or_else(|| format!("{key} must be a whole number {lo}..={hi}")),
    }
}

/// One palette index, or none.
fn one_slot(params: &Value, key: &str) -> Result<Option<u8>, String> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(found) => found
            .as_i64()
            .filter(|v| (1..=255).contains(v))
            .map(|v| Some(v as u8))
            .ok_or_else(|| format!("{key} must be a palette index 1..=255")),
    }
}

fn whole_pair(params: &Value, key: &str, default: (i32, i32)) -> Result<(i32, i32), String> {
    let (lo, hi) = range_pair(params, key, (f64::from(default.0), f64::from(default.1)))?;
    if lo.fract() != 0.0 || hi.fract() != 0.0 {
        return Err(format!("{key} must be whole numbers"));
    }
    Ok((lo as i32, hi as i32))
}

/// MCP `joints` arguments as a spec.
pub(crate) fn joints_spec(params: &Value) -> Result<crate::tools::JointSpec, String> {
    use crate::tools::JointPattern;
    let pattern = opt_str(params, "pattern")?
        .ok_or_else(|| format!("pattern is required: {}", JointPattern::NAMES))?;
    let pattern = JointPattern::parse(&pattern)
        .ok_or_else(|| format!("pattern must be {}, not `{pattern}`", JointPattern::NAMES))?;
    let (spacing, course) = pattern.defaults();
    Ok(crate::tools::JointSpec {
        region: region(params, "joint")?,
        pattern,
        spacing: range_pair(params, "spacing", spacing)?,
        course: whole_pair(params, "course", course)?,
        width: whole(params, "width", 1, 1, 3)? as i32,
        depth: whole(params, "depth", 2, 1, 16)? as u32,
        dirt: one_slot(params, "dirt")?,
        only: slot_list(params, "only")?,
        density: opt_f64(params, "density")?.unwrap_or(0.8),
        exclude: exclude_boxes(params)?,
        seed: seed(params)?,
    })
}

/// MCP `column_relief` arguments as a spec.
pub(crate) fn relief_spec(params: &Value) -> Result<crate::tools::ReliefSpec, String> {
    let bad =
        "facing must be +x, -x, +y, -y, or one of [1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0]";
    let facing = match params.get("facing") {
        None | Some(Value::Null) => return Err(format!("facing is required: {bad}")),
        Some(Value::String(name)) => match name.as_str() {
            "+x" | "x" => (0, 1),
            "-x" => (0, -1),
            "+y" | "y" => (1, 1),
            "-y" => (1, -1),
            _ => return Err(bad.into()),
        },
        Some(_) => {
            let v = super::opt_vec3(params, "facing").map_err(|_| bad.to_string())?;
            let v = v.expect("present");
            crate::tools::relief_facing(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]))
                .ok_or(bad)?
        }
    };
    let palette = one_slot(params, "palette")?
        .ok_or("palette is required: the slot of the columns, usually the stone")?;
    let lower = match params.get("lower") {
        None | Some(Value::Null) => true,
        Some(found) => found.as_bool().ok_or("lower must be true or false")?,
    };
    let crown = match params.get("crown") {
        None | Some(Value::Null) => None,
        Some(found) => Some(
            super::as_i32(found).ok_or("crown must be a whole number: the height of the crown")?,
        ),
    };
    Ok(crate::tools::ReliefSpec {
        region: region(params, "column_relief")?,
        facing,
        width: whole_pair(params, "width", (6, 12))?,
        out: whole_pair(params, "out", (1, 4))?,
        crown,
        top: whole_pair(params, "top", (-8, 4))?,
        lower,
        depth: whole(params, "depth", 3, 1, 16)? as i32,
        palette,
        exclude: exclude_boxes(params)?,
        seed: seed(params)?,
    })
}

/// MCP `weather` arguments as a spec.
pub(crate) fn weather_spec(params: &Value) -> Result<crate::tools::WeatherSpec, String> {
    Ok(crate::tools::WeatherSpec {
        region: region(params, "weather")?,
        chip: opt_f64(params, "chip")?.unwrap_or(0.15),
        chip_size: whole(params, "chipSize", 2, 1, 8)? as u32,
        pits: opt_f64(params, "pits")?.unwrap_or(0.02),
        pit_depth: whole(params, "pitDepth", 1, 1, 4)? as u32,
        broken: one_slot(params, "broken")?,
        noise: slot_list(params, "noise")?,
        noise_density: opt_f64(params, "noiseDensity")?.unwrap_or(0.35),
        noise_scale: opt_f64(params, "noiseScale")?.unwrap_or(3.0),
        only: slot_list(params, "only")?,
        exclude: exclude_boxes(params)?,
        seed: seed(params)?,
    })
}

/// Whether MCP `moss` grows cushions (`style` `cushions`) rather than
/// caps and curtains (`caps`, the default).
fn is_cushions(params: &Value) -> bool {
    params.get("style").and_then(Value::as_str) == Some("cushions")
}

fn moss_style(params: &Value) -> Result<(), String> {
    match opt_str(params, "style")?.as_deref() {
        None | Some("caps") | Some("cushions") => Ok(()),
        Some(other) => Err(format!("style must be caps or cushions, not `{other}`")),
    }
}

/// Points along the wood: `[x, y, z]`, or the points of `fill_tube`
/// (`[x, y, z, radius]`) or `sweep` (`[x, y, z, twist, scale]`), whose
/// extra numbers are ignored, or `{x, y, z}`.
fn path_points(list: &[Value]) -> Result<Vec<[f64; 3]>, String> {
    list.iter()
        .enumerate()
        .map(|(index, point)| {
            let bad = || format!("point {index} must be [x, y, z, ...] or {{x, y, z}}");
            let mut at = [0.0; 3];
            for (axis, name) in ["x", "y", "z"].into_iter().enumerate() {
                let value = match point {
                    Value::Array(arr) if (3..=5).contains(&arr.len()) => arr.get(axis),
                    Value::Object(_) => point.get(name),
                    _ => return Err(bad()),
                };
                at[axis] = value.and_then(Value::as_f64).ok_or_else(bad)?;
            }
            Ok(at)
        })
        .collect()
}

/// MCP `bark` arguments as a spec.
pub(crate) fn bark_spec(params: &Value) -> Result<crate::tools::BarkSpec, String> {
    use crate::tools::BarkDirection;
    let direction = match (params.get("axis"), params.get("points")) {
        (Some(_), Some(_)) => return Err("pass axis or points, not both".into()),
        (Some(Value::String(name)), None) => BarkDirection::Axis(match name.as_str() {
            "x" | "+x" | "-x" => [1.0, 0.0, 0.0],
            "y" | "+y" | "-y" => [0.0, 1.0, 0.0],
            "z" | "+z" | "-z" => [0.0, 0.0, 1.0],
            _ => return Err(format!("axis must be x, y, z, or [x, y, z], not `{name}`")),
        }),
        (Some(_), None) => {
            let v = super::opt_vec3(params, "axis")
                .map_err(|_| "axis must be x, y, z, or [x, y, z]".to_string())?
                .expect("present");
            BarkDirection::Axis([f64::from(v[0]), f64::from(v[1]), f64::from(v[2])])
        }
        (None, Some(Value::Array(list))) => BarkDirection::Path {
            points: path_points(list)?,
            curve: match opt_str(params, "curve")?.as_deref() {
                None | Some("polyline") => false,
                Some("smooth") => true,
                Some(other) => {
                    return Err(format!("curve must be polyline or smooth, not `{other}`"))
                }
            },
        },
        (None, Some(_)) => return Err("points must be a list of [x, y, z]".into()),
        (None, None) => {
            return Err(
                "axis or points is required: the way the grain runs (x, y, z, a vector, or the points of the tube)"
                    .into(),
            )
        }
    };
    let palettes = slot_list(params, "palettes")?;
    if palettes.is_empty() {
        return Err("palettes is required: the plate slots, a few bark tones".into());
    }
    Ok(crate::tools::BarkSpec {
        region: region(params, "bark")?,
        direction,
        palettes,
        dirt: one_slot(params, "dirt")?,
        width: range_pair(params, "width", (3.0, 6.0))?,
        length: range_pair(params, "length", (8.0, 20.0))?,
        depth: {
            let (lo, hi) = whole_pair(params, "depth", (1, 2))?;
            if lo < 1 || hi < lo || hi > 4 {
                return Err(format!(
                    "depth must be [min, max] with 1 <= min <= max <= 4, not [{lo}, {hi}]"
                ));
            }
            (lo as u32, hi as u32)
        },
        cracks: opt_f64(params, "cracks")?.unwrap_or(0.25),
        only: slot_list(params, "only")?,
        exclude: exclude_boxes(params)?,
        seed: seed(params)?,
    })
}

/// MCP `cavity_tint`: the writes against `cells` and the counts of its
/// reply. Shades it makes go into `shades.created`; the caller writes them
/// to the palette in the same undo step.
pub(crate) fn cavity_writes(
    cells: &HashMap<Cell, u8>,
    params: &Value,
    shades: &mut crate::tools::Shades,
) -> Result<(Writes, Map<String, Value>), String> {
    use crate::tools::Tint;
    let lighten = match params.get("lighten") {
        None | Some(Value::Null) => false,
        Some(found) => found.as_bool().ok_or("lighten must be true or false")?,
    };
    let spec = crate::tools::CavitySpec {
        region: region(params, "cavity_tint")?,
        radius: whole(
            params,
            "radius",
            2,
            1,
            i64::from(crate::tools::MAX_CAVITY_RADIUS),
        )? as i32,
        dark: opt_f64(params, "dark")?.unwrap_or(0.12),
        light: if lighten {
            Some(opt_f64(params, "light")?.unwrap_or(0.25))
        } else {
            None
        },
        only: slot_list(params, "only")?,
        exclude: exclude_boxes(params)?,
    };
    // `pairs`: [slot, darker] or [slot, darker, lighter] per slot.
    let mut pairs: HashMap<u8, (Option<u8>, Option<u8>)> = HashMap::new();
    let given = match params.get("pairs") {
        None | Some(Value::Null) => false,
        Some(Value::Array(list)) => {
            let bad = "pairs must be a list of [slot, darker] or [slot, darker, lighter]";
            for entry in list {
                let row: Vec<i32> = entry
                    .as_array()
                    .filter(|row| (2..=3).contains(&row.len()))
                    .and_then(|row| row.iter().map(super::as_i32).collect())
                    .ok_or(bad)?;
                if row.iter().any(|v| !(0..=255).contains(v)) || row[0] == 0 {
                    return Err(format!(
                        "{bad}, palette indices 1..=255 (0 leaves that tint out)"
                    ));
                }
                let pick = |v: Option<&i32>| v.filter(|v| **v > 0).map(|v| *v as u8);
                pairs.insert(row[0] as u8, (pick(row.get(1)), pick(row.get(2))));
            }
            true
        }
        Some(_) => return Err("pairs must be a list of [slot, darker, lighter?]".into()),
    };
    let darken = opt_f64(params, "darken")?.unwrap_or(0.72);
    let brighten = opt_f64(params, "brighten")?.unwrap_or(1.18);
    if !(0.05..1.0).contains(&darken) {
        return Err("darken must be 0.05..1, the factor of a darker shade".into());
    }
    if !(1.0..=4.0).contains(&brighten) || brighten == 1.0 {
        return Err("brighten must be above 1 and at most 4, the factor of a lighter shade".into());
    }
    let found = crate::tools::cavities(cells, &spec)?;
    let mut made: HashMap<(u8, bool), u8> = HashMap::new();
    let mut writes = Vec::new();
    let (mut darkened, mut lightened) = (0, 0);
    for spot in found {
        let dark = spot.tint == Tint::Dark;
        let target = if given {
            pairs
                .get(&spot.slot)
                .and_then(|(d, l)| if dark { *d } else { *l })
        } else if let Some(slot) = made.get(&(spot.slot, dark)) {
            Some(*slot)
        } else {
            let slot = shades.shade(spot.slot, if dark { darken } else { brighten })?;
            made.insert((spot.slot, dark), slot);
            Some(slot)
        };
        let Some(target) = target.filter(|t| *t != spot.slot) else {
            continue;
        };
        let (x, y, z) = spot.cell;
        writes.push((x, y, z, i32::from(target)));
        if dark {
            darkened += 1;
        } else {
            lightened += 1;
        }
    }
    let mut reply = Map::new();
    reply.insert("darkened".into(), json!(darkened));
    reply.insert("lightened".into(), json!(lightened));
    reply.insert(
        "shades".into(),
        json!(shades
            .created
            .iter()
            .map(|s| json!({"slot": s.slot, "from": s.from, "rgba": s.rgba}))
            .collect::<Vec<_>>()),
    );
    Ok((writes, reply))
}

/// MCP handler of `cavity_tint`: the writes and any new shades on the
/// active object and the palette, one undo step.
pub(super) fn apply_cavity(session: &mut Session, params: &Value) -> Result<Value, String> {
    let mut shades = session.shades();
    let (writes, mut reply) = cavity_writes(&session.active().cells, params, &mut shades)?;
    let changed = session.transaction(|session| {
        session.begin_edit();
        for shade in &shades.created {
            session.palette[shade.slot as usize] = shade.rgba;
            session.materials[shade.slot as usize] = session.materials[shade.from as usize];
        }
        let changed = session.write_cells(&writes);
        session.end_edit();
        changed
    })?;
    session.mesh_epoch = session.mesh_epoch.wrapping_add(1);
    reply.insert("changed".into(), json!(changed));
    reply.insert("voxels".into(), json!(session.voxel_count()));
    Ok(Value::Object(reply))
}

/// MCP `moss`, `algae`, or `vines` arguments as a spec.
pub(crate) fn growth_spec(params: &Value, tool: Stone) -> Result<crate::tools::GrowthSpec, String> {
    use crate::tools::{Face, GrowthKind, Layer};
    let name = match tool {
        Stone::Moss => "moss",
        Stone::Algae => "algae",
        _ => "vines",
    };
    let palettes = slot_list(params, "palettes")?;
    if palettes.is_empty() {
        return Err("palettes is required: a palette index or a list of them".into());
    }
    if tool == Stone::Moss {
        moss_style(params)?;
    }
    let has_facing = !matches!(params.get("facing"), None | Some(Value::Null));
    let faces = match params.get("faces") {
        None | Some(Value::Null) => match tool {
            Stone::Vines => vec![Face::Side],
            // facing alone picks the faces: a downward one takes undersides.
            _ if has_facing => vec![Face::Top, Face::Side, Face::Bottom],
            _ => vec![Face::Top, Face::Side],
        },
        Some(found) => {
            let names: Vec<Value> = match found {
                Value::Array(list) => list.clone(),
                one => vec![one.clone()],
            };
            let mut faces = Vec::new();
            for face in names {
                let parsed = face
                    .as_str()
                    .and_then(Face::parse)
                    .ok_or("faces must be top, side, bottom, all, or a list of them")?;
                for face in parsed {
                    if !faces.contains(&face) {
                        faces.push(face);
                    }
                }
            }
            faces
        }
    };
    let facing = match params.get("facing") {
        None | Some(Value::Null) => None,
        Some(_) => {
            let v = super::opt_vec3(params, "facing")?.expect("present");
            Some([f64::from(v[0]), f64::from(v[1]), f64::from(v[2])])
        }
    };
    let band = match params.get("band") {
        None | Some(Value::Null) => None,
        Some(_) => Some(whole_pair(params, "band", (0, 0))?),
    };
    let layer = match opt_str(params, "layer")?.as_deref() {
        None => match tool {
            Stone::Algae => Layer::Paint,
            _ => Layer::Grow,
        },
        Some("grow") => Layer::Grow,
        Some("paint") => Layer::Paint,
        Some(other) => return Err(format!("layer must be grow or paint, not `{other}`")),
    };
    let cushions = tool == Stone::Moss && is_cushions(params);
    let kind = match tool {
        Stone::Moss if cushions => GrowthKind::Cushions {
            size: whole_pair(params, "size", (2, 5))?,
            gap: whole(params, "gap", 2, 1, 8)? as i32,
            exposure: opt_f64(params, "exposure")?.unwrap_or(0.35),
            drapes: opt_f64(params, "drapes")?.unwrap_or(0.25),
            length: whole_pair(params, "length", (2, 6))?,
            thickness: whole(params, "thickness", 2, 1, 4)? as i32,
        },
        Stone::Moss => GrowthKind::Moss {
            curtains: opt_f64(params, "curtains")?.unwrap_or(0.3),
            length: whole_pair(params, "length", (2, 8))?,
            thickness: whole(params, "thickness", 1, 1, 3)? as i32,
        },
        Stone::Algae => GrowthKind::Algae,
        _ => GrowthKind::Vines {
            length: whole_pair(params, "length", (4, 16))?,
            blooms: slot_list(params, "blooms")?,
            bloom_chance: opt_f64(params, "bloomChance")?.unwrap_or(0.06),
        },
    };
    let amount = match tool {
        Stone::Moss if cushions => 0.45,
        Stone::Moss => 0.5,
        Stone::Algae => 0.9,
        _ => 0.08,
    };
    Ok(crate::tools::GrowthSpec {
        kind,
        region: region(params, name)?,
        palettes,
        amount: opt_f64(params, "amount")?.unwrap_or(amount),
        noise: opt_f64(params, "noise")?.unwrap_or(0.5),
        faces,
        facing,
        band,
        only: slot_list(params, "only")?,
        exclude: exclude_boxes(params)?,
        layer,
        seed: seed(params)?,
    })
}

/// The writes of MCP `sweep` against `cells`, how many cells the sweep
/// covers, and the path length.
fn sweep_writes(cells: &HashMap<Cell, u8>, params: &Value) -> Result<(Writes, usize, f64), String> {
    let list = params.get("points").and_then(Value::as_array).ok_or(
        "points must be a list of [x, y, z], [x, y, z, twist], or [x, y, z, twist, scale]",
    )?;
    let mut points = Vec::with_capacity(list.len());
    for (index, point) in list.iter().enumerate() {
        let number = |value: Option<&Value>, what: &str| -> Result<Option<f64>, String> {
            match value {
                None | Some(Value::Null) => Ok(None),
                Some(found) => found
                    .as_f64()
                    .map(Some)
                    .ok_or_else(|| format!("point {index} {what} must be a number")),
            }
        };
        let names = ["x", "y", "z", "twist", "scale"];
        let mut values = [None; 5];
        if let Some(arr) = point.as_array() {
            if !(3..=5).contains(&arr.len()) {
                return Err(format!(
                    "point {index} must be [x, y, z], [x, y, z, twist], or [x, y, z, twist, scale]"
                ));
            }
            for (slot, name) in names.iter().enumerate() {
                values[slot] = number(arr.get(slot), name)?;
            }
        } else if point.is_object() {
            for (slot, name) in names.iter().enumerate() {
                values[slot] = number(point.get(*name), name)?;
            }
        } else {
            return Err(format!(
                "point {index} must be [x, y, z, twist?, scale?] or {{x, y, z, twist?, scale?}}"
            ));
        }
        let mut at = [0.0; 3];
        for axis in 0..3 {
            at[axis] =
                values[axis].ok_or_else(|| format!("point {index} is missing {}", names[axis]))?;
        }
        points.push(crate::tools::SweepPoint {
            at,
            twist: values[3].unwrap_or(0.0),
            scale: values[4].unwrap_or(1.0),
        });
    }
    let curve = match opt_str(params, "curve")?.as_deref() {
        None | Some("polyline") => false,
        Some("smooth") => true,
        Some(other) => return Err(format!("curve must be polyline or smooth, not `{other}`")),
    };
    let profile = match params.get("profile") {
        None | Some(Value::Null) => {
            let size = |key: &str| {
                opt_f64(params, key)?.ok_or_else(|| {
                    format!("{key} is required without a profile polygon (or pass profile)")
                })
            };
            crate::tools::Profile::Rect {
                width: size("width")?,
                height: size("height")?,
                radius: opt_f64(params, "radius")?.unwrap_or(0.0),
            }
        }
        Some(Value::Array(list)) => crate::tools::Profile::Polygon(
            list.iter()
                .map(|p| match p.as_array().map(|a| a.as_slice()) {
                    Some([u, v]) => match (u.as_f64(), v.as_f64()) {
                        (Some(u), Some(v)) => Ok([u, v]),
                        _ => Err("profile points must be [u, v] numbers".to_string()),
                    },
                    _ => Err("profile points must be [u, v]".to_string()),
                })
                .collect::<Result<_, _>>()?,
        ),
        Some(_) => return Err("profile must be a list of [u, v] points".into()),
    };
    let palettes = match (params.get("palette"), params.get("palettes")) {
        (Some(_), Some(_)) => return Err("pass palette or palettes, not both".into()),
        (Some(_), None) => {
            let slot = req_i32(params, "palette")?;
            if !(1..=255).contains(&slot) {
                return Err(format!("palette must be 1..255, not {slot}"));
            }
            vec![slot as u8]
        }
        _ => slot_list(params, "palettes")?,
    };
    if palettes.is_empty() {
        return Err("palette or palettes is required".into());
    }
    let period = opt_f64(params, "period")?.unwrap_or(2.0);
    if !(0.5..=512.0).contains(&period) {
        return Err("period must be 0.5..=512".into());
    }
    let pattern = opt_str(params, "pattern")?.unwrap_or_else(|| "bands".into());
    if !["bands", "rings", "random"].contains(&pattern.as_str()) {
        return Err(format!(
            "pattern must be bands, rings, or random, not `{pattern}`"
        ));
    }
    let seed = seed(params)?;
    let mode = fill_mode(params)?;
    let swept = crate::tools::sweep(&points, curve, &profile)?;
    let length = swept.iter().map(|c| c.along).fold(0.0, f64::max);
    let count = swept.len();
    let writes = swept
        .into_iter()
        .filter(|c| match mode {
            FillMode::Replace => true,
            FillMode::Empty => !cells.contains_key(&c.cell),
            FillMode::Paint => cells.contains_key(&c.cell),
        })
        .map(|c| {
            let index = match pattern.as_str() {
                "bands" => (c.v / period).floor() as i64,
                "rings" => (c.along / period).floor() as i64,
                _ => (cell_hash(c.cell, seed) >> 1) as i64,
            };
            let slot = palettes[index.rem_euclid(palettes.len() as i64) as usize];
            let (x, y, z) = c.cell;
            (x, y, z, i32::from(slot))
        })
        .collect();
    Ok((writes, count, length))
}

/// A stable per-cell value for the `random` pattern.
fn cell_hash(cell: Cell, seed: u64) -> u64 {
    let mut h = seed ^ 0x9e37_79b9_7f4a_7c15;
    for v in [cell.0, cell.1, cell.2] {
        h ^= u64::from(v as u32);
        h = h.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        h ^= h >> 31;
    }
    h
}
