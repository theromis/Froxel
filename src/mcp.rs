//! Model Context Protocol on stdin/stdout.
//!
//! `froxel mcp` edits an in-process document and has no window.
//! `froxel mcp --connect 127.0.0.1:PORT` drives an editor started with
//! `--control PORT`. `--launch` opens that editor when the port is closed.
//! Screenshots exist only on the connected window. `render` path-traces a PNG
//! in this process. The window path-traces in Render mode on the GPU.

use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::control::ControlClient;
use crate::remote::{self, apply};
use crate::state::Session;

const PROTOCOL: &str = "2025-06-18";
const SUPPORTED: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18", "2026-07-28"];

pub enum ShotRegion {
    Window,
    Viewport,
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
}

pub fn parse_shot(params: &Value) -> Result<(ShotRegion, Option<String>), String> {
    let path = remote::opt_str(params, "path")?;
    if params.get("x").is_some()
        || params.get("y").is_some()
        || params.get("width").is_some()
        || params.get("height").is_some()
    {
        let x = num(params, "x")?;
        let y = num(params, "y")?;
        let width = num(params, "width")?;
        let height = num(params, "height")?;
        return Ok((
            ShotRegion::Rect {
                x,
                y,
                width,
                height,
            },
            path,
        ));
    }
    match params
        .get("region")
        .and_then(Value::as_str)
        .unwrap_or("window")
    {
        "window" => Ok((ShotRegion::Window, path)),
        "viewport" => Ok((ShotRegion::Viewport, path)),
        other => Err(format!(
            "region must be \"window\" or \"viewport\", or pass x, y, width, height ({other})"
        )),
    }
}

pub fn run(args: &[String]) -> i32 {
    match parse_args(args) {
        Ok(None) => 0,
        Ok(Some(mode)) => serve(mode),
        Err(err) => {
            eprintln!("{err}");
            2
        }
    }
}

enum Mode {
    Headless,
    Remote { addr: String },
}

fn parse_args(args: &[String]) -> Result<Option<Mode>, String> {
    let mut connect: Option<String> = None;
    let mut launch = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_help();
                return Ok(None);
            }
            "--launch" => launch = true,
            "--connect" => {
                i += 1;
                let addr = args
                    .get(i)
                    .filter(|s| !s.starts_with('-'))
                    .cloned()
                    .ok_or_else(|| "--connect needs HOST:PORT".to_string())?;
                if addr
                    .rsplit(':')
                    .next()
                    .and_then(|p| p.parse::<u16>().ok())
                    .is_none()
                {
                    return Err(format!("--connect {addr} needs HOST:PORT"));
                }
                connect = Some(addr);
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
        i += 1;
    }
    if launch && connect.is_none() {
        return Err("--launch needs --connect HOST:PORT".into());
    }
    match connect {
        None => Ok(Some(Mode::Headless)),
        Some(addr) => {
            if let Err(err) = prepare_remote(&addr, launch) {
                return Err(err.to_string());
            }
            Ok(Some(Mode::Remote { addr }))
        }
    }
}

fn prepare_remote(addr: &str, launch: bool) -> io::Result<()> {
    if probe(addr) {
        return Ok(());
    }
    if !launch {
        return Err(io::Error::other(format!(
            "Froxel is not answering at {addr}. Open it with `froxel --control PORT`, or pass --launch"
        )));
    }
    let port = addr.rsplit(':').next().unwrap();
    let exe = std::env::current_exe()?;
    let mut child = Command::new(exe)
        .args(["--control", port])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()?;
    let start = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return Err(io::Error::other(
                "the editor exited before the control port opened",
            ));
        }
        if probe(addr) {
            // The editor keeps running after this process drops the handle.
            return Ok(());
        }
        if start.elapsed() > Duration::from_secs(20) {
            return Err(io::Error::other(format!(
                "timed out waiting for the editor at {addr}"
            )));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn probe(addr: &str) -> bool {
    let Ok(sock) = addr.parse() else { return false };
    std::net::TcpStream::connect_timeout(&sock, Duration::from_millis(200)).is_ok()
}

fn serve(mode: Mode) -> i32 {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut state = Proto::default();
    let mut session = Session::new();
    session.replace_empty();
    let mut remote = match &mode {
        Mode::Headless => None,
        Mode::Remote { addr } => match ControlClient::connect(addr) {
            Ok(client) => Some(client),
            Err(err) => {
                eprintln!("{err}");
                return 1;
            }
        },
    };
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(err) => {
                eprintln!("stdin: {err}");
                return 1;
            }
        };
        if line.len() > 16 * 1024 * 1024 {
            let _ = writeln!(
                stdout,
                r#"{{"jsonrpc":"2.0","id":null,"error":{{"code":-32700,"message":"line too long"}}}}"#
            );
            let _ = stdout.flush();
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => msg,
            Err(_) => {
                let _ = writeln!(
                    stdout,
                    r#"{{"jsonrpc":"2.0","id":null,"error":{{"code":-32700,"message":"parse error"}}}}"#
                );
                let _ = stdout.flush();
                continue;
            }
        };
        if let Some(reply) = dispatch(&mut state, &mut session, remote.as_mut(), &msg) {
            if writeln!(stdout, "{reply}").is_err() || stdout.flush().is_err() {
                return 0;
            }
        }
    }
    0
}

