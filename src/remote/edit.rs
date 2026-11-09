//! MCP handlers of the whole-object edits: `split_object`,
//! `clear_object`, `recolor`, `surface_stroke`, and `components`, and the
//! `mirror` argument of the paint and edit tools.
//!
//! Rhai records `split_object` and `clear_object(object)` as document calls and runs it through this
//! same handler after its voxels are written; its `recolor` runs
//! [`recolor_writes`] on the object being written.

use std::collections::HashMap;

use serde_json::{json, Value};

use super::{as_i32, box_corners, opt_bool, opt_f64, opt_str, slot_list};
use crate::state::{Cell, Session, SplitInto, SplitSpec};

/// Most boxes one `split_object` call takes.
pub(crate) const MAX_SPLIT_BOXES: usize = 64;

/// An object named by `key`: an index or a name (the first object with it).
/// `None` when the key is absent.
pub(crate) fn object_arg(
    session: &Session,
    params: &Value,
    key: &str,
) -> Result<Option<usize>, String> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(name)) => session
            .objects
            .iter()
            .position(|object| &object.name == name)
            .map(Some)
            .ok_or_else(|| format!("there is no object named `{name}`")),
        Some(found) => {
            let index = found
                .as_i64()
                .ok_or_else(|| format!("{key} must be an object index or name"))?;
            if index < 0 || index as usize >= session.objects.len() {
                return Err(format!(
                    "object index {index} is out of range ({} objects)",
                    session.objects.len()
                ));
            }
            Ok(Some(index as usize))
        }
    }
}

/// `boxes`: one inclusive box `[x0, y0, z0, x1, y1, z1]` or a list of them,
/// corners in either order.
pub(crate) fn boxes_arg(params: &Value, key: &str) -> Result<Vec<(Cell, Cell)>, String> {
    let bad = || format!("{key} must be [x0, y0, z0, x1, y1, z1] or a list of such boxes");
    let list = match params.get(key) {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(items)) if items.first().is_some_and(Value::is_number) => {
            vec![params[key].clone()]
        }
        Some(Value::Array(items)) => items.clone(),
        Some(_) => return Err(bad()),
    };
    if list.len() > MAX_SPLIT_BOXES {
        return Err(format!("{key} holds at most {MAX_SPLIT_BOXES} boxes"));
    }
    list.iter()
        .map(|item| {
            let numbers = item
                .as_array()
                .filter(|corners| corners.len() == 6)
                .ok_or_else(bad)?
                .iter()
                .map(|value| {
                    value
                        .as_i64()
                        .and_then(|number| i32::try_from(number).ok())
                        .ok_or_else(bad)
                })
                .collect::<Result<Vec<i32>, String>>()?;
            Ok((
                (
                    numbers[0].min(numbers[3]),
                    numbers[1].min(numbers[4]),
                    numbers[2].min(numbers[5]),
                ),
                (
                    numbers[0].max(numbers[3]),
                    numbers[1].max(numbers[4]),
                    numbers[2].max(numbers[5]),
                ),
            ))
        })
        .collect()
}

