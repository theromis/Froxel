//! MCP `blend_shapes`: its arguments, writes, and reply.
//!
//! The MCP handler writes the blend to the active object as one undo step;
//! a Rhai script runs [`blend_writes`] on the object it builds, and the
//! editor's Blend shapes panel builds the same arguments.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Map, Value};

use super::{edit, fill_mode, intersect_writes, opt_f64, opt_str, FillMode};
use crate::state::{Cell, Session};
use crate::tools::{BlendColors, BlendOp, BlendShape, BlendSpec, Form};

/// The keys a shape takes.
const SHAPE_KEYS: &[&str] = &[
    "shape",
    "op",
    "k",
    "palette",
    "center",
    "radius",
    "radii",
    "size",
    "round",
    "rotate",
    "from",
    "to",
    "points",
    "thickness",
];

/// MCP handler: the blend on the active object, one undo step.
pub(super) fn apply_blend(session: &mut Session, params: &Value) -> Result<Value, String> {
    let (writes, mut reply) = blend_writes(&session.active().cells, params)?;
    let changed = session.write_cells(&writes)?;
    reply.insert("changed".into(), json!(changed));
    reply.insert("voxels".into(), json!(session.voxel_count()));
    Ok(Value::Object(reply))
}

/// The writes of MCP `blend_shapes` against `cells`, kept by `mode` and
/// repeated across `mirror`, and the counts of its reply. Rhai scripts call
/// this.
pub(crate) fn blend_writes(
    cells: &HashMap<Cell, u8>,
    params: &Value,
) -> Result<(Vec<(i32, i32, i32, i32)>, Map<String, Value>), String> {
    let spec = blend_spec(params)?;
    let mode = fill_mode(params)?;
    let mirror = edit::mirror_arg(cells, params)?;
    if mirror.is_some() && mode == FillMode::Intersect {
        return Err(
            "mirror does not take mode intersect: blend the mirrored shapes into the list instead"
                .into(),
        );
    }
    let (writes, blended, mirrored) = crate::tools::mirrored(cells, mirror, |cells| {
        let blended = crate::tools::blend_shapes(&spec)?;
        let writes = match mode {
            FillMode::Intersect => {
                let inside: HashSet<Cell> = blended.cells.iter().map(|c| c.0).collect();
                intersect_writes(cells, &inside)
            }
            FillMode::Replace => crate::tools::blend_writes(cells, &blended, false, false),
            FillMode::Empty => crate::tools::blend_writes(cells, &blended, true, false),
            FillMode::Paint => crate::tools::blend_writes(cells, &blended, false, true),
        };
        Ok((writes, blended))
    })?;
    let mut reply = Map::new();
    reply.insert("cells".into(), json!(blended.cells.len()));
    reply.insert("shapes".into(), json!(spec.shapes.len()));
    reply.insert("evaluated".into(), json!(blended.evaluated));
    if let Some(first) = blended.cells.first() {
        let mut min = first.0;
        let mut max = first.0;
        for &((x, y, z), _) in &blended.cells {
            min = (min.0.min(x), min.1.min(y), min.2.min(z));
            max = (max.0.max(x), max.1.max(y), max.2.max(z));
        }
        reply.insert(
            "bounds".into(),
            json!({"min": [min.0, min.1, min.2], "max": [max.0, max.1, max.2]}),
        );
    }
    if mirror.is_some() {
        reply.insert("mirrored".into(), json!(mirrored));
    }
    Ok((writes, reply))
}