struct Proto {
    initialized: bool,
}

impl Default for Proto {
    fn default() -> Self {
        Self { initialized: false }
    }
}

fn dispatch(
    state: &mut Proto,
    session: &mut Session,
    remote: Option<&mut ControlClient>,
    msg: &Value,
) -> Option<Value> {
    if !msg.is_object() {
        return Some(rpc_error(Value::Null, -32600, "request must be an object"));
    }
    if msg.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(rpc_error(Value::Null, -32600, "jsonrpc must be \"2.0\""));
    }
    let Some(method) = msg.get("method").and_then(Value::as_str) else {
        return Some(rpc_error(Value::Null, -32600, "missing method"));
    };
    let id = msg.get("id").cloned();
    let notification = id.is_none();
    let id = id.unwrap_or(Value::Null);
    let params = msg.get("params").cloned().unwrap_or(json!({}));

    if notification {
        if method == "notifications/initialized" || method == "initialized" {
            state.initialized = true;
        }
        return None;
    }
    if method == "ping" {
        return Some(rpc_ok(id, json!({})));
    }
    if method == "initialize" {
        let asked = params
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or(PROTOCOL);
        let version = if SUPPORTED.contains(&asked) {
            asked
        } else {
            PROTOCOL
        };
        state.initialized = true;
        return Some(rpc_ok(
            id,
            json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "Froxel", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Edits apply to the active object, in that object's local cells. render path-traces a PNG in this process. screenshot, screenshot_viewport, screenshot_region, ui_inspect, and ui_click need an editor started with --control and this server started with --connect. The window path-traces in Render mode. Hand sculpting (docs/hand-sculpting.md) models a creature or a unique asset without a script: block out with fill_box, fill_rounded_box, and fill_ellipsoid, refine with set_voxels, render after every step, then set_method hand_sculpting before save_file. inspect_document reports the label as method."
            }),
        ));
    }
    if !state.initialized {
        return Some(rpc_error(id, -32600, "server not initialized"));
    }
    let result = match method {
        "tools/list" => json!({"tools": tools()}),
        "tools/call" => call_tool(session, remote, &params),
        "resources/list" => json!({"resources": []}),
        "resources/templates/list" => json!({"resourceTemplates": []}),
        "prompts/list" => json!({"prompts": []}),
        "logging/setLevel" => json!({}),
        other => return Some(rpc_error(id, -32601, &format!("method not found: {other}"))),
    };
    Some(rpc_ok(id, result))
}

fn call_tool(session: &mut Session, remote: Option<&mut ControlClient>, params: &Value) -> Value {
    if !params.is_object() {
        return rpc_tool_error_value(-32602, "params must be an object");
    }
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return tool_error("missing tool name");
    };
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    if !args.is_object() {
        return tool_error("arguments must be an object");
    }
    let mapped = match map_tool(name, &args) {
        Ok(mapped) => mapped,
        Err(err) => return tool_error(&err),
    };
    let result = match remote {
        Some(client) => client.call(&mapped.0, &mapped.1),
        None => apply(session, &mapped.0, &mapped.1),
    };
    match result {
        Ok(value) => tool_ok(value),
        Err(err) => tool_error(&err),
    }
}