/// MCP `split_object`: move or copy cells of one object into another.
pub(super) fn split_object(session: &mut Session, params: &Value) -> Result<Value, String> {
    let source = object_arg(session, params, "index")?.unwrap_or(session.selected);
    let boxes = boxes_arg(params, "boxes")?;
    let slots = match params.get("slots") {
        None | Some(Value::Null) => None,
        Some(_) => Some(slot_list(params, "slots")?),
    };
    let into = match (
        object_arg(session, params, "into")?,
        opt_str(params, "name")?,
    ) {
        (Some(_), Some(_)) => return Err("pass into or name, not both".into()),
        (Some(target), None) => SplitInto::Object(target),
        (None, name) => {
            SplitInto::New(name.unwrap_or_else(|| format!("{}_part", session.objects[source].name)))
        }
    };
    let spec = SplitSpec {
        source,
        boxes,
        slots,
        cells: None,
        invert: opt_bool(params, "invert")?.unwrap_or(false),
        copy: opt_bool(params, "copy")?.unwrap_or(false),
        into,
    };
    let split = session.split_object(&spec)?;
    if opt_bool(params, "makeActive")?.unwrap_or(false) {
        session.select_object(split.target);
    }
    let target = &session.objects[split.target];
    let position = target.translation;
    Ok(json!({
        "source": source,
        "target": split.target,
        "name": target.name,
        "created": split.created,
        "cells": split.cells,
        "replaced": split.replaced,
        "copied": spec.copy,
        "position": [position.0, position.1, position.2],
        "sourceVoxels": session.objects[source].cells.len(),
        "targetVoxels": session.objects[split.target].cells.len(),
        "bounds": super::bounds_json(session.objects[split.target].cells.keys().copied()),
        "activeObject": session.selected,
    }))
}

/// MCP `clear_object`: empty an object (by index or name; default the
/// active one) with no box limit, one undo step.
pub(super) fn clear_object(session: &mut Session, params: &Value) -> Result<Value, String> {
    let index = object_arg(session, params, "index")?.unwrap_or(session.selected);
    let removed = session.clear_object(index)?;
    Ok(json!({
        "index": index,
        "name": session.objects[index].name,
        "removed": removed,
        "voxels": session.voxel_count(),
    }))
}

/// The slot map of `recolor`: `from` and `to`, or `map`, an object of
/// `"from": to` or a list of `[from, to]` pairs. A slot maps once; `to` 0
/// erases.
pub(crate) fn recolor_map(params: &Value) -> Result<[Option<u8>; 256], String> {
    let slot = |value: &Value, what: &str, low: i32| -> Result<u8, String> {
        as_i32(value)
            .filter(|slot| (low..=255).contains(slot))
            .map(|slot| slot as u8)
            .ok_or_else(|| format!("{what} must be a palette index {low}..255"))
    };
    let mut pairs = Vec::new();
    match (params.get("from"), params.get("map")) {
        (Some(_), Some(_)) => return Err("pass from and to, or map, not both".into()),
        (Some(from), None) => {
            let to = params.get("to").ok_or("to is required with from")?;
            pairs.push((slot(from, "from", 1)?, slot(to, "to", 0)?));
        }
        (None, Some(Value::Object(map))) => {
            for (key, to) in map {
                let from: i32 = key
                    .trim()
                    .parse()
                    .map_err(|_| format!("map key `{key}` is not a palette index"))?;
                pairs.push((
                    slot(&json!(from), "a map key", 1)?,
                    slot(to, "a map value", 0)?,
                ));
            }
        }
        (None, Some(Value::Array(items))) => {
            for item in items {
                let pair = item
                    .as_array()
                    .filter(|pair| pair.len() == 2)
                    .ok_or("map must be {\"from\": to} or a list of [from, to]")?;
                pairs.push((slot(&pair[0], "from", 1)?, slot(&pair[1], "to", 0)?));
            }
        }
        (None, Some(_)) => return Err("map must be {\"from\": to} or a list of [from, to]".into()),
        (None, None) => return Err("pass from and to, or map".into()),
    }
    if pairs.is_empty() {
        return Err("map is empty".into());
    }
    let mut table = [None; 256];
    for (from, to) in pairs {
        if table[from as usize].is_some_and(|before| before != to) {
            return Err(format!("slot {from} is mapped twice"));
        }
        table[from as usize] = Some(to);
    }
    Ok(table)
}

/// The writes of `recolor` against `cells`: each solid cell whose slot is
/// in the map takes its new slot, inside the inclusive box `x0..z1` when
/// one is given. No shape changes unless a slot maps to 0. Also returns how
/// many cells matched.
/// With `mirror` the box is mirrored too; `matched` counts the first side,
/// and the last number is the cells only the mirrored side wrote.
pub(crate) fn recolor_writes(
    cells: &HashMap<Cell, u8>,
    params: &Value,
) -> Result<(Vec<(i32, i32, i32, i32)>, usize, usize), String> {
    let mirror = mirror_arg(cells, params)?;
    crate::tools::mirrored(cells, mirror, |cells| recolor_pass(cells, params))
}