/// MCP `blend_shapes` arguments as a spec.
pub(crate) fn blend_spec(params: &Value) -> Result<BlendSpec, String> {
    let list = params.get("shapes").and_then(Value::as_array).ok_or(
        "shapes must be a list of shapes such as {shape: \"sphere\", center: [x, y, z], radius}",
    )?;
    if list.len() > crate::tools::MAX_BLEND_SHAPES {
        return Err(format!(
            "shapes takes at most {} shapes",
            crate::tools::MAX_BLEND_SHAPES
        ));
    }
    // k, grow, and seam are in their schema ranges (ops::check_arguments).
    let k = opt_f64(params, "k")?.unwrap_or(3.0);
    let palette = match params.get("palette") {
        None | Some(Value::Null) => None,
        Some(_) => Some(slot(params, "palette")?),
    };
    let shapes = list
        .iter()
        .enumerate()
        .map(|(index, item)| shape(item, k, palette).map_err(|err| format!("shape {index}: {err}")))
        .collect::<Result<Vec<_>, _>>()?;
    let grow = opt_f64(params, "grow")?.unwrap_or(0.0);
    let colors = match opt_str(params, "colors")?.as_deref() {
        None | Some("nearest") => BlendColors::Nearest,
        Some("last") => BlendColors::Last,
        Some(other) => return Err(format!("colors must be nearest or last, not `{other}`")),
    };
    let seam = opt_f64(params, "seam")?.unwrap_or(0.0);
    let seed = match params.get("seed") {
        None | Some(Value::Null) => 0,
        Some(found) => found
            .as_u64()
            .ok_or("seed must be a whole number of at least 0")?,
    };
    let corners = ["x0", "y0", "z0", "x1", "y1", "z1"];
    let given = corners.iter().filter(|k| params.get(**k).is_some()).count();
    let bounds = match given {
        0 => None,
        6 => {
            let (x0, y0, z0, x1, y1, z1) = super::box_corners(params)?;
            Some(((x0, y0, z0), (x1, y1, z1)))
        }
        _ => return Err("pass all of x0, y0, z0, x1, y1, z1 for a box, or none".into()),
    };
    Ok(BlendSpec {
        shapes,
        grow,
        colors,
        seam,
        seed,
        bounds,
    })
}

fn slot(params: &Value, key: &str) -> Result<u8, String> {
    let value = super::req_i32(params, key)?;
    if !(0..=255).contains(&value) {
        return Err(format!("{key} must be 0..255, not {value}"));
    }
    Ok(value as u8)
}

/// `[x, y, z]` or `{x, y, z}`.
fn point(value: &Value, key: &str) -> Result<[f64; 3], String> {
    let bad = || format!("{key} must be [x, y, z]");
    let found: Vec<f64> = match value {
        Value::Array(values) if values.len() >= 3 => values
            .iter()
            .take(3)
            .map(Value::as_f64)
            .collect::<Option<_>>()
            .ok_or_else(bad)?,
        Value::Object(_) => ["x", "y", "z"]
            .iter()
            .map(|axis| value.get(*axis).and_then(Value::as_f64))
            .collect::<Option<_>>()
            .ok_or_else(bad)?,
        _ => return Err(bad()),
    };
    Ok([found[0], found[1], found[2]])
}

/// A number or `[a, b, c]` as three numbers.
fn triple(value: &Value, key: &str) -> Result<[f64; 3], String> {
    match value {
        Value::Number(_) => {
            let v = value
                .as_f64()
                .ok_or_else(|| format!("{key} must be a number"))?;
            Ok([v; 3])
        }
        _ => point(value, key).map_err(|_| format!("{key} must be a number or [x, y, z]")),
    }
}

