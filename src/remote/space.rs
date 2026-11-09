//! `space`: the coordinates of an editing tool in the object's local cells
//! (the default) or in world cells.
//!
//! With `space: "world"` every coordinate argument of the tool is a world
//! cell, and the handler subtracts the object's position before the tool
//! runs, so a caller does not recompute boxes per part after a save and
//! reopen moved the objects. The coordinates in the reply (`x`, `y`, `z`,
//! `cells`, `bounds`, `origin`) come back in world cells too. Rhai scripts
//! call [`to_local`] with the position the object had when the run started.

use serde_json::{json, Value};

use crate::state::Cell;

/// The methods that take `space`.
pub(crate) const WORLD_METHODS: &[&str] = &[
    "voxel.get",
    "voxel.set",
    "voxel.set_many",
    "voxel.get_box",
    "voxel.fill",
    "voxel.ellipsoid",
    "voxel.rounded",
    "voxel.tube",
    "voxel.carve",
    "voxel.smooth",
    "voxel.recolor",
    "voxel.surface_stroke",
    "voxel.loft",
    "voxel.brush",
    "voxel.carve_from_views",
    "voxel.project_paint",
    "voxel.decal",
    "object.split",
];

/// The schema property of `space`.
pub(crate) fn space_prop() -> Value {
    json!({"type": "string", "enum": ["local", "world"], "description": "local (default): coordinates are the active object's local cells. world: they are world cells and the object's position is subtracted first, so the same box hits the same place whatever object it lands in; coordinates in the reply are world cells too."})
}

/// Whether `params` asks for world space. `space` is `local` or `world`.
pub(crate) fn is_world(params: &Value) -> Result<bool, String> {
    match params.get("space") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::String(text)) if text == "local" => Ok(false),
        Some(Value::String(text)) if text == "world" => Ok(true),
        Some(other) => Err(format!("space must be local or world, not {other}")),
    }
}

/// The arguments of `method` with world cells turned into local cells of an
/// object at `offset`, and `space` removed. Corners `x0..z1`, cells `x`,
/// `y`, `z`, `points` and `cells` (lists or maps; a list's first three
/// numbers), `origin`, `boxes`, `exclude`, a decal's `point` and `ray`
/// origin, and a `mirror`'s `at` move;
/// everything else stays.
pub(crate) fn to_local(method: &str, params: &Value, offset: Cell) -> Result<Value, String> {
    if !WORLD_METHODS.contains(&method) {
        return Err("space world is not taken by this tool; pass local cells".into());
    }
    let mut out = params.clone();
    let Some(map) = out.as_object_mut() else {
        return Ok(out);
    };
    map.remove("space");
    let by = [offset.0, offset.1, offset.2];
    for (index, axis) in ["x", "y", "z"].iter().enumerate() {
        for key in [format!("{axis}0"), format!("{axis}1"), axis.to_string()] {
            if let Some(value) = map.get_mut(&key) {
                shift_number(value, -by[index], &key)?;
            }
        }
    }
    for key in ["points", "cells"] {
        if let Some(Value::Array(items)) = map.get_mut(key) {
            for item in items {
                shift_point(item, by, -1, key)?;
            }
        }
    }
    if let Some(origin @ Value::Array(_)) = map.get_mut("origin") {
        shift_point(origin, by, -1, "origin")?;
    }
    // A decal's point and its ray's origin.
    if let Some(point @ Value::Array(_)) = map.get_mut("point") {
        shift_point(point, by, -1, "point")?;
    }
    if let Some(Value::Object(ray)) = map.get_mut("ray") {
        if let Some(origin) = ray.get_mut("origin") {
            shift_point(origin, by, -1, "ray origin")?;
        }
    }
    // A mirror plane given at a world coordinate.
    if let Some(Value::Object(mirror)) = map.get_mut("mirror") {
        let axis = match mirror.get("axis").and_then(Value::as_str) {
            Some("y" | "Y") => 1,
            Some("z" | "Z") => 2,
            _ => 0,
        };
        if let Some(at) = mirror.get("at").and_then(Value::as_f64) {
            mirror.insert("at".into(), json!(at - f64::from(by[axis])));
        }
    }
    for key in ["boxes", "exclude"] {
        match map.get_mut(key) {
            Some(Value::Array(items)) if items.first().is_some_and(Value::is_number) => {
                shift_box(items, by, key)?;
            }
            Some(Value::Array(items)) => {
                for item in items {
                    if let Value::Array(corners) = item {
                        shift_box(corners, by, key)?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// The reply of a tool run in local cells of an object at `offset`, with
/// `x`, `y`, `z`, `cells`, `bounds`, `origin`, and `centre` in world cells, and
/// `space` stated, and `position` unless the reply has one.
pub(crate) fn to_world_reply(reply: &mut Value, offset: Cell) {
    let by = [offset.0, offset.1, offset.2];
    let Some(map) = reply.as_object_mut() else {
        return;
    };
    for (index, key) in ["x", "y", "z"].iter().enumerate() {
        if let Some(value) = map.get_mut(*key) {
            let _ = shift_number(value, by[index], key);
        }
    }
    if let Some(Value::Array(items)) = map.get_mut("cells") {
        for item in items {
            let _ = shift_point(item, by, 1, "cells");
        }
    }
    if let Some(Value::Object(bounds)) = map.get_mut("bounds") {
        for key in ["min", "max"] {
            if let Some(corner) = bounds.get_mut(key) {
                let _ = shift_point(corner, by, 1, key);
            }
        }
    }
    for key in ["origin", "centre"] {
        if let Some(point @ Value::Array(_)) = map.get_mut(key) {
            let _ = shift_point(point, by, 1, key);
        }
    }
    map.insert("space".into(), json!("world"));
    map.entry("position").or_insert(json!(by));
}

fn shift_number(value: &mut Value, by: i32, key: &str) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    *value = if let Some(int) = value.as_i64() {
        json!(int + i64::from(by))
    } else if let Some(float) = value.as_f64() {
        json!(float + f64::from(by))
    } else {
        return Err(format!("{key} must be a number"));
    };
    Ok(())
}

/// A point `[x, y, z, …]` or `{x, y, z, …}` moved by `sign * by`.
fn shift_point(item: &mut Value, by: [i32; 3], sign: i32, key: &str) -> Result<(), String> {
    match item {
        Value::Array(values) => {
            for (axis, value) in values.iter_mut().take(3).enumerate() {
                shift_number(value, sign * by[axis], key)?;
            }
        }
        Value::Object(fields) => {
            for (axis, name) in ["x", "y", "z"].iter().enumerate() {
                if let Some(value) = fields.get_mut(*name) {
                    shift_number(value, sign * by[axis], key)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// A box `[x0, y0, z0, x1, y1, z1]` moved into local cells.
fn shift_box(corners: &mut [Value], by: [i32; 3], key: &str) -> Result<(), String> {
    for (index, value) in corners.iter_mut().take(6).enumerate() {
        shift_number(value, -by[index % 3], key)?;
    }
    Ok(())
}