fn recolor_pass(
    cells: &HashMap<Cell, u8>,
    params: &Value,
) -> Result<(Vec<(i32, i32, i32, i32)>, usize), String> {
    let table = recolor_map(params)?;
    let corners = ["x0", "y0", "z0", "x1", "y1", "z1"];
    let given = corners
        .iter()
        .filter(|key| params.get(**key).is_some())
        .count();
    let region = match given {
        0 => None,
        6 => Some(box_corners(params)?),
        _ => {
            return Err(
                "pass all of x0, y0, z0, x1, y1, z1 for a box, or none for the whole object".into(),
            )
        }
    };
    let mut matched = 0;
    let mut writes = Vec::new();
    for (&(x, y, z), &slot) in cells {
        if let Some((x0, y0, z0, x1, y1, z1)) = region {
            if !((x0..=x1).contains(&x) && (y0..=y1).contains(&y) && (z0..=z1).contains(&z)) {
                continue;
            }
        }
        if let Some(to) = table[slot as usize] {
            matched += 1;
            if to != slot {
                writes.push((x, y, z, i32::from(to)));
            }
        }
    }
    Ok((writes, matched))
}

/// MCP `recolor`: replace palette slots in the active object, one undo step.
pub(super) fn recolor(session: &mut Session, params: &Value) -> Result<Value, String> {
    let (writes, matched, mirrored) = recolor_writes(&session.active().cells, params)?;
    let changed = session.write_cells(&writes)?;
    let mut reply =
        json!({"matched": matched, "changed": changed, "voxels": session.voxel_count()});
    if params
        .get("mirror")
        .is_some_and(|m| !m.is_null() && m != &json!(false))
    {
        reply["mirrored"] = json!(mirrored);
    }
    Ok(reply)
}

/// The methods that take `mirror`.
pub(crate) const MIRROR_METHODS: &[&str] = &[
    "voxel.set_many",
    "voxel.fill",
    "voxel.ellipsoid",
    "voxel.rounded",
    "voxel.tube",
    "voxel.recolor",
    "voxel.surface_stroke",
    "voxel.project_paint",
    "voxel.decal",
];

/// The schema property of `mirror`.
pub(crate) fn mirror_prop() -> Value {
    json!({"type": ["boolean", "string", "object"], "description": "Do it on both sides of a plane in one call: true (x), an axis x, y, or z, or {axis?: x, at?: the plane's coordinate in local cells (world with space world), a multiple of 0.5; cell c spans c..c+1, so at 16 swaps cells 15 and 16}. Without at the plane is the middle of the object's bounds along the axis. The tool runs again on the object reflected across the plane and its writes are reflected back, so the far side's own surface is painted and a picture comes out mirrored (a left eye for a right one); a cell both sides write keeps the first side's slot. The reply adds mirrored, the cells only the far side wrote."})
}

