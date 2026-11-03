//! The stone tools: MCP `joints`, `weather`, `moss`, `algae`, and `vines`,
//! their arguments, and their replies.
//!
//! Each tool turns its MCP arguments into writes against a cell map and a
//! reply. The MCP handler writes them to the active object as one undo step;
//! a Rhai script runs the same function on the object it builds, and the
//! editor's panels build the same arguments.

use std::collections::HashMap;

use serde_json::{json, Map, Value};

use super::{box_corners, opt_f64, opt_str, range_pair, slot_list};
use crate::state::{Cell, Session};

/// A stone tool, by its `voxel.*` method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stone {
    Joints,
    Weather,
    Moss,
    Algae,
    Vines,
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
                Stone::Moss => reply.insert("curtains".into(), json!(done.strands)),
                Stone::Vines => {
                    reply.insert("blooms".into(), json!(done.blooms));
                    reply.insert("strands".into(), json!(done.strands))
                }
                _ => None,
            };
            done.writes
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
        seed: seed(params)?,
    })
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
    let faces = match params.get("faces") {
        None | Some(Value::Null) => match tool {
            Stone::Vines => vec![Face::Side],
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
    let kind = match tool {
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
        layer,
        seed: seed(params)?,
    })
}
