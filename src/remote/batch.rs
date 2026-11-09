//! MCP `batch`: a list of editing tools applied in order as one undo step.
//!
//! The list is checked before anything runs: every item names a tool that
//! edits the active object or the palette. Tools that write files, read
//! images or meshes, or change which object is active are not items, so a
//! failed batch has nothing to put back beyond the document. The first item
//! that fails stops the run, the error names it, and `Session::transaction`
//! restores the document with no undo step added. A Rhai run has the same
//! guarantee already, so scripts have no `batch` of their own.

use serde_json::{json, Map, Value};

use crate::state::Session;

/// Most items in one batch.
pub(crate) const MAX_BATCH_ITEMS: usize = 4096;

/// The tools a batch can run, and their methods.
pub(crate) const BATCH_TOOLS: &[(&str, &str)] = &[
    ("set_voxel", "voxel.set"),
    ("set_voxels", "voxel.set_many"),
    ("set_palette", "palette.set"),
    ("fill_box", "voxel.fill"),
    ("fill_ellipsoid", "voxel.ellipsoid"),
    ("fill_rounded_box", "voxel.rounded"),
    ("fill_tube", "voxel.tube"),
    ("sweep", "voxel.sweep"),
    ("loft", "voxel.loft"),
    ("carve", "voxel.carve"),
    ("recolor", "voxel.recolor"),
    ("surface_stroke", "voxel.surface_stroke"),
    ("components", "voxel.components"),
    ("clear_object", "object.clear"),
    ("trim_outline", "voxel.trim_outline"),
    ("smooth", "voxel.smooth"),
    ("deform", "voxel.deform"),
    ("brush", "voxel.brush"),
    ("break_end", "voxel.break"),
    ("scatter", "voxel.scatter"),
    ("column_relief", "voxel.column_relief"),
    ("joints", "voxel.joints"),
    ("weather", "voxel.weather"),
    ("bark", "voxel.bark"),
    ("planks", "voxel.planks"),
    ("cavity_tint", "voxel.cavity_tint"),
    ("foliage", "voxel.foliage"),
    ("moss", "voxel.moss"),
    ("algae", "voxel.algae"),
    ("vines", "voxel.vines"),
];

/// One checked item of a batch.
pub(crate) struct BatchItem<'a> {
    pub tool: &'a str,
    pub method: &'a str,
    pub arguments: &'a Value,
}

/// The items of MCP `batch`, all checked before any of them runs. Item
/// numbers count from 0, as in the `operations` list.
pub(crate) fn batch_items(params: &Value) -> Result<Vec<BatchItem<'_>>, String> {
    let list = params
        .get("operations")
        .and_then(Value::as_array)
        .ok_or("operations is required: a list of {tool, arguments}")?;
    if list.is_empty() {
        return Err("operations must hold at least one item".into());
    }
    if list.len() > MAX_BATCH_ITEMS {
        return Err(format!(
            "operations holds {} items; a batch takes at most {MAX_BATCH_ITEMS}",
            list.len()
        ));
    }
    list.iter()
        .enumerate()
        .map(|(index, item)| {
            let map = item
                .as_object()
                .ok_or_else(|| format!("batch item {index} must be {{tool, arguments}}"))?;
            if let Some(key) = map.keys().find(|key| *key != "tool" && *key != "arguments") {
                return Err(format!(
                    "batch item {index} has no field `{key}`; it takes tool and arguments"
                ));
            }
            let tool = map
                .get("tool")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("batch item {index} needs a tool name"))?;
            let (tool, method) = BATCH_TOOLS
                .iter()
                .find(|(name, _)| *name == tool)
                .ok_or_else(|| {
                    format!(
                        "batch item {index}: `{tool}` cannot run in a batch; it takes {}",
                        batch_tool_names().join(", ")
                    )
                })?;
            let arguments = match map.get("arguments") {
                None => &Value::Null,
                Some(found) if found.is_object() => found,
                Some(_) => {
                    return Err(format!(
                        "batch item {index} ({tool}): arguments must be an object"
                    ))
                }
            };
            if *tool == "components" && arguments.get("op").and_then(Value::as_str) == Some("split")
            {
                return Err(format!(
                    "batch item {index}: components with op split changes the document; call it outside the batch"
                ));
            }
            Ok(BatchItem {
                tool,
                method,
                arguments,
            })
        })
        .collect()
}

/// The tool names a batch takes.
pub(crate) fn batch_tool_names() -> Vec<&'static str> {
    BATCH_TOOLS.iter().map(|(name, _)| *name).collect()
}

/// MCP handler: run the items in order as one undo step.
pub(super) fn apply_batch(session: &mut Session, params: &Value) -> Result<Value, String> {
    let items = batch_items(params)?;
    let results = session.transaction(|session| {
        items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                super::apply(session, item.method, item.arguments)
                    .map(|reply| short_reply(item.tool, reply))
                    .map_err(|err| {
                        format!(
                            "batch item {index} ({}) failed, so nothing was changed: {err}",
                            item.tool
                        )
                    })
            })
            .collect::<Result<Vec<Value>, String>>()
    })?;
    session.mesh_epoch = session.mesh_epoch.wrapping_add(1);
    let changed: i64 = results
        .iter()
        .map(|reply| {
            ["changed", "removed"]
                .iter()
                .find_map(|key| reply.get(*key).and_then(Value::as_i64))
                .unwrap_or(0)
        })
        .sum();
    Ok(json!({
        "items": results.len(),
        "changed": changed,
        "voxels": session.voxel_count(),
        "results": results,
    }))
}

/// An item's reply in short form: its tool and counts, a list as its length,
/// without what the call already said or the running voxel total.
fn short_reply(tool: &str, reply: Value) -> Value {
    let mut short = Map::new();
    short.insert("tool".into(), json!(tool));
    let Value::Object(reply) = reply else {
        return Value::Object(short);
    };
    for (key, value) in reply {
        if ["voxels", "x", "y", "z", "palette"].contains(&key.as_str()) {
            continue;
        }
        match value {
            Value::Array(list) => {
                short.insert(key, json!(list.len()));
            }
            Value::Object(_) | Value::Null => {}
            scalar => {
                short.insert(key, scalar);
            }
        }
    }
    Value::Object(short)
}