/// `mirror` of the paint and edit tools: `true` (x about the object's
/// centre), an axis `"x"`, `"y"`, or `"z"` (about the centre), or
/// `{axis?, at?}` with the plane's coordinate `at` in local cells (a
/// multiple of 0.5; cell c spans c..c+1, so at 16 swaps cells 15 and 16).
/// The centre is the middle of the object's bounds along the axis.
pub(crate) fn mirror_arg(
    cells: &HashMap<Cell, u8>,
    params: &Value,
) -> Result<Option<crate::tools::Mirror>, String> {
    let axis_of = |text: &str| match text {
        "x" | "X" => Ok(0),
        "y" | "Y" => Ok(1),
        "z" | "Z" => Ok(2),
        other => Err(format!("mirror axis must be x, y, or z, not `{other}`")),
    };
    let (axis, at) = match params.get("mirror") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => return Ok(None),
        Some(Value::Bool(true)) => (0, None),
        Some(Value::String(text)) => (axis_of(text)?, None),
        Some(Value::Object(map)) => {
            let axis = match map.get("axis") {
                None | Some(Value::Null) => 0,
                Some(Value::String(text)) => axis_of(text)?,
                Some(_) => return Err("mirror axis must be x, y, or z".into()),
            };
            let at = match map.get("at") {
                None | Some(Value::Null) => None,
                Some(found) => Some(found.as_f64().ok_or("mirror at must be a number")?),
            };
            for key in map.keys() {
                if key != "axis" && key != "at" {
                    return Err(format!("mirror takes axis and at, not `{key}`"));
                }
            }
            (axis, at)
        }
        Some(_) => return Err("mirror must be true, an axis x, y, or z, or {axis, at}".into()),
    };
    match at {
        Some(at) => crate::tools::Mirror::at(axis, at).map(Some),
        None => crate::tools::Mirror::about_centre(axis, cells.keys().copied())
            .map(Some)
            .ok_or_else(|| "mirror about the centre needs cells: pass mirror {axis, at}".into()),
    }
}

/// The stroke of MCP `surface_stroke`: `points` (`[x, y, z]` or `{x, y, z}`),
/// `curve`, `width`, `depth`, `reach`, `palette`, and `only`.
pub(crate) fn stroke_spec(params: &Value) -> Result<crate::tools::StrokeSpec, String> {
    let list = params
        .get("points")
        .and_then(Value::as_array)
        .ok_or("points must be a list of [x, y, z]")?;
    let mut points = Vec::with_capacity(list.len());
    for (index, point) in list.iter().enumerate() {
        let bad = || format!("point {index} must be [x, y, z] or {{x, y, z}}");
        let at: Vec<f64> = match point {
            Value::Array(values) if values.len() == 3 => {
                values.iter().map(Value::as_f64).collect::<Option<_>>()
            }
            Value::Object(_) => ["x", "y", "z"]
                .iter()
                .map(|key| point.get(*key).and_then(Value::as_f64))
                .collect::<Option<_>>(),
            _ => None,
        }
        .ok_or_else(bad)?;
        points.push([at[0], at[1], at[2]]);
    }
    let smooth = match opt_str(params, "curve")?.as_deref() {
        None | Some("polyline") => false,
        Some("smooth") => true,
        Some(other) => return Err(format!("curve must be polyline or smooth, not `{other}`")),
    };
    let palette = params
        .get("palette")
        .ok_or("palette is required: the slot painted, or 0 for a groove")?;
    let slot = as_i32(palette)
        .filter(|slot| (0..=255).contains(slot))
        .ok_or("palette must be 0..255")? as u8;
    let depth = opt_f64(params, "depth")?.unwrap_or(1.0);
    if depth.fract() != 0.0 || depth < 1.0 {
        return Err(format!(
            "depth must be a whole number 1..={}",
            crate::tools::MAX_STROKE_DEPTH
        ));
    }
    Ok(crate::tools::StrokeSpec {
        points,
        smooth,
        width: opt_f64(params, "width")?.unwrap_or(1.0),
        depth: depth.min(f64::from(u32::MAX)) as u32,
        reach: opt_f64(params, "reach")?.unwrap_or(8.0),
        slot,
        only: match params.get("only") {
            None | Some(Value::Null) => None,
            Some(_) => Some(slot_list(params, "only")?),
        },
    })
}

