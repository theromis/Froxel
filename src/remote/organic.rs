//! The organic shaping tools: MCP `deform`, `loft`, and `brush`, their
//! arguments, and their replies.
//!
//! Each tool turns its MCP arguments into writes against a cell map and a
//! reply. The MCP handler writes them to the active object as one undo step;
//! a Rhai script runs the same function on the object it builds, and the
//! editor's Deform and Loft panels and Sculpt tool build the same arguments.

use std::collections::HashMap;

use serde_json::{json, Map, Value};

use super::stone::Writes;
use super::{box_corners, fill_mode, opt_f64, opt_str, req_i32, slot_list, FillMode};
use crate::state::{Cell, Session};

/// An organic tool, by its `voxel.*` method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Organic {
    Deform,
    Loft,
    Brush,
}

/// MCP handler: the tool's writes on the active object, one undo step.
pub(super) fn apply_organic(
    session: &mut Session,
    params: &Value,
    tool: Organic,
) -> Result<Value, String> {
    let (writes, mut reply) = organic_writes(&session.active().cells, params, tool)?;
    let changed = session.write_cells(&writes)?;
    reply.insert("changed".into(), json!(changed));
    reply.insert("voxels".into(), json!(session.voxel_count()));
    Ok(Value::Object(reply))
}

/// The writes of an organic tool against `cells` and the counts of its reply.
pub(crate) fn organic_writes(
    cells: &HashMap<Cell, u8>,
    params: &Value,
    tool: Organic,
) -> Result<(Writes, Map<String, Value>), String> {
    let mut reply = Map::new();
    let writes = match tool {
        Organic::Deform => {
            let done = crate::tools::deform(cells, &deform_spec(params)?)?;
            reply.insert("cells".into(), json!(done.cells));
            reply.insert("result".into(), json!(done.result));
            reply.insert("added".into(), json!(done.added));
            reply.insert("removed".into(), json!(done.removed));
            done.writes
        }
        Organic::Loft => {
            let (writes, count, length) = loft_writes(cells, params)?;
            reply.insert("cells".into(), json!(count));
            reply.insert("length".into(), json!((length * 100.0).round() / 100.0));
            writes
        }
        Organic::Brush => {
            let done = crate::tools::brush(cells, &brush_spec(params)?)?;
            reply.insert("dabs".into(), json!(done.dabs));
            reply.insert("added".into(), json!(done.added));
            reply.insert("removed".into(), json!(done.removed));
            done.writes
        }
    };
    Ok((writes, reply))
}

fn number(params: &Value, key: &str, default: f64) -> Result<f64, String> {
    Ok(opt_f64(params, key)?.unwrap_or(default))
}

fn seed(params: &Value) -> Result<u64, String> {
    match params.get("seed") {
        None | Some(Value::Null) => Ok(0),
        Some(found) => found
            .as_u64()
            .ok_or_else(|| "seed must be a whole number of at least 0".into()),
    }
}

/// `x`, `y`, or `z` as 0, 1, or 2.
fn axis_name(name: &str) -> Option<usize> {
    match name {
        "x" | "+x" => Some(0),
        "y" | "+y" => Some(1),
        "z" | "+z" => Some(2),
        _ => None,
    }
}

