//! MCP `foliage`: a clustered tree crown, its arguments, and its reply.
//! The MCP handler, the Rhai `foliage`, and the editor's Foliage section
//! all come through [`foliage_writes`].

use std::collections::HashMap;

use serde_json::{json, Map, Value};

use super::{as_i32, exclude_boxes, opt_f64, opt_vec3, range_pair, slot_list};
use crate::state::Cell;
use crate::tools::{CrownShape, FoliageSpec, Region};

/// Most ellipsoids and boxes one crown takes.
pub(crate) const MAX_CROWN_SHAPES: usize = 64;

/// MCP `foliage` arguments against `cells` as a spec. The box is optional:
/// without it the crown's shapes and `from` cells, grown by room for the
/// clumps to bulge, bound the writes.
pub(crate) fn foliage_spec(
    cells: &HashMap<Cell, u8>,
    params: &Value,
) -> Result<FoliageSpec, String> {
    let palettes = slot_list(params, "palettes")?;
    if palettes.is_empty() {
        return Err(
            "palettes is required: the leaf slots from the darkest (shade) to the lightest (tips)"
                .into(),
        );
    }
    let mut shapes = Vec::new();
    match params.get("ellipsoids") {
        None | Some(Value::Null) => {}
        Some(Value::Array(list)) => {
            for (index, entry) in list.iter().enumerate() {
                let bad = || {
                    format!("ellipsoid {index} must be [x, y, z, radius] or [x, y, z, rx, ry, rz]")
                };
                let v: Vec<f64> = entry
                    .as_array()
                    .filter(|row| row.len() == 4 || row.len() == 6)
                    .and_then(|row| row.iter().map(Value::as_f64).collect())
                    .ok_or_else(bad)?;
                let radii = if v.len() == 4 {
                    [v[3]; 3]
                } else {
                    [v[3], v[4], v[5]]
                };
                shapes.push(CrownShape::Ellipsoid {
                    center: [v[0], v[1], v[2]],
                    radii,
                });
            }
        }
        Some(_) => return Err("ellipsoids must be a list of [x, y, z, rx, ry, rz]".into()),
    }
    match params.get("boxes") {
        None | Some(Value::Null) => {}
        Some(Value::Array(list)) => {
            for (index, entry) in list.iter().enumerate() {
                let v: Vec<i32> = entry
                    .as_array()
                    .filter(|row| row.len() == 6)
                    .and_then(|row| row.iter().map(as_i32).collect())
                    .ok_or_else(|| format!("box {index} must be [x0, y0, z0, x1, y1, z1]"))?;
                let r = Region::spanning((v[0], v[1], v[2]), (v[3], v[4], v[5]));
                shapes.push(CrownShape::Box {
                    min: r.min,
                    max: r.max,
                });
            }
        }
        Some(_) => return Err("boxes must be a list of [x0, y0, z0, x1, y1, z1]".into()),
    }
    if shapes.len() > MAX_CROWN_SHAPES {
        return Err(format!(
            "a crown takes at most {MAX_CROWN_SHAPES} ellipsoids and boxes"
        ));
    }
    let from = slot_list(params, "from")?;
    let size = range_pair(params, "size", (3.0, 6.0))?;
    let region = if ["x0", "y0", "z0", "x1", "y1", "z1"]
        .iter()
        .any(|key| params.get(*key).is_some_and(|v| !v.is_null()))
    {
        let (x0, y0, z0, x1, y1, z1) = super::box_corners(params)?;
        Region::new((x0, y0, z0), (x1, y1, z1), "foliage")?
    } else {
        let mut lo = (i32::MAX, i32::MAX, i32::MAX);
        let mut hi = (i32::MIN, i32::MIN, i32::MIN);
        let mut grow = |a: Cell, b: Cell| {
            lo = (lo.0.min(a.0), lo.1.min(a.1), lo.2.min(a.2));
            hi = (hi.0.max(b.0), hi.1.max(b.1), hi.2.max(b.2));
        };
        for shape in &shapes {
            let (a, b) = shape.bounds();
            grow(a, b);
        }
        if !from.is_empty() {
            for (cell, slot) in cells {
                if from.contains(slot) {
                    grow(*cell, *cell);
                }
            }
        }
        if lo.0 > hi.0 {
            return Err(if shapes.is_empty() && !from.is_empty() {
                "no cell of the from slots in the active object".into()
            } else {
                "the crown needs a volume: ellipsoids, boxes, or from (the slots of an existing crown)"
                    .into()
            });
        }
        let pad = (size.1.clamp(1.0, 32.0) * 0.8).ceil() as i32 + 2;
        Region::new(
            crate::tools::add(lo, (-pad, -pad, -pad)),
            crate::tools::add(hi, (pad, pad, pad)),
            "foliage",
        )?
    };
    let core = match params.get("core") {
        None | Some(Value::Null) => palettes[0],
        Some(found) => as_i32(found)
            .filter(|v| (1..=255).contains(v))
            .ok_or("core must be a palette index 1..=255")? as u8,
    };
    let sun = match opt_vec3(params, "sun")? {
        Some(v) => [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])],
        None => [-0.35, -0.45, 1.0],
    };
    let whole = |key: &str, default: i64, hi: i64| -> Result<i64, String> {
        match params.get(key) {
            None | Some(Value::Null) => Ok(default),
            Some(found) => found
                .as_i64()
                .filter(|v| (0..=hi).contains(v))
                .ok_or_else(|| format!("{key} must be a whole number 0..={hi}")),
        }
    };
    let seed = match params.get("seed") {
        None | Some(Value::Null) => 0,
        Some(found) => found
            .as_u64()
            .ok_or("seed must be a whole number of at least 0")?,
    };
    Ok(FoliageSpec {
        region,
        shapes,
        from,
        palettes,
        core,
        size,
        gap: opt_f64(params, "gap")?.unwrap_or(1.0),
        flatten: opt_f64(params, "flatten")?.unwrap_or(0.7),
        ragged: opt_f64(params, "ragged")?.unwrap_or(0.4),
        light: opt_f64(params, "light")?.unwrap_or(1.0),
        dark: opt_f64(params, "dark")?.unwrap_or(1.0),
        sun,
        masses: whole("masses", 0, i64::from(crate::tools::MAX_MASSES))? as u32,
        groove: whole("groove", 2, 8)? as i32,
        protect: slot_list(params, "protect")?,
        exclude: exclude_boxes(params)?,
        seed,
    })
}

/// The writes of MCP `foliage` against `cells` and the counts of its reply.
pub(crate) fn foliage_writes(
    cells: &HashMap<Cell, u8>,
    params: &Value,
) -> Result<(super::stone::Writes, Map<String, Value>), String> {
    let spec = foliage_spec(cells, params)?;
    let done = crate::tools::foliage_crown(cells, &spec)?;
    let mut reply = Map::new();
    reply.insert("clumps".into(), json!(done.clumps));
    reply.insert("masses".into(), json!(done.masses));
    reply.insert("added".into(), json!(done.added));
    reply.insert("recoloured".into(), json!(done.recoloured));
    reply.insert("removed".into(), json!(done.removed));
    reply.insert("core".into(), json!(done.core));
    Ok((done.writes, reply))
}