fn map_tool(name: &str, args: &Value) -> Result<(String, Value), String> {
    let method = match name {
        "new_document" => "document.new",
        "open_file" => "document.open",
        "save_file" => "document.save",
        "inspect_document" => "document.inspect",
        "set_method" => "document.method",
        "get_voxel" => "voxel.get",
        "set_voxel" => "voxel.set",
        "set_voxels" => "voxel.set_many",
        "fill_box" => "voxel.fill",
        "fill_ellipsoid" => "voxel.ellipsoid",
        "fill_rounded_box" => "voxel.rounded",
        "get_voxels" => "voxel.get_box",
        "set_palette" => "palette.set",
        "set_active_object" => "object.set_active",
        "add_object" => "object.add",
        "set_object" => "object.set",
        "remove_object" => "object.remove",
        "mirror_object" => "object.mirror",
        "add_bone" => "rig.add",
        "set_bone" => "rig.set",
        "remove_bone" => "rig.remove",
        "bind_object" => "rig.bind",
        "list_bones" => "rig.list",
        "set_pose" => "pose.set",
        "clear_pose" => "pose.clear",
        "add_clip" => "clip.add",
        "set_clip_keys" => "clip.keys",
        "list_clips" => "clip.list",
        "remove_clip" => "clip.remove",
        "undo" => "edit.undo",
        "redo" => "edit.redo",
        "render" => "render",
        "screenshot" => "ui.screenshot",
        "screenshot_viewport" => "ui.screenshot",
        "screenshot_region" => "ui.screenshot",
        "ui_inspect" => "ui.inspect",
        "ui_click" => "ui.click",
        "export_mesh" => "mesh.export",
        "export_models" => "mesh.export_batch",
        "set_script" => "script.set",
        "get_script" => "script.get",
        "set_params" => "script.params",
        "list_params" => "script.params.list",
        "regenerate" => "script.regenerate",
        other => return Err(format!("unknown tool `{other}`")),
    };
    let params = match name {
        "screenshot_viewport" => {
            let mut params = args.clone();
            params["region"] = json!("viewport");
            params
        }
        "screenshot_region" => args.clone(),
        _ => args.clone(),
    };
    if name == "screenshot" || name == "screenshot_viewport" || name == "screenshot_region" {
        parse_shot(&params)?;
    }
    Ok((method.to_string(), params))
}

fn tool_ok(value: Value) -> Value {
    let mut shown = value.clone();
    let image = shown
        .as_object_mut()
        .and_then(|obj| obj.remove("pngBase64"))
        .and_then(|v| v.as_str().map(str::to_string));
    let text = serde_json::to_string_pretty(&shown).unwrap_or_else(|_| shown.to_string());
    let mut content = vec![json!({"type": "text", "text": text})];
    if let Some(data) = image {
        content.push(json!({"type": "image", "data": data, "mimeType": "image/png"}));
    }
    json!({"content": content, "isError": false})
}

fn tool_error(message: &str) -> Value {
    json!({
        "content": [{"type": "text", "text": message}],
        "isError": true
    })
}

fn rpc_tool_error_value(code: i32, message: &str) -> Value {
    let text = format!("{code}: {message}");
    json!({"content": [{"type": "text", "text": text}], "isError": true})
}

fn rpc_ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn rpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn num(value: &Value, key: &str) -> Result<f32, String> {
    value
        .get(key)
        .and_then(Value::as_f64)
        .map(|n| n as f32)
        .ok_or_else(|| format!("missing number {key}"))
}

