//! The operation registry: every MCP tool, the `remote::apply` method it
//! runs, its annotation hints, and what it takes. MCP `tools/list`, the
//! tool-to-method map, the batch, `space`, and `mirror` lists, the
//! `control.methods` help, and the argument check every frontend goes
//! through (MCP, the control port, the CLI, Rhai, the panels) all read this
//! one table, so they cannot drift apart. The argument schemas are in
//! [`schema`].

pub(crate) mod schema;

use serde_json::{json, Value};

/// What a tool does to the session, for the MCP annotation hints.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Access {
    /// Reads only: read-only, idempotent.
    Read,
    /// Moves the selection or the camera: not destructive, idempotent.
    Settle,
    /// Adds something new each call: not destructive, not idempotent.
    Add,
    /// May replace or remove, and a repeat does more: destructive.
    Replace,
    /// Writes cells, settings, or a file; a repeat changes nothing more.
    Write,
}

impl Access {
    /// `readOnlyHint`, `destructiveHint`, and `idempotentHint`.
    pub(crate) fn hints(self) -> [bool; 3] {
        match self {
            Access::Read => [true, false, true],
            Access::Settle => [false, false, true],
            Access::Add => [false, false, false],
            Access::Replace => [false, true, false],
            Access::Write => [false, true, true],
        }
    }
}

/// The tool may be an item of MCP `batch`.
pub(crate) const BATCH: u8 = 1;
/// The tool takes `space` (`local` or `world`).
pub(crate) const SPACE: u8 = 2;
/// The tool takes `mirror`.
pub(crate) const MIRROR: u8 = 4;

/// One operation.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Op {
    /// The MCP tool name.
    pub tool: &'static str,
    /// The `remote::apply` method it runs.
    pub method: &'static str,
    pub access: Access,
    flags: u8,
}

impl Op {
    pub(crate) fn batch(&self) -> bool {
        self.flags & BATCH != 0
    }

    pub(crate) fn space(&self) -> bool {
        self.flags & SPACE != 0
    }

    pub(crate) fn mirror(&self) -> bool {
        self.flags & MIRROR != 0
    }
}

const fn op(tool: &'static str, method: &'static str, access: Access, flags: u8) -> Op {
    Op {
        tool,
        method,
        access,
        flags,
    }
}

use Access::*;

