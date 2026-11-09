//! MCP handlers of the whole-object edits: `split_object`.
//!
//! Rhai records `split_object` as a document call and runs it through this
//! same handler after its voxels are written.

use serde_json::{json, Value};

use super::{opt_bool, opt_str, slot_list};
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

#[cfg(test)]
#[path = "../../tests/edit.rs"]
mod tests;