fn tools() -> Vec<Value> {
    vec![
        tool("new_document", "Replace the session with an empty one-object document and the default palette.", json!({}), &[]),
        tool("open_file", "Open a .vox file, replacing the session document.", json!({"path": str_prop("Path of a .vox file")}), &["path"]),
        tool("save_file", "Write every non-empty object as a .vox scene. Replies with voxels (solid cells in the file) and objects (models written).", json!({"path": str_prop("Where to write. Omit to overwrite the open file.")}), &[]),
        tool("inspect_document", "Path, voxel count, world bounds, objects (name, world position, local bounds), the 256-color palette, and method: how the document was made (hand_sculpting, script, or null).", json!({}), &[]),
        tool("set_method", "Label how the document was made. hand_sculpting: primitive fills and set_voxels edits, no script (see docs/hand-sculpting.md). script: a Rhai script. null clears the label. An object with a stored script always reports script, and hand_sculpting is refused while one exists. Saved in the .vox and written to glTF asset.extras.method. One undo step.", json!({
            "method": {"type": ["string", "null"], "enum": ["hand_sculpting", "script", null], "description": "hand_sculpting, script, or null"}
        }), &["method"]),
        tool("get_voxel", "Read one local cell of the active object. 0 is empty. Coordinates are local to that object.", xyz_props(), &["x", "y", "z"]),
        tool("set_voxel", "Write one local cell of the active object. Coordinates are local. One undo step.", {
            let mut p = xyz_props();
            p["palette"] = int_prop("Palette index. 0 erases. 1..255 paint.");
            p
        }, &["x", "y", "z", "palette"]),
        tool("set_voxels", "Write up to 200000 local cells of the active object as one undo step. Coordinates are local to that object.", json!({"cells": {"type": "array", "description": "Each item is {x,y,z,palette} or [x,y,z,palette], in the active object's local cells. Last duplicate wins."}}), &["cells"]),
        tool("fill_box", "Fill an inclusive box of local cells in the active object. At most 2000000 cells. One undo step.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner");
            }
            p["palette"] = int_prop("Palette index. 0 erases.");
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1", "palette"]),
        tool("get_voxels", "List non-empty local cells of the active object inside an inclusive box. At most 100000.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner");
            }
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1"]),
        tool("set_palette", "Replace one palette slot, or many via entries, as one undo step. motion is none, sway, or bob.", json!({
            "index": int_prop("Palette slot 0..255"),
            "r": int_prop("Red 0..255"),
            "g": int_prop("Green 0..255"),
            "b": int_prop("Blue 0..255"),
            "a": int_prop("Alpha 0..255. Defaults to 255."),
            "motion": str_prop("none, sway, or bob. Omit to leave the slot's motion."),
            "motion_amount": num_prop("Motion strength, 0..4. Omit to leave it. Default 1 when a kind is first set."),
            "entries": {"type": "array", "description": "Batch of {index,r,g,b,a?,motion?,motion_amount?}. Omit when passing index."}
        }), &[]),
        tool("set_active_object", "Edit a different object from inspect_document. Later voxel tools use that object's local cells.", json!({"index": int_prop("Object index")}), &["index"]),
        tool("add_object", "Add an empty object at a world voxel position and return its index. It becomes active unless make_active is false. size is a local capacity hint; the object still grows up to 256 per axis. One undo step.", json!({
            "name": str_prop("Object name. Omit for Object N."),
            "position": ivec3_prop("World position [x, y, z] in voxel units. Defaults to [0, 0, 0]."),
            "size": ivec3_prop("Optional SIZE box [x, y, z], each axis 1..=256. Does not cap later growth below the per-object limit."),
            "make_active": {"type": "boolean", "description": "Select the new object. Defaults to true."}
        }), &[]),
        tool("set_object", "Rename, move, or hide an object. position is a world voxel coordinate; local cells stay put, so the voxels move with the object. One undo step.", json!({
            "index": int_prop("Object index"),
            "name": str_prop("New name"),
            "position": ivec3_prop("World position [x, y, z] in voxel units"),
            "hidden": {"type": "boolean", "description": "Hide the object. Hidden objects stay in the file and outliner."}
        }), &["index"]),
        tool("remove_object", "Remove an object and its voxels. A bone bound to it stays; the binding goes. Later object indices decrease by one, and the reply says so. Refuses the last object. One undo step.", json!({
            "index": int_prop("Object index")
        }), &["index"]),
        tool("fill_ellipsoid", "Fill the ellipsoid inscribed in an inclusive local box. Same limits and one undo step as fill_box. The box is checked before any cell is written.", json!({
            "x0": int_prop("Inclusive minimum x"),
            "y0": int_prop("Inclusive minimum y"),
            "z0": int_prop("Inclusive minimum z"),
            "x1": int_prop("Inclusive maximum x"),
            "y1": int_prop("Inclusive maximum y"),
            "z1": int_prop("Inclusive maximum z"),
            "palette": int_prop("Palette index. 0 erases.")
        }), &["x0", "y0", "z0", "x1", "y1", "z1", "palette"]),
        tool("fill_rounded_box", "Fill an inclusive local box with rounded edges. radius is the bevel, clamped to half the shortest side. radius 0 fills the whole box. Same limits and one undo step as fill_box.", json!({
            "x0": int_prop("Inclusive minimum x"),
            "y0": int_prop("Inclusive minimum y"),
            "z0": int_prop("Inclusive minimum z"),
            "x1": int_prop("Inclusive maximum x"),
            "y1": int_prop("Inclusive maximum y"),
            "z1": int_prop("Inclusive maximum z"),
            "palette": int_prop("Palette index. 0 erases."),
            "radius": num_prop("Edge bevel in voxels. 0 fills the box.")
        }), &["x0", "y0", "z0", "x1", "y1", "z1", "palette", "radius"]),
        tool("mirror_object", "Copy an object mirrored across a world axis through the origin. Cells use -coord-1 on that axis. One undo step. mirrorBones also copies the bound bone and its descendants, swapping an _l/_r suffix. The copy is unbound unless mirrorBones is true.", json!({
            "index": int_prop("Object index to copy"),
            "axis": str_prop("x, y, or z"),
            "name": str_prop("Name of the copy"),
            "position": ivec3_prop("World cell of the copy's local (0, 0, 0). Omit to use the minimum corner of the mirrored cells."),
            "mirrorBones": {"type": "boolean", "description": "Copy the bound bone subtree and swap _l/_r. Default false."}
        }), &["index", "axis", "name"]),
        tool("add_bone", "Add a bone. One undo step. parent is omitted for a root. pivot is a fractional world coordinate, so a joint can sit between cells.", json!({
            "name": str_prop("Unique bone name"),
            "parent": {"type": ["string", "null"], "description": "Parent bone name. Omit or null for a root."},
            "pivot": vec3_prop("World pivot [x, y, z]. Fractional values sit between cells."),
            "rotation": vec4_prop("Quaternion [x, y, z, w] in voxel space. Defaults to identity.")
        }), &["name", "pivot"]),
        tool("set_bone", "Change a bone's parent, pivot, rotation, or name. One undo step. Omit a field to leave it. parent null makes a root.", json!({
            "name": str_prop("Bone to edit"),
            "parent": {"type": ["string", "null"], "description": "Parent bone. null makes a root. Omit to leave the parent."},
            "pivot": vec3_prop("World pivot [x, y, z]"),
            "rotation": vec4_prop("Quaternion [x, y, z, w]"),
            "rename": str_prop("New unique name")
        }), &["name"]),
        tool("remove_bone", "Delete a bone. Objects bound to it become unbound. Children keep its parent. One undo step.", json!({
            "name": str_prop("Bone to delete")
        }), &["name"]),
        tool("bind_object", "Bind an object to one bone, or unbind it. One undo step. An unbound object follows the skeleton root at export.", json!({
            "index": int_prop("Object index from inspect_document"),
            "bone": {"type": ["string", "null"], "description": "Bone name. null unbinds."}
        }), &["index", "bone"]),
        tool("list_bones", "Bones with parent, pivot, rotation, and the object indices bound to each. Pivots are fractional world coordinates.", json!({}), &[]),
        tool("set_pose", "Preview a pose in the window. Each quaternion is local and multiplied on the right of the bind rotation, so a parent orbits its children. Omitted bones stay at the bind pose. Identity is a no-op. Does not change the bind pose. One undo step.", json!({
            "pose": {"type": "object", "description": "Map of bone name to a quaternion [x, y, z, w] or {rotation, scale}; scale is local per axis, 0 to 16, and children inherit it. Replaces the preview.", "additionalProperties": {"anyOf": [{"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, {"type": "object", "properties": {"rotation": {"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, "scale": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}}, "additionalProperties": false}]}}
        }), &["pose"]),
        tool("clear_pose", "Remove the preview pose. The bind pose stays. One undo step.", json!({}), &[]),
        tool("add_clip", "Add an empty named clip. Keys are local TRS in voxel space: translation is the full offset from the parent, not a delta. One undo step. Saved in the .vox MVAN chunk and written to glTF as an animation.", json!({
            "name": str_prop("Unique clip name")
        }), &["name"]),
        tool("set_clip_keys", "Replace one bone's keys in a clip. Times must be finite, >= 0, and increasing. rotation defaults to identity, translation to [0, 0, 0], scale to [1, 1, 1]. One undo step.", json!({
            "name": str_prop("Clip name"),
            "bone": str_prop("Bone name"),
            "keys": {"type": "array", "description": "Keyframes. time is required. rotation is [x, y, z, w]. translation and scale are [x, y, z].", "items": {"type": "object"}}
        }), &["name", "bone", "keys"]),
        tool("list_clips", "Named clips and their bone keys.", json!({}), &[]),
        tool("remove_clip", "Remove a clip. One undo step.", json!({
            "name": str_prop("Clip name")
        }), &["name"]),
        tool("undo", "Undo the last edit.", json!({}), &[]),
        tool("redo", "Redo the last undone edit.", json!({}), &[]),
        tool("render", "Path-trace the whole scene to a PNG. Blocks until the file is written. The camera frames the scene; yaw, pitch, and distance orbit that frame.", {
            let denoise = if crate::denoise::available() {
                "Open Image Denoise on the finished frame, using the first-hit albedo and normal. Default true."
            } else {
                "Edge-aware filter before writing. Default true. Build with --features oidn for Open Image Denoise."
            };
            json!({
            "path": str_prop("PNG file to write"),
            "width": int_prop("Pixels, 1..1024. Default 256."),
            "height": int_prop("Pixels, 1..1024. Default 256."),
            "samples": int_prop("Path-trace samples, 1..128. Default 16."),
            "bounces": int_prop("Max bounces, 0..8. Default 4."),
            "seed": int_prop("Sampling seed. Default 1."),
            "denoise": {"type": "boolean", "description": denoise},
            "atmospheric": {"type": "boolean", "description": "Nishita sky. Omit to keep the document setting."},
            "sky-intensity": num_prop("Sky intensity. Omit to keep the document setting."),
            "sun-intensity": num_prop("Sun intensity. Omit to keep the document setting."),
            "glass-mode": str_prop("probabilistic or absorption. Omit to keep the document setting."),
            "aperture": num_prop("Lens aperture. Omit to keep the document setting."),
            "focus": num_prop("Focus distance. Omit to keep the document setting."),
            "yaw": num_prop("Orbit yaw in degrees. 0 puts the camera on +X, looking toward -X. A model facing -Y is face-on at yaw -90. Omit to keep the framing angle (30)."),
            "pitch": num_prop("Orbit pitch in degrees, -90..90. 90 looks straight down. Omit to keep the framing angle (25)."),
            "distance": num_prop("Multiple of the framing distance, greater than 0 and at most 100. Default 1."),
            "orthographic": {"type": "boolean", "description": "Parallel rays. The frame matches the perspective view at the same distance. Default false."},
            "pose": {"type": "object", "description": "Temporary preview pose for this still. Bone name to a quaternion [x, y, z, w] or {rotation, scale}, on top of the bind pose. A zero scale hides that bone's parts. The document pose is restored afterwards.", "additionalProperties": {"anyOf": [{"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, {"type": "object", "properties": {"rotation": {"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, "scale": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}}, "additionalProperties": false}]}},
            "showBones": {"type": "boolean", "description": "Draw joint points and parent links on the PNG. Default false."},
            "views": {"type": "array", "description": "Extra PNGs. The required path is still written with the top-level yaw and pitch. Each item is {yaw, pitch, path}.", "items": {"type": "object"}}
            })
        }, &["path"]),
        tool("screenshot", "PNG of the whole editor window. Only when connected to a window.", json!({"path": str_prop("Optional file to write")}), &[]),
        tool("screenshot_viewport", "PNG of the 3D viewport. Only when connected to a window.", json!({"path": str_prop("Optional file to write")}), &[]),
        tool("screenshot_region", "PNG of a rectangle in window points, origin at the top-left of the client area. Only when connected to a window.", json!({
            "x": num_prop("Left edge, window points"),
            "y": num_prop("Top edge, window points"),
            "width": num_prop("Width in window points"),
            "height": num_prop("Height in window points"),
            "path": str_prop("Optional file to write")
        }), &["x", "y", "width", "height"]),
        tool("ui_inspect", "Controls from the previous frame: tool buttons, mode, outliner, sliders, palette cells, viewport chips, and the viewport rectangle, in window points. Only when connected to a window.", json!({}), &[]),
        tool("ui_click", "Click the topmost registered control at a window point. A viewport hit stamps the active tool. A slider hit sets that slider from the point along its track. A palette cell selects that slot. Only when connected to a window.", json!({
            "x": num_prop("X in window points, origin at the top-left of the client area"),
            "y": num_prop("Y in window points, origin at the top-left of the client area")
        }), &["x", "y"]),
        tool("set_script", "Store a Rhai script on an object and read its parameters. One undo step. Voxels change only when regenerate runs. A failure keeps the source and reports the script line.", json!({
            "object": int_prop("Object index. Omit for the active object."),
            "source": str_prop("Rhai source. Empty clears the script.")
        }), &["source"]),
        tool("get_script", "Script source, auto flag, and declared parameters.", json!({
            "object": int_prop("Object index. Omit for the active object.")
        }), &[]),
        tool("set_params", "Set script parameters by name. One undo step. Does not run the script.", json!({
            "object": int_prop("Object index. Omit for the active object."),
            "values": {"type": "object", "description": "Map of parameter name to a number, bool, or [r, g, b]."}
        }), &["values"]),
        tool("list_params", "Parameters from the last successful run of the object's script.", json!({
            "object": int_prop("Object index. Omit for the active object.")
        }), &[]),
        tool("regenerate", "Run the object's script and replace the voxels it writes. One undo step. There is no operation or voxel cap; the run stops at timeoutSeconds. A timeout or runtime error leaves the document unchanged and includes the script line.", json!({
            "object": int_prop("Object index. Omit for the active object."),
            "timeoutSeconds": {"type": "number", "description": "Stop the run after this many seconds. Default 300. 0 waits as long as it takes."}
        }), &[]),
        tool("export_mesh", "Write a glTF 2.0 .glb of every object. Exposed faces only. The default material is a 256-wide palette texture. A swaying or bobbing slot adds TEXCOORD_1 (sway, bob) on every vertex. A skeleton writes one rigid skin: each vertex belongs to one bone with weight 1. The reply includes the file's node, mesh, skin and animation structure.", json!({
            "path": str_prop("Where to write the .glb"),
            "merge": {"type": "boolean", "description": "Greedy-merge coplanar faces of one palette slot. Defaults to false."},
            "vertexColors": {"type": "boolean", "description": "Store a linear COLOR_0 on each vertex instead of the palette texture. Defaults to false."},
            "exportHidden": {"type": "boolean", "description": "Include hidden objects. Default false, which omits them."},
            "splitObjects": {"type": "boolean", "description": "One mesh node per object, still sharing one skin, so a game can hide a part. Default false is a single mesh."}
        }), &["path"]),
        tool("export_models", "Write 1 to 16 .glb files and report each file's structure: nodes, meshes (vertices, indices, bounds), skins, and animations. A warning names an empty mesh, a skin with no joints, a nameless or duplicated node, or an animation with no channels. source opens that .vox in its own session, so the open document is not replaced. Omit source to export the open document. One model's failure is reported on that model and does not skip the others.", json!({
            "models": {"type": "array", "minItems": 1, "maxItems": 16, "description": "Each item is {path, source?, merge?, vertexColors?, exportHidden?, splitObjects?}. path ends in .glb. source, when set, is a .vox file.", "items": {"type": "object"}}
        }), &["models"]),
    ]
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        }
    })
}