/// MCP `deform` arguments as a spec.
pub(crate) fn deform_spec(params: &Value) -> Result<crate::tools::DeformSpec, String> {
    use crate::tools::{DeformOp, Displace};
    let op = opt_str(params, "op")?.ok_or("op is required: bend, taper, twist, or displace")?;
    let axis = match opt_str(params, "axis")?.as_deref() {
        None => 2,
        Some(name) => {
            axis_name(name).ok_or_else(|| format!("axis must be x, y, or z, not `{name}`"))?
        }
    };
    let corners = ["x0", "y0", "z0", "x1", "y1", "z1"];
    let given = corners.iter().filter(|k| params.get(**k).is_some()).count();
    let region = match given {
        0 => None,
        6 => {
            let (x0, y0, z0, x1, y1, z1) = box_corners(params)?;
            Some(crate::tools::Region::new(
                (x0, y0, z0),
                (x1, y1, z1),
                "deform",
            )?)
        }
        _ => {
            return Err(
                "pass all of x0, y0, z0, x1, y1, z1 for a region, or none for the whole object"
                    .into(),
            )
        }
    };
    let center = match params.get("center") {
        None | Some(Value::Null) => None,
        Some(found) => {
            let pair = found
                .as_array()
                .filter(|list| list.len() == 2)
                .and_then(|list| list.iter().map(Value::as_f64).collect::<Option<Vec<_>>>())
                .ok_or(
                    "center must be [a, b]: the two coordinates across the axis, in x, y, z order",
                )?;
            Some([pair[0], pair[1]])
        }
    };
    let angle = |what: &str| -> Result<f64, String> {
        Ok(opt_f64(params, "angle")?
            .ok_or_else(|| format!("angle is required for {what}, in degrees"))?
            .to_radians())
    };
    let op = match op.as_str() {
        "bend" => {
            let toward = match opt_str(params, "direction")?.as_deref() {
                None => {
                    if axis == 2 {
                        (0, 1.0)
                    } else {
                        (2, 1.0)
                    }
                }
                Some(name) => {
                    let (sign, rest) = match name.strip_prefix('-') {
                        Some(rest) => (-1.0, rest),
                        None => (1.0, name.strip_prefix('+').unwrap_or(name)),
                    };
                    let index = axis_name(rest).ok_or_else(|| {
                        format!("direction must be +x, -x, +y, -y, +z, or -z, not `{name}`")
                    })?;
                    if index == axis {
                        return Err("direction must lie across the axis, not along it".into());
                    }
                    (index, sign)
                }
            };
            DeformOp::Bend {
                angle: angle("bend")?,
                toward,
            }
        }
        "taper" => DeformOp::Taper {
            start: number(params, "start", 1.0)?,
            end: number(params, "end", 0.5)?,
        },
        "twist" => DeformOp::Twist {
            angle: angle("twist")?,
        },
        "displace" => {
            let octaves = match params.get("octaves") {
                None | Some(Value::Null) => 2,
                Some(found) => found
                    .as_u64()
                    .filter(|v| (1..=6).contains(v))
                    .ok_or("octaves must be a whole number 1..=6")?
                    as u32,
            };
            DeformOp::Displace(Displace {
                amplitude: number(params, "amplitude", 2.0)?,
                scale: number(params, "scale", 8.0)?,
                octaves,
                seed: seed(params)?,
                bias: number(params, "bias", 0.0)?,
            })
        }
        other => {
            return Err(format!(
                "op must be bend, taper, twist, or displace, not `{other}`"
            ))
        }
    };
    Ok(crate::tools::DeformSpec {
        op,
        region,
        axis,
        origin: opt_f64(params, "origin")?,
        length: opt_f64(params, "length")?,
        center,
    })
}

