//! MCP handlers of the whole-object edits: `split_object`,
//! `clear_object`, and `recolor`.
//!
//! Rhai records `split_object` and `clear_object(object)` as document calls and runs it through this
//! same handler after its voxels are written; its `recolor` runs
//! [`recolor_writes`] on the object being written.

use std::collections::HashMap;

use serde_json::{json, Value};

use super::{as_i32, box_corners, opt_bool, opt_str, slot_list};
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
pub(crate) fn recolor_writes(
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
    let (writes, matched) = recolor_writes(&session.active().cells, params)?;
    let changed = session.write_cells(&writes)?;
    Ok(json!({"matched": matched, "changed": changed, "voxels": session.voxel_count()}))
}

#[cfg(test)]
#[path = "../../tests/edit.rs"]
mod tests;