/// The writes of `surface_stroke` against `cells` and its counts. Rhai
/// scripts call this with their options map.
/// With `mirror` the stroke runs again on the far side, mirrored.
pub(crate) fn stroke_writes(
    cells: &HashMap<Cell, u8>,
    params: &Value,
) -> Result<(Vec<(i32, i32, i32, i32)>, Value), String> {
    let spec = stroke_spec(params)?;
    let mirror = mirror_arg(cells, params)?;
    let (writes, mut counts, mirrored) =
        crate::tools::mirrored(cells, mirror, |cells| stroke_pass(cells, &spec))?;
    if mirror.is_some() {
        counts["cells"] = json!(writes.len());
        counts["mirrored"] = json!(mirrored);
    }
    Ok((writes, counts))
}

fn stroke_pass(
    cells: &HashMap<Cell, u8>,
    spec: &crate::tools::StrokeSpec,
) -> Result<(Vec<(i32, i32, i32, i32)>, Value), String> {
    let stroked = crate::tools::surface_stroke(cells, spec)?;
    let counts = json!({
        "cells": stroked.writes.len(),
        "samples": stroked.samples,
        "missed": stroked.missed,
        "snap": (stroked.snap * 100.0).round() / 100.0,
        "length": (stroked.length * 100.0).round() / 100.0,
    });
    Ok((stroked.writes, counts))
}

/// MCP `surface_stroke`: paint (or groove) a stroke that snaps to the
/// active object's surface, one undo step.
pub(super) fn surface_stroke(session: &mut Session, params: &Value) -> Result<Value, String> {
    let (writes, mut reply) = stroke_writes(&session.active().cells, params)?;
    let changed = session.write_cells(&writes)?;
    reply["changed"] = json!(changed);
    reply["voxels"] = json!(session.voxel_count());
    Ok(reply)
}

/// A point `[x, y, z]` or `{x, y, z}` under `key`.
fn opt_point(params: &Value, key: &str) -> Result<Option<[f64; 3]>, String> {
    let bad = || format!("{key} must be [x, y, z] or {{x, y, z}}");
    let at: Vec<f64> = match params.get(key) {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Array(values)) if values.len() == 3 => values
            .iter()
            .map(Value::as_f64)
            .collect::<Option<_>>()
            .ok_or_else(bad)?,
        Some(point @ Value::Object(_)) => ["x", "y", "z"]
            .iter()
            .map(|axis| point.get(*axis).and_then(Value::as_f64))
            .collect::<Option<_>>()
            .ok_or_else(bad)?,
        Some(_) => return Err(bad()),
    };
    if at.iter().any(|v| !v.is_finite()) {
        return Err(bad());
    }
    Ok(Some([at[0], at[1], at[2]]))
}

/// The spec of MCP `components`: `op`, `connectivity`, `keep`, `minCells`,
/// `seed`, `anchor`, `reach`, `radius`, and `offset`.
pub(crate) fn components_spec(params: &Value) -> Result<crate::tools::ComponentSpec, String> {
    use crate::tools::ComponentOp;
    let count = |key: &str, default: Option<usize>| -> Result<usize, String> {
        match params.get(key) {
            None | Some(Value::Null) => default.ok_or_else(|| format!("{key} is required")),
            Some(value) => value
                .as_u64()
                .map(|v| v.min(u64::from(u32::MAX)) as usize)
                .ok_or_else(|| format!("{key} must be a whole number")),
        }
    };
    let op = match opt_str(params, "op")?.as_deref() {
        None | Some("list") => ComponentOp::List,
        Some("keep_largest") => ComponentOp::KeepLargest {
            keep: count("keep", Some(1))?,
        },
        Some("remove_small") => ComponentOp::RemoveSmall {
            min: count("minCells", None)?,
        },
        Some("delete") => ComponentOp::Delete,
        Some("move") => {
            let offset = super::opt_ivec3(params, "offset")?
                .ok_or("offset [dx, dy, dz] is required with op move")?;
            ComponentOp::Move {
                offset: (offset[0], offset[1], offset[2]),
            }
        }
        Some("split") => ComponentOp::Split,
        Some(other) => {
            return Err(format!(
                "op must be list, keep_largest, remove_small, delete, move, or split, not `{other}`"
            ))
        }
    };
    let connectivity = match params.get("connectivity") {
        None | Some(Value::Null) => 26,
        Some(value) => match value.as_u64() {
            Some(n @ (6 | 18 | 26)) => n as u8,
            _ => return Err("connectivity must be 6, 18, or 26".into()),
        },
    };
    Ok(crate::tools::ComponentSpec {
        op,
        connectivity,
        seed: opt_point(params, "seed")?,
        anchor: opt_point(params, "anchor")?,
        reach: opt_f64(params, "reach")?.unwrap_or(8.0),
        radius: opt_f64(params, "radius")?,
    })
}