/// The loft points of `params`: `[x, y, z]`, `[x, y, z, r]`,
/// `[x, y, z, rx, ry]`, `[x, y, z, rx, ry, roll]`, or a map with `x`, `y`,
/// `z` and any of `radius`, `rx`, `ry`, `roll`, `flatTop`, `noise`,
/// `frequency`. A missing value takes the call's default.
pub(crate) fn loft_points(params: &Value) -> Result<Vec<crate::tools::LoftPoint>, String> {
    let list = params.get("points").and_then(Value::as_array).ok_or(
        "points must be a list of [x, y, z], [x, y, z, r], [x, y, z, rx, ry], [x, y, z, rx, ry, roll], or {x, y, z, rx?, ry?, ...}",
    )?;
    let radius = opt_f64(params, "radius")?;
    let default_rx = opt_f64(params, "rx")?.or(radius);
    let default_ry = opt_f64(params, "ry")?.or(default_rx);
    let defaults = crate::tools::Section {
        rx: 0.0,
        ry: 0.0,
        roll: number(params, "roll", 0.0)?.to_radians(),
        flat: number(params, "flatTop", 0.0)?,
        noise: number(params, "noise", 0.0)?,
        frequency: number(params, "frequency", 5.0)?,
    };
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
        let mut at = [0.0; 3];
        let mut section = defaults;
        let (rx, ry);
        if let Some(arr) = point.as_array() {
            if !(3..=6).contains(&arr.len()) {
                return Err(format!(
                    "point {index} must be [x, y, z], [x, y, z, r], [x, y, z, rx, ry], or [x, y, z, rx, ry, roll]"
                ));
            }
            for (axis, name) in ["x", "y", "z"].iter().enumerate() {
                at[axis] = number(arr.get(axis), name)?.unwrap_or(0.0);
            }
            rx = number(arr.get(3), "rx")?;
            ry = number(arr.get(4), "ry")?.or(rx);
            if let Some(roll) = number(arr.get(5), "roll")? {
                section.roll = roll.to_radians();
            }
        } else if point.is_object() {
            for (axis, name) in ["x", "y", "z"].iter().enumerate() {
                at[axis] = number(point.get(*name), name)?
                    .ok_or_else(|| format!("point {index} is missing {name}"))?;
            }
            let r = number(point.get("radius"), "radius")?;
            rx = number(point.get("rx"), "rx")?.or(r);
            ry = number(point.get("ry"), "ry")?.or(r).or(rx);
            if let Some(roll) = number(point.get("roll"), "roll")? {
                section.roll = roll.to_radians();
            }
            if let Some(flat) = number(point.get("flatTop"), "flatTop")? {
                section.flat = flat;
            }
            if let Some(noise) = number(point.get("noise"), "noise")? {
                section.noise = noise;
            }
            if let Some(frequency) = number(point.get("frequency"), "frequency")? {
                section.frequency = frequency;
            }
            for key in point.as_object().expect("an object").keys() {
                if ![
                    "x",
                    "y",
                    "z",
                    "radius",
                    "rx",
                    "ry",
                    "roll",
                    "flatTop",
                    "noise",
                    "frequency",
                ]
                .contains(&key.as_str())
                {
                    return Err(format!("point {index} has no field `{key}`"));
                }
            }
        } else {
            return Err(format!(
                "point {index} must be a list of numbers or {{x, y, z, rx?, ry?, roll?, flatTop?, noise?, frequency?}}"
            ));
        }
        section.rx = rx
            .or(default_rx)
            .ok_or_else(|| format!("point {index} has no radius; give it one or pass radius"))?;
        section.ry = ry.or(default_ry).unwrap_or(section.rx);
        points.push(crate::tools::LoftPoint { at, section });
    }
    Ok(points)
}