fn shape(item: &Value, k: f64, palette: Option<u8>) -> Result<BlendShape, String> {
    let Some(map) = item.as_object() else {
        return Err(
            "a shape is a map such as {shape: \"sphere\", center: [x, y, z], radius}".into(),
        );
    };
    for key in map.keys() {
        if !SHAPE_KEYS.contains(&key.as_str()) {
            return Err(format!(
                "unknown key `{key}`; a shape takes {}",
                SHAPE_KEYS.join(", ")
            ));
        }
    }
    let kind = opt_str(item, "shape")?
        .ok_or("shape is required: sphere, ellipsoid, box, capsule, or torus")?;
    let op = match opt_str(item, "op")?.as_deref() {
        None => BlendOp::Union,
        Some(text) => BlendOp::parse(text)
            .ok_or_else(|| format!("op must be {}, not `{text}`", BlendOp::NAMES))?,
    };
    let k = opt_f64(item, "k")?.unwrap_or(k);
    if !(0.0..=crate::tools::MAX_BLEND_K).contains(&k) {
        return Err(format!("k must be 0..={} cells", crate::tools::MAX_BLEND_K));
    }
    let palette =
        match item.get("palette") {
            None | Some(Value::Null) => {
                if op == BlendOp::Union {
                    Some(palette.ok_or(
                        "palette is required on a union shape, or pass palette for the call",
                    )?)
                } else {
                    None
                }
            }
            Some(_) => Some(slot(item, "palette")?),
        };
    let rotate = match item.get("rotate") {
        None | Some(Value::Null) => [0.0; 3],
        Some(found) => triple(found, "rotate")?,
    };
    let center = || -> Result<[f64; 3], String> {
        point(item.get("center").ok_or("center is required")?, "center")
    };
    let radius = || opt_f64(item, "radius");
    let form = match kind.as_str() {
        "sphere" => Form::Sphere {
            center: center()?,
            radius: radius()?.ok_or("radius is required")?,
        },
        "ellipsoid" => Form::Ellipsoid {
            center: center()?,
            radii: match (item.get("radii"), radius()?) {
                (Some(radii), _) => triple(radii, "radii")?,
                (None, Some(r)) => [r; 3],
                (None, None) => return Err("radii [rx, ry, rz] is required".into()),
            },
        },
        "box" => Form::Box {
            center: center()?,
            half: triple(
                item.get("size").ok_or("size [sx, sy, sz] is required")?,
                "size",
            )?
            .map(|s| s / 2.0),
            round: opt_f64(item, "round")?.unwrap_or(0.0),
        },
        "torus" => Form::Torus {
            center: center()?,
            major: radius()?.ok_or("radius (to the middle of the ring) is required")?,
            minor: opt_f64(item, "thickness")?
                .ok_or("thickness (the ring's own radius) is required")?,
        },
        "capsule" => {
            if rotate != [0.0; 3] {
                return Err("a capsule takes no rotate: place its points".into());
            }
            let radii: Option<[f64; 2]> = match item.get("radii") {
                None | Some(Value::Null) => None,
                Some(Value::Array(pair)) if pair.len() == 2 => Some([
                    pair[0].as_f64().ok_or("radii must be [r0, r1]")?,
                    pair[1].as_f64().ok_or("radii must be [r0, r1]")?,
                ]),
                Some(_) => {
                    return Err("radii of a capsule is [r0, r1], the radius at each end".into())
                }
            };
            let points = match (item.get("points"), item.get("from"), item.get("to")) {
                (Some(Value::Array(list)), None, None) => list
                    .iter()
                    .map(|p| {
                        let at = point(p, "points")?;
                        let r = match p {
                            Value::Array(v) if v.len() >= 4 => v[3].as_f64(),
                            Value::Object(_) => p.get("radius").and_then(Value::as_f64),
                            _ => None,
                        };
                        let r = r.or(radius()?).ok_or(
                            "each point is [x, y, z, radius], or pass radius for all of them",
                        )?;
                        Ok((at, r))
                    })
                    .collect::<Result<Vec<_>, String>>()?,
                (None, Some(from), Some(to)) => {
                    let [r0, r1] = match (radii, radius()?) {
                        (Some(pair), _) => pair,
                        (None, Some(r)) => [r, r],
                        (None, None) => return Err("radius or radii [r0, r1] is required".into()),
                    };
                    vec![(point(from, "from")?, r0), (point(to, "to")?, r1)]
                }
                _ => {
                    return Err(
                        "a capsule takes from and to (with radius or radii), or points".into(),
                    )
                }
            };
            Form::Capsule { points }
        }
        other => {
            return Err(format!(
                "shape must be sphere, ellipsoid, box, capsule, or torus, not `{other}`"
            ))
        }
    };
    Ok(BlendShape {
        form,
        rotate,
        op,
        k,
        palette,
    })
}

/// Shape points of `blend_shapes` moved by `by` cells: each shape's
/// `center`, `from`, `to`, and `points`. Called by `space` world.
pub(crate) fn shift_shapes(map: &mut Map<String, Value>, by: [i32; 3]) -> Result<(), String> {
    let Some(Value::Array(shapes)) = map.get_mut("shapes") else {
        return Ok(());
    };
    for shape in shapes {
        let Some(fields) = shape.as_object_mut() else {
            continue;
        };
        for key in ["center", "from", "to"] {
            if let Some(found) = fields.get_mut(key) {
                shift(found, by)?;
            }
        }
        if let Some(Value::Array(points)) = fields.get_mut("points") {
            for p in points {
                shift(p, by)?;
            }
        }
    }
    Ok(())
}

fn shift(value: &mut Value, by: [i32; 3]) -> Result<(), String> {
    let moved = |v: &mut Value, d: i32| -> Result<(), String> {
        let n = v.as_f64().ok_or("shape points must be numbers")?;
        *v = json!(n + f64::from(d));
        Ok(())
    };
    match value {
        Value::Array(values) => {
            for (axis, v) in values.iter_mut().take(3).enumerate() {
                moved(v, by[axis])?;
            }
        }
        Value::Object(fields) => {
            for (axis, name) in ["x", "y", "z"].iter().enumerate() {
                if let Some(v) = fields.get_mut(*name) {
                    moved(v, by[axis])?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