/// Every MCP tool, in `tools/list` order.
pub(crate) const OPS: &[Op] = &[
    op("batch", "voxel.batch", Replace, 0),
    op("new_document", "document.new", Write, 0),
    op("open_file", "document.open", Write, 0),
    op("save_file", "document.save", Write, 0),
    op("inspect_document", "document.inspect", Read, 0),
    op("get_voxel", "voxel.get", Read, SPACE),
    op("set_voxel", "voxel.set", Write, BATCH | SPACE),
    op(
        "set_voxels",
        "voxel.set_many",
        Write,
        BATCH | SPACE | MIRROR,
    ),
    op("fill_box", "voxel.fill", Write, BATCH | SPACE | MIRROR),
    op("get_voxels", "voxel.get_box", Read, SPACE),
    op("set_palette", "palette.set", Write, BATCH),
    op("set_active_object", "object.set_active", Settle, 0),
    op("add_object", "object.add", Add, 0),
    op("set_object", "object.set", Write, 0),
    op("remove_object", "object.remove", Replace, 0),
    op(
        "fill_ellipsoid",
        "voxel.ellipsoid",
        Write,
        BATCH | SPACE | MIRROR,
    ),
    op(
        "fill_rounded_box",
        "voxel.rounded",
        Write,
        BATCH | SPACE | MIRROR,
    ),
    op("fill_tube", "voxel.tube", Write, BATCH | SPACE | MIRROR),
    op("recolor", "voxel.recolor", Write, BATCH | SPACE | MIRROR),
    op("deterrace", "voxel.deterrace", Write, BATCH),
    op("split_parts", "object.split_parts", Replace, 0),
    op("reduce_palette", "voxel.reduce_palette", Write, BATCH),
    op("components", "voxel.components", Write, BATCH),
    op(
        "surface_stroke",
        "voxel.surface_stroke",
        Write,
        BATCH | SPACE | MIRROR,
    ),
    op("carve", "voxel.carve", Write, BATCH | SPACE),
    op("smooth", "voxel.smooth", Write, BATCH | SPACE),
    op("break_end", "voxel.break", Write, BATCH),
    op("scatter", "voxel.scatter", Write, BATCH),
    op("joints", "voxel.joints", Write, BATCH),
    op("column_relief", "voxel.column_relief", Write, BATCH),
    op("weather", "voxel.weather", Write, BATCH),
    op("moss", "voxel.moss", Write, BATCH),
    op("algae", "voxel.algae", Write, BATCH),
    op("vines", "voxel.vines", Write, BATCH),
    op("bark", "voxel.bark", Write, BATCH),
    op("planks", "voxel.planks", Write, BATCH),
    op("cavity_tint", "voxel.cavity_tint", Write, BATCH),
    op("radial_relief", "voxel.radial_relief", Write, BATCH | SPACE),
    op("foliage", "voxel.foliage", Write, BATCH),
    op("sweep", "voxel.sweep", Write, BATCH),
    op("trim_outline", "voxel.trim_outline", Write, BATCH),
    op("deform", "voxel.deform", Write, BATCH),
    op(
        "blend_shapes",
        "voxel.blend_shapes",
        Write,
        BATCH | SPACE | MIRROR,
    ),
    op("loft", "voxel.loft", Write, BATCH | SPACE),
    op("brush", "voxel.brush", Write, BATCH | SPACE),
    op("carve_from_views", "voxel.carve_from_views", Write, SPACE),
    op(
        "project_paint",
        "voxel.project_paint",
        Write,
        SPACE | MIRROR,
    ),
    op("paint_from_views", "voxel.paint_from_views", Write, SPACE),
    op("shell_plates", "voxel.shell_plates", Write, BATCH | SPACE),
    op("decal", "voxel.decal", Write, SPACE | MIRROR),
    op("orient_to_views", "voxel.orient_to_views", Write, 0),
    op("fit_to_views", "voxel.fit_to_views", Write, 0),
    op("optimize_to_views", "voxel.optimize_to_views", Write, 0),
    op("pixelate_views", "image.pixelate_views", Write, 0),
    op("measure_sheet", "image.measure_sheet", Read, 0),
    op("compare_silhouette", "render.compare_silhouette", Write, 0),
    op("set_marker", "marker.set", Write, 0),
    op("remove_marker", "marker.remove", Write, 0),
    op("collision_from_voxels", "marker.collision", Write, 0),
    op("list_markers", "marker.list", Read, 0),
    op("import_mesh", "mesh.import", Replace, 0),
    op("mirror_object", "object.mirror", Add, 0),
    op("clear_object", "object.clear", Write, BATCH),
    op("split_object", "object.split", Replace, SPACE),
    op("add_bone", "rig.add", Add, 0),
    op("set_bone", "rig.set", Write, 0),
    op("place_joints", "rig.joints", Write, 0),
    op("remove_bone", "rig.remove", Write, 0),
    op("bind_object", "rig.bind", Write, 0),
    op("list_bones", "rig.list", Read, 0),
    op("set_pose", "pose.set", Write, 0),
    op("clear_pose", "pose.clear", Write, 0),
    op("add_clip", "clip.add", Add, 0),
    op("set_clip_keys", "clip.keys", Write, 0),
    op("list_clips", "clip.list", Read, 0),
    op("remove_clip", "clip.remove", Write, 0),
    op("undo", "edit.undo", Replace, 0),
    op("redo", "edit.redo", Replace, 0),
    op("render", "render", Write, 0),
    op("model_view", "render.flat", Write, 0),
    op("preview", "render.preview", Write, 0),
    op("reload", "server.reload", Add, 0),
    op("set_view", "view.set", Settle, 0),
    op("screenshot", "ui.screenshot", Write, 0),
    op("screenshot_viewport", "ui.screenshot", Write, 0),
    op("screenshot_region", "ui.screenshot", Write, 0),
    op("ui_inspect", "ui.inspect", Read, 0),
    op("ui_click", "ui.click", Replace, 0),
    op("set_script", "script.set", Write, 0),
    op("get_script", "script.get", Read, 0),
    op("set_params", "script.params", Write, 0),
    op("list_params", "script.params.list", Read, 0),
    op("regenerate", "script.regenerate", Replace, 0),
    op("export_mesh", "mesh.export", Write, 0),
    op("export_variants", "mesh.export_variants", Write, 0),
    op("export_models", "mesh.export_batch", Write, 0),
    op("bake_scene", "mesh.bake_scene", Write, 0),
];