fn str_prop(description: &str) -> Value {
    json!({"type": "string", "description": description})
}

fn int_prop(description: &str) -> Value {
    json!({"type": "integer", "description": description})
}

fn num_prop(description: &str) -> Value {
    json!({"type": "number", "description": description})
}

fn vec3_prop(description: &str) -> Value {
    json!({
        "type": "array",
        "items": {"type": "number"},
        "minItems": 3,
        "maxItems": 3,
        "description": description
    })
}

fn ivec3_prop(description: &str) -> Value {
    json!({
        "type": "array",
        "items": {"type": "integer"},
        "minItems": 3,
        "maxItems": 3,
        "description": description
    })
}

fn vec4_prop(description: &str) -> Value {
    json!({
        "type": "array",
        "items": {"type": "number"},
        "minItems": 4,
        "maxItems": 4,
        "description": description
    })
}

fn xyz_props() -> Value {
    json!({
        "x": int_prop("Local x of the active object"),
        "y": int_prop("Local y"),
        "z": int_prop("Local z (up)")
    })
}

fn print_help() {
    eprintln!(
        "\
usage: froxel mcp [--connect HOST:PORT] [--launch]

Model Context Protocol on stdin/stdout, one JSON-RPC message per line.
Without --connect the session is in-process and has no window.
Screenshots need the editor:

  froxel --control 7979
  froxel mcp --connect 127.0.0.1:7979

--launch opens the editor with that control port when nothing is listening.
render path-traces a PNG in this process. The window path-traces in Render mode."
    );
}

#[cfg(test)]
#[path = "../tests/mcp.rs"]
mod tests;