/// The writes of MCP `loft` against `cells`, how many cells the loft
/// covers, and the path length.
fn loft_writes(cells: &HashMap<Cell, u8>, params: &Value) -> Result<(Writes, usize, f64), String> {
    let points = loft_points(params)?;
    let curve = match opt_str(params, "curve")?.as_deref() {
        None | Some("polyline") => false,
        Some("smooth") => true,
        Some(other) => return Err(format!("curve must be polyline or smooth, not `{other}`")),
    };
    let caps = match opt_str(params, "caps")?.as_deref() {
        None | Some("flat") => crate::tools::Caps::Flat,
        Some("round") => crate::tools::Caps::Round,
        Some(other) => return Err(format!("caps must be flat or round, not `{other}`")),
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
    let period = number(params, "period", 2.0)?;
    if !(0.5..=512.0).contains(&period) {
        return Err("period must be 0.5..=512".into());
    }
    let sectors = match params.get("sectors") {
        None | Some(Value::Null) => palettes.len() as u64,
        Some(found) => found
            .as_u64()
            .filter(|v| (1..=256).contains(v))
            .ok_or("sectors must be a whole number 1..=256")?,
    };
    let seed = seed(params)?;
    let pattern = match opt_str(params, "pattern")?.as_deref() {
        None | Some("rings") => crate::tools::TubePattern::Rings { period },
        Some("stripes") => crate::tools::TubePattern::Stripes {
            sectors: sectors as u32,
        },
        Some("random") => crate::tools::TubePattern::Random { seed },
        Some(other) => {
            return Err(format!(
                "pattern must be rings, stripes, or random, not `{other}`"
            ))
        }
    };
    let mode = fill_mode(params)?;
    let lofted = crate::tools::loft(&points, curve, caps, seed)?;
    let length = crate::tools::loft_length(&points, curve);
    let count = lofted.len();
    let writes = lofted
        .iter()
        .filter(|c| match mode {
            FillMode::Replace => true,
            FillMode::Empty => !cells.contains_key(&c.cell),
            FillMode::Paint => cells.contains_key(&c.cell),
        })
        .map(|c| {
            let slot = palettes[pattern.pick(c, palettes.len())];
            let (x, y, z) = c.cell;
            (x, y, z, i32::from(slot))
        })
        .collect();
    Ok((writes, count, length))
}

/// MCP `brush` arguments as a spec.
pub(crate) fn brush_spec(params: &Value) -> Result<crate::tools::BrushSpec, String> {
    use crate::tools::{BrushOp, Dab, Falloff};
    let op = opt_str(params, "op")?.ok_or_else(|| format!("op is required: {}", BrushOp::NAMES))?;
    let op =
        BrushOp::parse(&op).ok_or_else(|| format!("op must be {}, not `{op}`", BrushOp::NAMES))?;
    let radius = number(params, "radius", 3.0)?;
    let strength = number(
        params,
        "strength",
        if op == BrushOp::Grab { 1.0 } else { 0.5 },
    )?;
    let list = params.get("points").and_then(Value::as_array).ok_or(
        "points must be a list of [x, y, z], [x, y, z, radius], or [x, y, z, radius, strength]",
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
        let names = ["x", "y", "z", "radius", "strength"];
        let mut values = [None; 5];
        if let Some(arr) = point.as_array() {
            if !(3..=5).contains(&arr.len()) {
                return Err(format!(
                    "point {index} must be [x, y, z], [x, y, z, radius], or [x, y, z, radius, strength]"
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
                "point {index} must be [x, y, z, radius?, strength?] or {{x, y, z, radius?, strength?}}"
            ));
        }
        let mut at = [0.0; 3];
        for axis in 0..3 {
            at[axis] =
                values[axis].ok_or_else(|| format!("point {index} is missing {}", names[axis]))?;
        }
        points.push(Dab {
            at,
            radius: values[3].unwrap_or(radius),
            strength: values[4].unwrap_or(strength),
        });
    }
    let falloff = match opt_str(params, "falloff")?.as_deref() {
        None => Falloff::Smooth,
        Some(name) => Falloff::parse(name)
            .ok_or_else(|| format!("falloff must be smooth, linear, or constant, not `{name}`"))?,
    };
    let palette = match params.get("palette") {
        None | Some(Value::Null) => None,
        Some(_) => {
            let slot = req_i32(params, "palette")?;
            if !(1..=255).contains(&slot) {
                return Err(format!("palette must be 1..255, not {slot}"));
            }
            Some(slot as u8)
        }
    };
    let vector = match params.get("vector") {
        None | Some(Value::Null) => [0.0; 3],
        Some(_) => {
            let v = super::opt_vec3(params, "vector")?.expect("present");
            [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]
        }
    };
    if op != BrushOp::Grab && vector != [0.0; 3] {
        return Err("vector is for op grab only".into());
    }
    Ok(crate::tools::BrushSpec {
        op,
        points,
        falloff,
        only: slot_list(params, "only")?,
        palette,
        vector,
        spacing: opt_f64(params, "spacing")?,
    })
}