/// The run of `components` on `cells`: the tool's result and the reply's
/// counts. Rhai scripts call this with their options map.
pub(crate) fn components_run(
    cells: &HashMap<Cell, u8>,
    params: &Value,
) -> Result<(crate::tools::Components, Value), String> {
    let spec = components_spec(params)?;
    let found = crate::tools::components(cells, &spec)?;
    let point = |c: Cell| json!([c.0, c.1, c.2]);
    let listed: Vec<Value> = found
        .components
        .iter()
        .take(crate::tools::LISTED_COMPONENTS)
        .map(|part| {
            json!({
                "cells": part.cells,
                "min": point(part.min),
                "max": point(part.max),
                "seed": point(part.seed),
            })
        })
        .collect();
    let mut reply = json!({
        "count": found.components.len(),
        "largest": found.components.first().map_or(0, |part| part.cells),
        "components": listed,
        "selected": found.selected.len(),
        "connectivity": spec.connectivity,
    });
    if let Some(seed) = found.seed {
        reply["seed"] = point(seed);
    }
    if let Some(anchor) = found.anchor {
        reply["anchor"] = point(anchor);
        reply["cut"] = json!(found.cut);
    }
    Ok((found, reply))
}

/// MCP `components`: list, clean up, or select the connected parts of the
/// active object (or `index`, for op split), one undo step.
pub(super) fn components(session: &mut Session, params: &Value) -> Result<Value, String> {
    let split = opt_str(params, "op")?.as_deref() == Some("split");
    let source = object_arg(session, params, "index")?.unwrap_or(session.selected);
    if !split && source != session.selected {
        return Err("index is taken by op split; the other ops edit the active object".into());
    }
    let (found, mut reply) = components_run(&session.objects[source].cells, params)?;
    if !split {
        let changed = session.write_cells(&found.writes)?;
        reply["changed"] = json!(changed);
        reply["voxels"] = json!(session.voxel_count());
        return Ok(reply);
    }
    if found.selected.is_empty() {
        return Err("the seed selects no cell".into());
    }
    let into = match (
        object_arg(session, params, "into")?,
        opt_str(params, "name")?,
    ) {
        (Some(_), Some(_)) => return Err("pass into or name, not both".into()),
        (Some(target), None) => SplitInto::Object(target),
        (None, name) => {
            SplitInto::New(name.unwrap_or_else(|| format!("{}_part", session.objects[source].name)))
        }
    };
    let spec = SplitSpec {
        source,
        boxes: Vec::new(),
        slots: None,
        cells: Some(found.selected.iter().copied().collect()),
        invert: false,
        copy: opt_bool(params, "copy")?.unwrap_or(false),
        into,
    };
    let done = session.split_object(&spec)?;
    if opt_bool(params, "makeActive")?.unwrap_or(false) {
        session.select_object(done.target);
    }
    let target = &session.objects[done.target];
    reply["target"] = json!(done.target);
    reply["name"] = json!(target.name);
    reply["created"] = json!(done.created);
    reply["sourceVoxels"] = json!(session.objects[source].cells.len());
    reply["targetVoxels"] = json!(session.objects[done.target].cells.len());
    reply["activeObject"] = json!(session.selected);
    Ok(reply)
}

#[cfg(test)]
#[path = "../../tests/edit.rs"]
mod tests;