/// The operation of an MCP tool.
pub(crate) fn by_tool(tool: &str) -> Option<&'static Op> {
    OPS.iter().find(|op| op.tool == tool)
}

/// Every operation that runs `method`: one, or several for the screenshot
/// tools, which share `ui.screenshot`.
pub(crate) fn by_method(method: &str) -> impl Iterator<Item = &'static Op> + '_ {
    OPS.iter().filter(move |op| op.method == method)
}

/// Whether `method` takes `space`.
pub(crate) fn takes_space(method: &str) -> bool {
    by_method(method).any(|op| op.space())
}

/// The tools MCP `batch` can run.
pub(crate) fn batch_tools() -> impl Iterator<Item = &'static Op> {
    OPS.iter().filter(|op| op.batch())
}

/// Refuse an argument that no tool running `method` lists, so a misspelt
/// or unsupported option is an error from every frontend rather than
/// silently ignored. A method no tool runs (`ping`) and the window's `ui.*`
/// helpers, which MCP checks and fills before they reach the window, are
/// not checked here.
pub(crate) fn check_arguments(method: &str, args: &Value) -> Result<(), String> {
    let Some(args) = args.as_object() else {
        return Ok(());
    };
    if method.starts_with("ui.") {
        return Ok(());
    }
    let mut ops = by_method(method).peekable();
    let Some(first) = ops.peek().copied() else {
        return Ok(());
    };
    let mut known: Vec<&str> = Vec::new();
    for op in ops {
        if let Some(properties) = schema::properties(op.tool) {
            known.extend(properties.keys().map(String::as_str));
        }
    }
    let unknown: Vec<&str> = args
        .keys()
        .map(String::as_str)
        .filter(|key| !known.contains(key))
        .collect();
    if unknown.is_empty() {
        return Ok(());
    }
    known.sort_unstable();
    known.dedup();
    Err(format!(
        "{} has no argument {}; it takes {}",
        first.tool,
        unknown
            .iter()
            .map(|key| format!("`{key}`"))
            .collect::<Vec<_>>()
            .join(", "),
        if known.is_empty() {
            "none".to_string()
        } else {
            known.join(", ")
        }
    ))
}

/// `control.methods`: each method, its arguments (optional ones with `?`),
/// and what it does, from the tools that run it, then the control port's
/// own helpers.
pub(crate) fn methods_help() -> Value {
    let mut out = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for op in OPS {
        if seen.contains(&op.method) {
            continue;
        }
        seen.push(op.method);
        let tool = schema::tool(op.tool).expect("every op has a schema");
        let required: Vec<&str> = tool["inputSchema"]["required"]
            .as_array()
            .map(|list| list.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let params: Vec<String> = tool["inputSchema"]["properties"]
            .as_object()
            .map(|map| {
                map.keys()
                    .map(|key| {
                        if required.contains(&key.as_str()) {
                            key.clone()
                        } else {
                            format!("{key}?")
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push(json!({
            "method": op.method,
            "tool": op.tool,
            "params": params.join(", "),
            "description": tool["description"],
        }));
    }
    for (method, description) in [
        ("ping", "Replies \"pong\""),
        (
            "server.version",
            "The Froxel version of the window, which froxel mcp --connect compares with its own",
        ),
        ("control.methods", "This list"),
        (
            "ui.inspect",
            "The window's widgets and their rectangles (MCP ui_inspect)",
        ),
        ("ui.click", "Click a point of the window (MCP ui_click)"),
    ] {
        if !seen.contains(&method) {
            out.push(json!({"method": method, "params": "", "description": description}));
        }
    }
    Value::Array(out)
}

#[cfg(test)]
#[path = "../../tests/ops.rs"]
mod tests;
