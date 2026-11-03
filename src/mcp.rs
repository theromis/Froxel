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
                "instructions": "Edits apply to the active object, in that object's local cells. render path-traces a PNG in this process. preview writes a fast lit PNG (the viewport mesh and light, rasterised, well under a second) framed on the model or one object; use it to look after every step. Its preset game is the gameplay camera, lighting (old_pond, or a project's lightingPresets) shows colour under a game's sun and ambient instead of the viewport light, overlay draws collision boxes, points, and connectors over the model, and inputs, variants, and reference make one labelled contact sheet of several models beside a concept image. Its previewTriangles is the unmerged mesh it draws, not the game's: its export counts (with export set to the game's options, e.g. colorAtlas) are what export_mesh writes. model_view writes the flat model view (palette colours, no light) for comparing with a turnaround sheet. set_view moves the window camera before screenshot_viewport. screenshot, screenshot_viewport, screenshot_region, ui_inspect, and ui_click need an editor started with --control and this server started with --connect. The window path-traces in Render mode. Hand sculpting (docs/hand-sculpting.md) models a creature or a unique asset without a script: block out with fill_box, fill_rounded_box, and fill_ellipsoid, grow limbs and roots with fill_tube, cut hollows with carve, soften with smooth, refine with set_voxels, and render after every step. Stone surfaces: column_relief breaks a cliff face into columns of uneven width, relief, and height, joints cuts real joint gaps (columns, courses, cracks), weather chips edges, dents pits, and adds colour noise, and moss, algae, and vines grow plants on exposed faces (the caller gives the slots and their motion). Their exclude boxes (and scatter's) keep walking surfaces whole, with no refill afterwards. sweep fills a cross-section along a curve for curved walls, banks, and ledges. set_marker names points, collision boxes, and connectors that the .vox keeps and the glb carries as nodes; export_variants writes a .vox and .glb per parameter set and merges them by name into a manifest of parameters, markers, and bounds (replace: true starts it fresh). examples/gorge_wall.rhai uses them all. A Rhai script (set_script) can call the same sculpt, voxel, object, rig, clip, and palette tools, with the MCP options in a trailing map, for a family of organic pieces (docs/scripting.md, examples/hollow_log.rhai)."
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
    if let Err(err) = known_arguments(name, &args) {
        return tool_error(&err);
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

/// Refuse an argument the tool's schema does not list, so a misspelt or
/// unsupported option is an error rather than silently ignored.
fn known_arguments(name: &str, args: &Value) -> Result<(), String> {
    let tools = tools();
    let Some(tool) = tools.iter().find(|tool| tool["name"] == name) else {
        return Ok(());
    };
    let properties = &tool["inputSchema"]["properties"];
    let Some(args) = args.as_object() else {
        return Ok(());
    };
    let unknown: Vec<&str> = args
        .keys()
        .filter(|key| properties.get(key.as_str()).is_none())
        .map(String::as_str)
        .collect();
    if unknown.is_empty() {
        return Ok(());
    }
    let mut known: Vec<&str> = properties
        .as_object()
        .map(|map| map.keys().map(String::as_str).collect())
        .unwrap_or_default();
    known.sort_unstable();
    Err(format!(
        "{name} has no argument {}; it takes {}",
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

fn map_tool(name: &str, args: &Value) -> Result<(String, Value), String> {
    let method = match name {
        "new_document" => "document.new",
        "open_file" => "document.open",
        "save_file" => "document.save",
        "inspect_document" => "document.inspect",
        "get_voxel" => "voxel.get",
        "set_voxel" => "voxel.set",
        "set_voxels" => "voxel.set_many",
        "fill_box" => "voxel.fill",
        "fill_ellipsoid" => "voxel.ellipsoid",
        "fill_rounded_box" => "voxel.rounded",
        "fill_tube" => "voxel.tube",
        "carve" => "voxel.carve",
        "smooth" => "voxel.smooth",
        "break_end" => "voxel.break",
        "scatter" => "voxel.scatter",
        "joints" => "voxel.joints",
        "column_relief" => "voxel.column_relief",
        "weather" => "voxel.weather",
        "moss" => "voxel.moss",
        "algae" => "voxel.algae",
        "vines" => "voxel.vines",
        "sweep" => "voxel.sweep",
        "set_marker" => "marker.set",
        "remove_marker" => "marker.remove",
        "list_markers" => "marker.list",
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
        "model_view" => "render.flat",
        "preview" => "render.preview",
        "set_view" => "view.set",
        "screenshot" => "ui.screenshot",
        "screenshot_viewport" => "ui.screenshot",
        "screenshot_region" => "ui.screenshot",
        "ui_inspect" => "ui.inspect",
        "ui_click" => "ui.click",
        "export_mesh" => "mesh.export",
        "export_models" => "mesh.export_batch",
        "export_variants" => "mesh.export_variants",
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
        tool("open_file", "Open a .vox file, replacing the session document. By default replies with path, objectCount, voxels, world bounds, activeObject, canUndo, canRedo, bones (count), paletteSlots (used slots), motionSlots, the first 20 objectNames, and moreObjects (how many names were left out). detail full replies with the inspect_document result instead.", json!({
            "path": str_prop("Path of a .vox file"),
            "detail": {"type": "string", "enum": ["summary", "full"], "description": "summary (default) is the short reply; full is the inspect_document result."},
            "fullPalette": {"type": "boolean", "description": "With detail full, list all 256 palette slots, not only the used ones. Defaults to false."}
        }), &["path"]),
        tool("save_file", "Write every non-empty object as a .vox scene. Replies with voxels (solid cells in the file) and objects (models written).", json!({"path": str_prop("Where to write. Omit to overwrite the open file.")}), &[]),
        tool("inspect_document", "Path, voxel count, world bounds, objects (name, world position, local bounds), palette colours keyed by index for the slots some voxel uses (fullPalette for all 256), and motion keyed by palette index for slots that are not still at amount 1.", json!({
            "fullPalette": {"type": "boolean", "description": "List all 256 palette slots, not only the used ones. Defaults to false."}
        }), &[]),
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
            p["mode"] = mode_prop();
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
            "palette": int_prop("Palette index. 0 erases."),
            "mode": mode_prop()
        }), &["x0", "y0", "z0", "x1", "y1", "z1", "palette"]),
        tool("fill_rounded_box", "Fill an inclusive local box with rounded edges. radius is the bevel, clamped to half the shortest side. radius 0 fills the whole box. Same limits and one undo step as fill_box.", json!({
            "x0": int_prop("Inclusive minimum x"),
            "y0": int_prop("Inclusive minimum y"),
            "z0": int_prop("Inclusive minimum z"),
            "x1": int_prop("Inclusive maximum x"),
            "y1": int_prop("Inclusive maximum y"),
            "z1": int_prop("Inclusive maximum z"),
            "palette": int_prop("Palette index. 0 erases."),
            "radius": num_prop("Edge bevel in voxels. 0 fills the box."),
            "mode": mode_prop()
        }), &["x0", "y0", "z0", "x1", "y1", "z1", "palette", "radius"]),
        tool("fill_tube", "Fill a round tube through 2..=1024 local points, for limbs, roots, branches, and twigs. Each point has its own radius, so the tube tapers; joints and ends are round. A whole-number point is the centre of that cell, and a cell is filled when its centre is within the radius of the axis. curve smooth passes a Catmull-Rom curve through the points; polyline (default) joins them straight. aspect, roll, and profile blade make it flat or a wedge, for shards, splinters, and plank ends. Colour with palette, or with palettes and a pattern: rings bands along the length every period cells, stripes divides the circumference into sectors (grooves along the tube), random picks per cell from seed. At most 2000000 cells; refused before any cell is written. One undo step.", json!({
            "points": {"type": "array", "description": "Each item is [x, y, z], [x, y, z, radius], or {x, y, z, radius?} in the active object's local cells. Numbers may be fractional."},
            "radius": num_prop("Radius for points that give none, 0.5..=128. 0.5 is one cell thick."),
            "curve": {"type": "string", "enum": ["polyline", "smooth"], "description": "polyline (default) or smooth."},
            "palette": int_prop("Palette index. 0 erases. Pass this or palettes."),
            "palettes": {"type": "array", "items": {"type": "integer"}, "description": "1..=64 palette indices cycled by pattern. Repeat an index to weight it."},
            "pattern": {"type": "string", "enum": ["rings", "stripes", "random"], "description": "How palettes are laid out. Defaults to rings."},
            "period": num_prop("rings: band length along the tube in cells. Defaults to 2."),
            "sectors": int_prop("stripes: sectors around the tube, 1..=256. Defaults to the number of palettes."),
            "seed": int_prop("random: seed. Defaults to 0."),
            "aspect": num_prop("Flatten: thickness over width, 0.05..=1. 1 (default) is round. The thickness is at least one cell and runs up for a level tube, so a flat tube lies flat."),
            "roll": num_prop("Degrees to turn a flat or blade cross-section about the axis. 90 stands a level shard on its edge."),
            "profile": {"type": "string", "enum": ["round", "blade"], "description": "blade tapers the thickness across the width to one cell at one edge: a wedge, for splinters and broken plank ends. Defaults to round."},
            "mode": mode_prop()
        }), &["points"]),
        tool("carve", "Remove the solid cells inside a box, ellipsoid, rounded_box, or tube of the active object, for hollows, tunnels, and pits. box, ellipsoid, and rounded_box take x0..z1 (and radius for rounded_box) exactly as their fill tools; tube takes points, radius, and curve as fill_tube. Same 2000000-cell limits, checked before any cell changes. One undo step. Replies with removed.", json!({
            "shape": {"type": "string", "enum": ["box", "ellipsoid", "rounded_box", "tube"], "description": "Shape to remove."},
            "x0": int_prop("box, ellipsoid, rounded_box: inclusive corner"),
            "y0": int_prop("Inclusive corner"),
            "z0": int_prop("Inclusive corner"),
            "x1": int_prop("Inclusive corner"),
            "y1": int_prop("Inclusive corner"),
            "z1": int_prop("Inclusive corner"),
            "radius": num_prop("rounded_box: edge bevel. tube: radius for points that give none."),
            "points": {"type": "array", "description": "tube: points as in fill_tube."},
            "curve": {"type": "string", "enum": ["polyline", "smooth"], "description": "tube: polyline (default) or smooth."},
            "aspect": num_prop("tube: flatten, as fill_tube."),
            "roll": num_prop("tube: roll in degrees, as fill_tube."),
            "profile": {"type": "string", "enum": ["round", "blade"], "description": "tube: as fill_tube."}
        }), &["shape"]),
        tool("smooth", "Soften a blocky region of the active object with a majority filter over each cell's 26 neighbours, inside an inclusive local box of at most 2000000 cells. An empty cell with at least fillAt solid neighbours is filled with the colour most of them hold; a solid cell with at most removeAt solid neighbours is removed. The defaults fill single-cell pits and inside corners and remove single-cell spikes and outside corners, and keep flat faces and edges. It also removes features one cell thick (twigs, thin walls), so leave those out of the box. Cells outside the box are read, never changed. One undo step. Replies with filled, removed, and the passes run.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner");
            }
            p["iterations"] = int_prop("Passes, 1..=8. Defaults to 1. Stops early when a pass changes nothing.");
            p["fillAt"] = int_prop("Fill an empty cell with at least this many solid neighbours, 1..=27. Defaults to 17; 14 also rounds inside edges; 27 never fills.");
            p["removeAt"] = int_prop("Remove a solid cell with at most this many solid neighbours, -1..=25. Defaults to 9; 12 also rounds outside edges; -1 never removes.");
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1"]),
        tool("break_end", "Break the active object across a plane, for a snapped trunk, branch, plank, or post. Every cell beyond a jagged surface around the plane goes: blocks of 2 by 3 cells across the face dip up to depth into the wood, a quarter of them deep pits. The cells left facing the cut take bark on the rim of the section and sapwood inside, and tapered flat splinters grow from the new face out along the normal, longer and more common near the rim with rimBias. Splinters fill only empty cells. Deterministic for a seed. At most 512 splinters and 2000000 cells. One undo step. Replies with removed, recoloured, and splinters.", json!({
            "point": vec3_prop("A point on the break plane, in local cells."),
            "normal": vec3_prop("Out of the wood, toward the part that breaks off. Need not be unit length."),
            "depth": num_prop("How far pits and the uneven face reach into the wood, 0..=32 cells. Defaults to 3."),
            "splinters": int_prop("Number of splinters, 0..=512. Pass this or density."),
            "density": num_prop("Splinters per cell of the new face, 0..=1. Defaults to 0.15."),
            "length": {"type": "array", "items": {"type": "number"}, "description": "Splinter length [min, max] in cells, 1..=64. Defaults to [4, 12]."},
            "thickness": num_prop("Splinter radius at its base, 0.5..=4. Defaults to 1."),
            "taper": num_prop("0 keeps the base radius to the tip; 1 (default) thins it to one cell."),
            "rimBias": num_prop("0..=1. How much more often and longer splinters are near the rim. Defaults to 0.5."),
            "bark": int_prop("Palette index for the rim of the new face and the base of rim splinters. Omit to keep the cells' colour."),
            "sapwood": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices mixed over the new face and the splinters. Omit to keep the cells' colour."},
            "tips": int_prop("Palette index for the outer part of each splinter, often a lighter wood. Omit to use the sapwood."),
            "seed": int_prop("Seed. Defaults to 0.")
        }), &["point", "normal"]),
        tool("scatter", "Scatter small pieces of debris (shard, chip, pebble, chunk) into the active object, for a heap at a break, chips at a stump, gravel. Each piece starts inside the inclusive box's x and y at its top and falls straight down until the next step would touch a solid cell (the model or a piece before it) or pass floor, so pieces rest on what is below and pile up. Only cells, no physics; only empty cells are written. A piece that meets a solid cell at the top of the box, or would rest in an exclude box, is skipped. Deterministic for a seed. At most 2000 pieces, a box of 2000000 cells, and 200000 new cells. One undo step. Replies with pieces and skipped.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner");
            }
            p["palettes"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Palette indices, one picked per piece. A single integer works too."});
            p["count"] = int_prop("Pieces, 0..=2000. Defaults to 20.");
            p["kinds"] = json!({"type": "array", "items": {"type": "string", "enum": ["shard", "chip", "pebble", "chunk"]}, "description": "Piece kinds to mix. shard is a flat wedge, chip a flake one cell thick, pebble a squashed ellipsoid, chunk a block with chipped corners. Defaults to all four."});
            p["size"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Piece size [min, max] in cells, 1..=16, or one number. Defaults to [1, 3]."});
            p["seed"] = int_prop("Seed. Defaults to 0.");
            p["floor"] = int_prop("Lowest z a piece may rest at. Defaults to the box's bottom.");
            p["clump"] = num_prop("0 spreads pieces evenly over the box; 1 piles them in its middle. Defaults to 0.5.");
            p["exclude"] = exclude_prop();
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1", "palettes"]),
        tool("joints", "Cut real joint gaps into the solid cells of an inclusive box of the active object, for fractured limestone, masonry, and cracked rock. The cells are split into stones by pattern: columns (vertical columns of random widths, seen from above as a jittered grid, with a bed joint now and then), courses (horizontal courses of blocks whose vertical joints shift from course to course, a running bond), or cracks (an irregular network between jagged pieces). Cells on a boundary are removed down to depth cells under the surface; the solid cell at the bottom of each gap takes the dirt slot. exclude boxes keep walking surfaces whole. Deterministic for a seed. A box of at most 2000000 cells. One undo step. Replies with removed and dirtied.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner");
            }
            p["pattern"] = json!({"type": "string", "enum": ["columns", "courses", "cracks"], "description": "columns, courses (running bond), or cracks"});
            p["spacing"] = json!({"type": "array", "items": {"type": "number"}, "description": "Stone width range [min, max] in cells, 2..=128, or one number: column width, block length, or crack piece size. Defaults to [4, 9] for columns, [6, 11] for courses, [6, 11] for cracks."});
            p["course"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Course height range [min, max] in cells, 1..=128 (courses), or the spacing of bed joints across a column (columns). Defaults to [3, 5] and [10, 24]. Cracks ignore it."});
            p["width"] = int_prop("Gap width in cells, 1..=3. Defaults to 1.");
            p["depth"] = int_prop("Gap depth under the surface in cells, 1..=16. Defaults to 2.");
            p["dirt"] = int_prop("Palette index for the solid cell at the bottom of each gap, such as dark dirt. Omit to keep its colour.");
            p["only"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Cut only cells of these palette indices (the stone slots), so moss and plants stay. Omit to cut every slot."});
            p["density"] = num_prop("cracks: the share of piece boundaries that crack, 0..=1. Defaults to 0.6.");
            p["exclude"] = exclude_prop();
            p["seed"] = int_prop("Seed. Defaults to 0.");
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1", "pattern"]),
        tool("column_relief", "Break a cliff face of the active object into columns, for fractured limestone cliffs: along the face, columns of random width stand out of it by a random amount and end at uneven heights around the crown. The face is found in the cells of the inclusive box, so a swept, curved wall works: for each step along it, the median of the frontmost solid cell of every row up to the crown, which a ledge does not pull forward. A column fills empty cells from depth cells behind the face to out cells in front of it, from the box's bottom to its top. With lower (the default), a column that ends below the crown cuts the wall above it, depth cells back from the face, so the crown is uneven; lower false only adds cells, columns rising above the crown or ending short in front of it. Writes stay inside the box and out of exclude boxes. Run it after sweep and before joints and weather. Deterministic for a seed. A box of at most 2000000 cells. One undo step. Replies with columns, added, removed, and crown.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner; a few cells larger than the wall so columns can stand out of it");
            }
            p["facing"] = json!({"type": ["string", "array"], "description": "The face's outward direction, along x or y: +x, -x, +y, -y, or a vector such as [0, -1, 0]."});
            p["palette"] = int_prop("Palette index of the columns, usually the stone.");
            p["width"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Column width range [min, max] along the face in cells, 1..=64, or one number. Defaults to [6, 12]."});
            p["out"] = json!({"type": "array", "items": {"type": "integer"}, "description": "How far a column stands out of the face, [min, max] cells, 0..=16, or one number. Defaults to [1, 4]."});
            p["crown"] = int_prop("Height the column tops scatter around. Defaults to the highest solid cell in the box.");
            p["top"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Column top range [min, max] relative to the crown in cells, -256..=256: negative ends short of it, positive rises above it. Defaults to [-8, 4]."});
            p["lower"] = json!({"type": "boolean", "description": "A column ending below the crown cuts the wall above it, so the crown is uneven. false only adds cells (only rising). Defaults to true."});
            p["depth"] = int_prop("How far a column reaches back into the face, and how deep the cut above a short column goes, 1..=16 cells. Defaults to 3.");
            p["exclude"] = exclude_prop();
            p["seed"] = int_prop("Seed. Defaults to 0.");
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1", "facing", "palette"]),
        tool("weather", "Weather the exposed cells of an inclusive box of the active object: chip edges and corners (small irregular chunks off cells exposed on two or more sides), dent flat faces with pits (real holes, not paint), and add subtle colour noise in soft patches. The faces a chip or pit uncovers take the broken slot, usually a slightly darker stone (not dirt). exclude boxes keep walking surfaces whole. Deterministic for a seed. A box of at most 2000000 cells. One undo step. Replies with chips, pits, removed, broken, and recoloured.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner");
            }
            p["chip"] = num_prop("Chance that an edge cell starts a chip, 0..=1; corners are likelier. Defaults to 0.15. 0 chips nothing.");
            p["chipSize"] = int_prop("Most cells one chip takes, 1..=8. Defaults to 2.");
            p["pits"] = num_prop("Chance that a flat face cell starts a pit, 0..=1. Defaults to 0.02.");
            p["pitDepth"] = int_prop("Deepest pit in cells, 1..=4. Defaults to 1.");
            p["broken"] = int_prop("Palette index for the faces a chip or pit uncovers, about 0.9 x the stone. Omit to keep their colour.");
            p["noise"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Palette indices of the colour noise, a few tones within a few units of the stone. Omit for no noise."});
            p["noiseDensity"] = num_prop("Share of exposed cells the noise recolours, 0..=1. Defaults to 0.35.");
            p["noiseScale"] = num_prop("Noise patch size in cells, 1..=32. Defaults to 3.");
            p["only"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Act only on cells of these palette indices (the stone slots). Omit to act on every slot."});
            p["exclude"] = exclude_prop();
            p["seed"] = int_prop("Seed. Defaults to 0.");
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1"]),
        tool("moss", "Grow moss on the exposed faces of an inclusive box of the active object that pass the face filter: cushions on tops (where the top face passes) and curtains running down side faces from a top edge, each with a ragged end. Coverage comes in soft patches mixed with speckle by noise. layer grow adds the moss on the empty side of each face (only empty cells are written); paint recolours the surface cells. palettes are picked in patches, darker and lighter moss. Cut joints and weather the stone first. Deterministic for a seed. A box of at most 2000000 cells, at most 400000 cells written. One undo step. Replies with added, painted, and curtains.", growth_props(&[
            ("curtains", num_prop("Share of moss-capped top edges a curtain runs down from (any top edge when faces leaves out top), 0..=1. Defaults to 0.3. 0 grows caps only.")),
            ("length", json!({"type": "array", "items": {"type": "integer"}, "description": "Curtain length range [min, max] in cells, 1..=64, or one number. Defaults to [2, 8]."})),
            ("thickness", int_prop("Cushion thickness on tops, 1..=3 (layer grow). Defaults to 1.")),
        ], "Coverage, 0..=1. Defaults to 0.5.", "top and side"), &["x0", "y0", "z0", "x1", "y1", "z1", "palettes"]),
        tool("algae", "Paint algae on the exposed faces of an inclusive box of the active object inside a height band, usually the waterline: dense at the bottom of the band and thinning upward in noisy clumps with stone peeking through. palettes go from the bottom of the band to its top (a darker tone low, a lighter one high). Deterministic for a seed. A box of at most 2000000 cells. One undo step. Replies with added and painted.", growth_props(&[], "Coverage at the bottom of the band, 0..=1. Defaults to 0.9.", "top and side"), &["x0", "y0", "z0", "x1", "y1", "z1", "palettes"]),
        tool("vines", "Hang vine strands from the top edges of an inclusive box of the active object: each strand starts beside an edge cell whose side passes the face filter and falls straight down through empty cells, wandering a little along the edge with noise, until its length or something solid below. palettes are graded from the anchor to the tip, so give them motion sway rising from the anchor (set_palette motion). blooms adds an occasional bloom cell beside a strand. Only empty cells are written. Deterministic for a seed. A box of at most 2000000 cells, at most 400000 cells written. One undo step. Replies with added, strands, and blooms.", growth_props(&[
            ("length", json!({"type": "array", "items": {"type": "integer"}, "description": "Strand length range [min, max] in cells, 1..=128, or one number. Defaults to [4, 16]."})),
            ("blooms", json!({"type": "array", "items": {"type": "integer"}, "description": "Palette indices of blooms beside the strands. Omit for none."})),
            ("bloomChance", num_prop("Chance of a bloom beside each strand cell, 0..=1. Defaults to 0.06.")),
        ], "Share of top edge cells a strand starts from, 0..=1. Defaults to 0.08.", "side"), &["x0", "y0", "z0", "x1", "y1", "z1", "palettes"]),
        tool("sweep", "Fill a 2D cross-section along a path of 2..=1024 local points, kept perpendicular to it, for curved walls, banks, ledges, and kerbs with no corners. The profile's u runs to the left of the path (seen from above, walking along it) and v up, so a wall profile stands on the path; a whole-number point is a cell centre. profile is a polygon of [u, v] points, or width (across, centred on the path), height (up from it), and radius (rounded corners) give a rounded rectangle. Each point may carry its own twist (degrees about the path) and scale, interpolated along it. curve smooth passes a Catmull-Rom curve through the points; extra slabs fill the outside of a bend. A cell is in when its centre is inside the profile at the nearest slab, so a flat top stays one flat layer. palettes lay out in bands (by height in the profile, strata), rings (along the path), or random. Writes as mode says. At most 2000000 cells. One undo step. Replies with cells and length.", json!({
            "points": {"type": "array", "items": {"type": ["array", "object"]}, "description": "Path points: [x, y, z], [x, y, z, twist], [x, y, z, twist, scale], or {x, y, z, twist?, scale?}. twist is degrees counter-clockwise looking along the path (default 0); scale multiplies the profile, 0.05..=16 (default 1)."},
            "profile": {"type": "array", "items": {"type": "array", "items": {"type": "number"}}, "description": "Closed polygon of 3..=256 [u, v] points in cells: u to the left of the path, v up. Omit to use width, height, and radius."},
            "width": num_prop("Rounded rectangle width across the path, centred on it, 0.5..=512. Used without profile."),
            "height": num_prop("Rounded rectangle height up from the path, 0.5..=512. Used without profile."),
            "radius": num_prop("Rounded rectangle corner radius, 0 up to half the smaller side. Defaults to 0."),
            "curve": str_prop("polyline (default) or smooth"),
            "palette": int_prop("Palette index, or pass palettes"),
            "palettes": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices laid out by pattern"},
            "pattern": {"type": "string", "enum": ["bands", "rings", "random"], "description": "bands (default) by height v in the profile, period cells each, like strata; rings along the path; random per cell"},
            "period": num_prop("Band or ring size in cells, 0.5..=512. Defaults to 2."),
            "seed": int_prop("Seed of the random pattern. Defaults to 0."),
            "mode": mode_prop()
        }), &["points"]),
        tool("set_marker", "Name a point, a collision box, or a connector (a piece's joining end) on an object, for a game to read: a ledge top, a spawn, a collision volume, where two pieces join. It is saved in the .vox, listed in the export_mesh and export_models replies, and exported as a glTF node named after the marker with extras.froxel (kind, plus size, min, and max for a box and direction for a connector, whose node rotation turns -Z toward it) and in the scene's extras.froxelMarkers. Coordinates are the object's local mesh units: cell (x, y, z) spans x..x+1, so the top centre of cell (4, 4, 10) is [4.5, 4.5, 11]. The marker moves with its object. A marker of the same name is replaced. One undo step. Replies with the marker in local, world, and glTF coordinates.", json!({
            "name": str_prop("Marker name, 1 to 64 characters, such as ledge_top_4m. The glTF node takes it."),
            "kind": {"type": "string", "enum": ["point", "box", "connector"], "description": "point (default), box (collision), or connector"},
            "point": vec3_prop("point and connector: [x, y, z] in local mesh units"),
            "direction": vec3_prop("connector: the way the joining end faces, [x, y, z], normalised"),
            "x0": int_prop("box: inclusive cell corner; the box spans x0..x1+1"),
            "y0": int_prop("box: inclusive cell corner"),
            "z0": int_prop("box: inclusive cell corner"),
            "x1": int_prop("box: inclusive cell corner"),
            "y1": int_prop("box: inclusive cell corner"),
            "z1": int_prop("box: inclusive cell corner"),
            "object": {"type": ["integer", "string"], "description": "Object index or name it belongs to. Defaults to the active object."}
        }), &["name"]),
        tool("remove_marker", "Remove the marker called name, or every marker with all: true. One undo step. Replies with removed.", json!({
            "name": str_prop("Marker name"),
            "all": {"type": "boolean", "description": "Remove every marker"}
        }), &[]),
        tool("list_markers", "Markers with their kind, object, and local, world, and glTF (Y-up) coordinates: a point, a box's min, max, centre, and size, a connector's point, direction, and node rotation.", json!({}), &[]),
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
        tool("render", "Path-trace the whole scene to a PNG. Blocks until the file is written. The camera orbits at yaw and pitch and fits the framed box (every visible posed cell, or target) inside the frame with margin on each side, at every angle of the call; zoom and distance scale that fit. Give only width or only height and the other side follows the model, so a long model gets a wide frame. Replies framed (the world box) and distance.", {
            let denoise = if crate::denoise::available() {
                "Open Image Denoise on the finished frame, using the first-hit albedo and normal. Default true."
            } else {
                "Edge-aware filter before writing. Default true. Build with --features oidn for Open Image Denoise."
            };
            json!({
            "path": str_prop("PNG file to write"),
            "width": int_prop("Pixels, 1..4096, at most 4096 x 2048 in all. Default 256, or from height and the model's shape."),
            "height": int_prop("Pixels, 1..4096. Default 256, or from width and the model's shape."),
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
            "distance": num_prop("Multiple of the fitted distance, greater than 0 and at most 100. Below 1 is closer. Default 1."),
            "zoom": num_prop("Magnification of the fitted frame, 0.01..100. 2 is twice as large (crops the box), 0.5 half. Default 1."),
            "margin": num_prop("Empty border on each side of the fitted box as a fraction of the frame, 0..0.45. Default 0.05."),
            "target": target_prop(),
            "orthographic": {"type": "boolean", "description": "Parallel rays, fitted to the box like perspective. Default false."},
            "pose": {"type": "object", "description": "Temporary preview pose for this still. Bone name to a quaternion [x, y, z, w] or {rotation, scale}, on top of the bind pose. A zero scale hides that bone's parts. The document pose is restored afterwards.", "additionalProperties": {"anyOf": [{"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, {"type": "object", "properties": {"rotation": {"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, "scale": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}}, "additionalProperties": false}]}},
            "showBones": {"type": "boolean", "description": "Draw joint points and parent links on the PNG. Default false."},
            "view": view_prop(),
            "views": {"type": "array", "description": "Extra PNGs. The required path is still written with the top-level yaw and pitch. Each item is {yaw, pitch, path} or {view, path}.", "items": {"type": "object"}}
            })
        }, &["path"]),
        tool("model_view", "Flat model view to a PNG, like the editor's model view without shading: each pixel is the palette colour of the first visible cell under its centre, with no light, no tonemap, and no denoise, so it compares directly with a turnaround sheet. No path tracer. The camera fits the framed box like render; every view of the call shares one centre and one scale. Orthographic replies pixelsPerVoxel.", json!({
            "path": str_prop("PNG file to write"),
            "width": int_prop("Pixels, 1..4096. Default 512, or from height, pixelsPerVoxel, and the model's shape."),
            "height": int_prop("Pixels, 1..4096. Default 512, or from width, pixelsPerVoxel, and the model's shape."),
            "view": view_prop(),
            "yaw": num_prop("Orbit yaw in degrees. 0 puts the camera on +X, looking toward -X. A model facing -Y is face-on at yaw -90. Default 30."),
            "pitch": num_prop("Orbit pitch in degrees, -90..90. 90 looks straight down. Default 25."),
            "distance": num_prop("Multiple of the fitted distance, greater than 0 and at most 100. Below 1 is closer. Default 1."),
            "zoom": num_prop("Magnification of the fitted frame, 0.01..100. 2 is twice as large (crops the box), 0.5 half. Default 1."),
            "margin": num_prop("Empty border on each side of the fitted box as a fraction of the frame, 0..0.45. Default 0.05."),
            "target": target_prop(),
            "pixelsPerVoxel": num_prop("Orthographic scale instead of the fit: pixels per voxel edge, 0.05..256. Omitted width and height then follow the framed box. Not with zoom or distance."),
            "orthographic": {"type": "boolean", "description": "Parallel rays. Default true."},
            "background": {"type": ["string", "array"], "description": "#rrggbb, #rrggbbaa, or [r, g, b, a?] for pixels that hit no cell. Default transparent."},
            "pose": {"type": "object", "description": "Temporary preview pose for this still. Bone name to a quaternion [x, y, z, w] or {rotation, scale}, on top of the bind pose. A zero scale hides that bone's parts. The document pose is restored afterwards.", "additionalProperties": {"anyOf": [{"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, {"type": "object", "properties": {"rotation": {"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, "scale": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}}, "additionalProperties": false}]}},
            "showBones": {"type": "boolean", "description": "Draw joint points and parent links on the PNG. Default false."},
            "views": {"type": "array", "description": "Extra PNGs with the same camera settings. Each item is {view, path} or {yaw, pitch, path}.", "items": {"type": "object"}}
        }), &["path"]),
        tool("preview", "Fast lit preview to a PNG in well under a second: the editor viewport's mesh and light (sun plus ambient), rasterised on the CPU with no path tracer and no window. The camera orbits at yaw and pitch and fits the model, or one object, inside the image with a margin. Use it to check shape and framing often; use render for lighting and materials and model_view for palette-exact colours. lighting lights it as a game does instead (old_pond is frog-world's Old Pond: warm sun, blue ambient, its background), to judge colour under the game's light; lightingPresets adds a project's presets. preset names a camera: game is the gameplay angle (orthographic, yaw -60, pitch 25), and presets defines or overrides presets. inputs (.vox files, or {path?, label?, params?}), variants (parameter sets of the open scripted document, or a variants JSON path), or reference (a concept PNG placed as the first tile) make a contact sheet instead: one tile per model in a grid with its name and parameters under it; width and height are then each tile's (default 512). The open document and its undo history are left as they were. overlay draws the markers over the model, collision boxes as outlines, with their names. Two triangle counts come back and they are different meshes: previewTriangles is the unmerged viewport mesh the image draws; export.triangles and export.quads are what export_mesh or export_models writes with the export options given in export (merge, colorAtlas, ...; all off by default), so pass the game's export options to see the game's count. countsLabel writes both in the corner. Replies with tiles (label, x, y, width, height, previewTriangles, export, voxels, params).", merged(json!({
            "path": str_prop("PNG file to write. Its folder is created."),
            "width": int_prop("Pixels, 1..4096. Default 1024; a contact sheet tile's, default 512."),
            "height": int_prop("Pixels, 1..4096. Default 1024; a contact sheet tile's, default 512."),
            "preset": str_prop("Named camera: game (orthographic, yaw -60, pitch 25, the gameplay angle), default (perspective, yaw 30, pitch 25), or one from presets. yaw, pitch, view, orthographic, fov, and margin given here win over it."),
            "presets": {"type": "object", "description": "Camera presets for this call: name to {yaw, pitch, orthographic, fov, margin}. A built-in name (game, default) is overridden field by field; another name is a new preset over default.", "additionalProperties": {"type": "object"}},
            "inputs": {"type": "array", "items": {"type": ["string", "object"]}, "description": "Contact sheet: .vox paths, or {path?, label?, params?} (no path is the open document; params are set_params values applied before the tile, so a scripted file is baked with them). At most 64."},
            "variants": {"type": ["array", "string"], "description": "Contact sheet of the open scripted document: parameter sets [{name?, params}] as export_variants takes, or the path of a variants JSON file. Each is baked in a scratch run; the document is restored."},
            "reference": {"type": ["string", "object"], "description": "Contact sheet: a PNG (a concept or turnaround) or {path, label?}, scaled to fit the first tile with its aspect kept. Alone, it sits beside the open document."},
            "columns": int_prop("Contact sheet columns, 1..65. Default about square."),
            "labels": {"type": "boolean", "description": "Contact sheet: each tile's name and parameters under it. Default true."},
            "overlay": {"type": ["boolean", "string", "array"], "description": "Draw the markers (set_marker, Rhai marker, collision_box, connector) over the model: true or all, or boxes (collision boxes as outlines), points, connectors (an arrow toward their direction), markers, or a list. What the model hides is drawn faded and dashed, so a collision box can be checked against the stone before export. Replies with overlay: each drawn marker's name, kind, object, anchor pixel, and whether it is hidden. Works on contact sheet tiles too."},
            "overlayLabels": {"type": "boolean", "description": "Each overlay marker's name beside it. Default true."},
            "export": {"type": ["object", "boolean"], "description": "The export options the reply's export counts use, as export_mesh takes them: merge, colorAtlas, vertexColors, exportHidden, splitObjects, aoStrength, aoRadius, aoTarget (all off when absent, like export_mesh). Replies export {triangles, quads, and these options}, or error when that export would fail. The counts are of the whole document whatever object frames, and match the glb. Unmerged counting is quick; merge costs about what the export does. false skips it."},
}), json!({
            "lighting": lighting_prop(),
            "lightingPresets": lighting_presets_prop(),
            "countsLabel": {"type": "boolean", "description": "Write previewTriangles (the unmerged mesh drawn) and the export counts in the image's bottom-left corner, and in each contact sheet tile. Default false."},
            "view": view_prop(),
            "yaw": num_prop("Orbit yaw in degrees. 0 puts the camera on +X, looking toward -X. A model facing -Y is face-on at yaw -90. Default 30."),
            "pitch": num_prop("Orbit pitch in degrees, -90..90. 90 looks straight down. Default 25."),
            "orthographic": {"type": "boolean", "description": "Parallel projection fitted tightly to the silhouette; replies pixelsPerVoxel. Default false (the viewport's 50 degree perspective)."},
            "fov": num_prop("Vertical field of view in degrees for perspective, 10..120. Default 50, like the viewport; lower flattens a long model."),
            "margin": num_prop("Empty border on each side as a fraction of the image, 0..0.45. Default 0.06."),
            "object": {"type": ["integer", "string"], "description": "Frame this object (index or name) instead of the whole model. Other visible objects are still drawn unless isolate is true. A hidden object named here is drawn."},
            "isolate": {"type": "boolean", "description": "Draw only object. Default false."},
            "background": {"type": ["string", "array"], "description": "#rrggbb, #rrggbbaa, or [r, g, b, a?]. Default the viewport grey #16181c; #00000000 is transparent."},
            "antialias": {"type": "boolean", "description": "2x2 supersampling up to 2048x2048 output pixels. Default true."},
            "pose": {"type": "object", "description": "Temporary preview pose for this still. Bone name to a quaternion [x, y, z, w] or {rotation, scale}, on top of the bind pose. The document pose is restored afterwards.", "additionalProperties": {"anyOf": [{"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, {"type": "object", "properties": {"rotation": {"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, "scale": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}}, "additionalProperties": false}]}}
        })), &["path"]),
        tool("set_view", "Move the session camera. Connected to a window, that is the editor's camera, so the next screenshot_viewport shows an exact view. Not an edit and not undoable. The edit viewport stays lit; use model_view for palette-exact colours.", json!({
            "view": view_prop(),
            "yaw": num_prop("Orbit yaw in degrees. 0 puts the camera on +X, looking toward -X. Omit to keep the current yaw."),
            "pitch": num_prop("Orbit pitch in degrees, -90..90. 90 looks straight down. Omit to keep the current pitch."),
            "frame": {"type": "boolean", "description": "Centre on every object at the framing distance first. Default true."},
            "distance": num_prop("Multiple of the framing distance, greater than 0 and at most 100. Needs frame. Default 1."),
            "orthographic": {"type": "boolean", "description": "Parallel projection in the edit viewport and the CPU trace. Omit to keep it."},
            "preset": str_prop("Named camera: game (orthographic, yaw -60, pitch 25) or default; yaw, pitch, view, and orthographic given here win over it."),
            "presets": {"type": "object", "description": "Presets for this call, as preview takes them.", "additionalProperties": {"type": "object"}},
            "bones": {"type": "boolean", "description": "Draw the skeleton overlay in the viewport. false hides it for a screenshot. Omit to keep it."}
        }), &[]),
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
        tool("set_script", "Store a Rhai script on an object and read its parameters. One undo step. Voxels change only when regenerate runs. A failure keeps the source and reports the script line. Scripts have the sculpt tools as functions (fill_box with a mode, fill_ellipsoid, fill_rounded_box, fill_tube, carve_box, carve_ellipsoid, carve_rounded_box, carve_tube, smooth, break_end, scatter, joints, column_relief, weather, moss, algae, vines, sweep, and marker, collision_box, connector for markers) with the MCP options in a trailing map such as #{mode: \"empty\"}, get_voxels, set_voxels, and the document tools (set_palette, set_object, remove_object, mirror_object, add_bone, set_bone, remove_bone, bind_object, list_bones, set_pose, clear_pose, add_clip, set_clip_keys, list_clips, remove_clip), which apply after the script in the same undo step, plus sqrt, pow, PI, min, max, clamp, push on arrays, and strings (\"ledge_\" + 4, `ledge_${i}`, to_string); docs/scripting.md lists them.", json!({
            "object": int_prop("Object index. Omit for the active object."),
            "source": str_prop("Rhai source. Empty clears the script.")
        }), &["source"]),
        tool("get_script", "Script source, auto flag, and declared parameters.", json!({
            "object": int_prop("Object index. Omit for the active object.")
        }), &[]),
        tool("set_params", "Set script parameters by name. One undo step. Does not run the script.", json!({
            "object": int_prop("Object index. Omit for the active object."),
            "values": {"type": "object", "description": "Map of parameter name to a number, bool, [r, g, b], a list of numbers (floats), or a list of [x, y] or [x, y, z] points (vec2s, vec3s). Out of range or too long is an error."}
        }), &["values"]),
        tool("list_params", "Parameters from the last successful run of the object's script.", json!({
            "object": int_prop("Object index. Omit for the active object.")
        }), &[]),
        tool("regenerate", "Run the object's script and replace the voxels it writes. One undo step. There is no operation or voxel cap; the run stops at timeoutSeconds. A timeout or runtime error leaves the document unchanged and includes the script line.", json!({
            "object": int_prop("Object index. Omit for the active object."),
            "timeoutSeconds": {"type": "number", "description": "Stop the run after this many seconds. Default 300. 0 waits as long as it takes."}
        }), &[]),
        tool("export_mesh", "Write a glTF 2.0 .glb of every object. Exposed faces only. The default material is a 256-wide palette texture. A swaying or bobbing slot adds TEXCOORD_1 (sway, bob) on every vertex. A skeleton writes one rigid skin: each vertex belongs to one bone with weight 1. The reply includes the file's node, mesh, skin and animation structure, quads, and triangles (every triangle in the glb).", json!({
            "path": str_prop("Where to write the .glb"),
            "merge": {"type": "boolean", "description": "Greedy-merge coplanar faces of one palette slot. Defaults to false."},
            "vertexColors": {"type": "boolean", "description": "Store a linear COLOR_0 on each vertex instead of the palette texture. Defaults to false."},
            "exportHidden": {"type": "boolean", "description": "Include hidden objects. Default false, which omits them."},
            "splitObjects": {"type": "boolean", "description": "One mesh node per object, still sharing one skin, so a game can hide a part. Default false is a single mesh."},
            "colorAtlas": {"type": "boolean", "description": "Merge coplanar faces whatever their palette slot and paint each cell into a per-mesh colour atlas texture (nearest filtering) instead of the palette, for far fewer triangles on noisy palettes. Faces of different motion or opacity stay apart. One rigid mesh only: not with a skeleton, splitObjects or vertexColors. Defaults to false."},
            "aoStrength": {"type": "number", "minimum": 0, "maximum": 1, "description": "Bake ambient occlusion into COLOR_0 so crevices, joints and the undersides of overhangs export darker, at no runtime cost. 0..1, how much light a fully occluded corner loses. Multiplied into vertexColors, otherwise a grey COLOR_0 that glTF (and Godot's StandardMaterial3D) multiplies with the texture. aoTarget picks vertex colours or the colour atlas texels. Defaults to 0, off."},
            "aoRadius": {"type": "integer", "minimum": 0, "maximum": 8, "description": "Reach of the occlusion in cells beyond the four that touch a corner, as a cosine-weighted hemisphere, so deep crevices and overhangs darken more. 0 or 1 is the corner term alone. Defaults to 3."},
            "aoTarget": {"type": "string", "enum": ["auto", "vertex", "texture", "both"], "description": "Where aoStrength goes. vertex: a per-corner COLOR_0; faces whose corners shade differently are not merged, which costs triangles. texture: each cell's colour atlas texel is darkened by the mean of its corners and faces merge exactly as without occlusion; needs colorAtlas. both: the two at once, for a shader that uses one (a material multiplying texture by COLOR_0 darkens twice). auto (default): texture with colorAtlas, else vertex."}
        }), &["path"]),
        tool("export_variants", "Bake a scripted document once per parameter set and write <name>.vox and <name>.glb for each into dir, with the export_mesh mesh options, then a manifest JSON (default dir/variants.json) with each set's requested and resolved parameters, markers (local, world, glTF), bounds (voxel and glTF), quads, voxels, and mesh options. An existing manifest is updated by variant name: the sets exported now replace their entries in place, the other entries are kept in order, and new names are appended, so a subset can be redone; replace: true writes a fresh manifest with only these sets. The top-level options are present while every entry shares them. Parameters are keyed by name, or object/name when two objects share one, as in set_params. Every script runs per set, as regenerate does. The open document and its undo history are left as they were. One failing set stops the call with its name, before the manifest is written. Replies with the whole manifest, its path, and the names it added, replaced, and kept.", json!({
            "variants": {"type": "array", "minItems": 1, "maxItems": 64, "items": {"type": "object"}, "description": "1 to 64 sets: {name?, params}. name is a file stem of letters, digits, _, -, . (default variant_1, ...); params maps parameter names to values."},
            "dir": str_prop("Folder for the .vox, .glb, and manifest files. It is created."),
            "manifest": str_prop("Manifest path. Defaults to dir/variants.json."),
            "replace": {"type": "boolean", "description": "Write a fresh manifest with only these sets instead of merging into the existing one by name. Defaults to false."},
            "vox": {"type": "boolean", "description": "Write a .vox per set too. Defaults to true."},
            "timeoutSeconds": num_prop("Script deadline per object and set, up to 3600. Defaults to 300."),
            "merge": {"type": "boolean", "description": "As export_mesh. Defaults to false."},
            "vertexColors": {"type": "boolean", "description": "As export_mesh. Defaults to false."},
            "colorAtlas": {"type": "boolean", "description": "As export_mesh. Defaults to false."},
            "aoStrength": num_prop("As export_mesh, 0..1. Defaults to 0, off."),
            "aoRadius": int_prop("As export_mesh, 0..8. Defaults to 3."),
            "aoTarget": {"type": "string", "enum": ["auto", "vertex", "texture", "both"], "description": "As export_mesh. Defaults to auto."},
            "exportHidden": {"type": "boolean", "description": "As export_mesh. Defaults to false."},
            "splitObjects": {"type": "boolean", "description": "As export_mesh. Defaults to false."}
        }), &["variants", "dir"]),
        tool("export_models", "Write 1 to 16 .glb files and report each file's quads, triangles, and structure: nodes, meshes (vertices, indices, bounds), skins, and animations. A warning names an empty mesh, a skin with no joints, a nameless or duplicated node, or an animation with no channels. source opens that .vox in its own session, so the open document is not replaced. Omit source to export the open document. One model's failure is reported on that model and does not skip the others.", json!({
            "models": {"type": "array", "minItems": 1, "maxItems": 16, "description": "Each item is {path, source?, merge?, vertexColors?, exportHidden?, splitObjects?, colorAtlas?, aoStrength?, aoRadius?, aoTarget?}, with the export_mesh meanings. path ends in .glb. source, when set, is a .vox file.", "items": {"type": "object"}}
        }), &["models"]),
    ]
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    let [read_only, destructive, idempotent] = hints(name);
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        },
        "annotations": {
            "readOnlyHint": read_only,
            "destructiveHint": destructive,
            "idempotentHint": idempotent,
            "openWorldHint": false
        }
    })
}

/// Read-only, destructive, and idempotent hints for a tool. Destructive
/// means it may replace or remove cells, document state, or a file on disk;
/// a tool that writes to a caller's path may overwrite that file. Every tool
/// works on the session and local files, so none is open-world.
fn hints(name: &str) -> [bool; 3] {
    match name {
        "inspect_document" | "get_voxel" | "get_voxels" | "list_bones" | "list_clips"
        | "list_markers" | "get_script" | "list_params" | "ui_inspect" => [true, false, true],
        "set_active_object" | "set_view" => [false, false, true],
        "add_object" | "mirror_object" | "add_bone" | "add_clip" => [false, false, false],
        "remove_object" | "undo" | "redo" | "ui_click" | "regenerate" => [false, true, false],
        "new_document"
        | "open_file"
        | "save_file"
        | "set_voxel"
        | "set_voxels"
        | "fill_box"
        | "fill_ellipsoid"
        | "fill_rounded_box"
        | "fill_tube"
        | "carve"
        | "smooth"
        | "break_end"
        | "scatter"
        | "joints"
        | "column_relief"
        | "weather"
        | "moss"
        | "algae"
        | "vines"
        | "sweep"
        | "set_marker"
        | "remove_marker"
        | "set_palette"
        | "set_object"
        | "set_bone"
        | "remove_bone"
        | "bind_object"
        | "set_pose"
        | "clear_pose"
        | "set_clip_keys"
        | "remove_clip"
        | "render"
        | "model_view"
        | "preview"
        | "screenshot"
        | "screenshot_viewport"
        | "screenshot_region"
        | "set_script"
        | "set_params"
        | "export_mesh"
        | "export_models"
        | "export_variants" => [false, true, true],
        other => panic!("no annotation hints for tool `{other}`"),
    }
}

/// `exclude`: boxes a stone tool leaves alone.
fn exclude_prop() -> Value {
    json!({"type": "array", "items": {"type": ["integer", "array"]}, "description": "Boxes left alone: one inclusive box [x0, y0, z0, x1, y1, z1] or a list of up to 64, such as a ledge top, a cave floor, or a river bed (the solid top layer, the air above it, or both). No cell inside one is changed, and nothing grows from or lands in one, so a walking surface needs no refill afterwards."})
}

/// The arguments `moss`, `algae`, and `vines` share, plus `extra`.
fn growth_props(extra: &[(&str, Value)], amount: &str, faces: &str) -> Value {
    let mut p = json!({});
    for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
        p[key] = int_prop("Inclusive box corner");
    }
    p["palettes"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Plant palette indices. A single integer works too."});
    p["amount"] = num_prop(amount);
    p["noise"] = num_prop("0 grows in smooth patches, 1 in ragged speckle. Defaults to 0.5.");
    p["faces"] = json!({"type": ["string", "array"], "description": format!("Faces it grows on: top, side, bottom, all, or a list of them. Defaults to {faces}.")});
    p["facing"] = vec3_prop("Only faces whose outward normal points within 60 degrees of this direction, such as [0, -1, 0] for the front.");
    p["band"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Only cells with z in [low, high], such as a waterline band. algae thins from its bottom to its top and uses the box's height without one."});
    p["only"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Grow only on cells of these palette indices (the stone slots). Omit for every slot."});
    p["layer"] = json!({"type": "string", "enum": ["grow", "paint"], "description": "grow adds cells on the empty side of each face; paint recolours the surface. Defaults to grow, paint for algae. vines always grow."});
    p["exclude"] = exclude_prop();
    p["seed"] = int_prop("Seed. Defaults to 0.");
    for (key, value) in extra {
        p[*key] = value.clone();
    }
    p
}

fn mode_prop() -> Value {
    json!({"type": "string", "enum": ["replace", "empty", "paint"], "description": "replace (default) writes every cell of the shape; empty writes only empty cells, so existing detail stays; paint recolours only solid cells."})
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

fn view_prop() -> Value {
    json!({
        "type": "string",
        "enum": ["front", "back", "left", "right", "top", "bottom"],
        "description": "Named view of a model facing -Y with +Z up, instead of yaw and pitch. front is yaw -90, back 90, left 0, right 180 (the model's own sides), top pitch 90, bottom pitch -90."
    })
}

fn target_prop() -> Value {
    json!({
        "anyOf": [
            {"type": "string", "enum": ["all", "active"]},
            {"type": "integer"},
            {"type": "object", "properties": {"min": {"type": "array", "items": {"type": "integer"}, "minItems": 3, "maxItems": 3}, "max": {"type": "array", "items": {"type": "integer"}, "minItems": 3, "maxItems": 3}}, "required": ["min", "max"], "additionalProperties": false}
        ],
        "description": "What the camera fits: all (default, every visible posed cell), active (the active object), an object index, or {min, max}, an inclusive box of world cells. Other objects are still drawn."
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

/// The properties of two `json!` objects in one, for a tool with more
/// properties than one `json!` expands within the recursion limit.
fn merged(mut first: Value, second: Value) -> Value {
    if let (Some(map), Value::Object(more)) = (first.as_object_mut(), second) {
        map.extend(more);
    }
    first
}

/// MCP `preview` `lighting`. Its own function keeps `json!` within the
/// macro recursion limit.
fn lighting_prop() -> Value {
    json!({"type": ["string", "object"], "description": "A game's light instead of the viewport's: a preset name (viewport, the default; old_pond, frog-world's Old Pond region; or one from lightingPresets or the user's lighting_presets.json), or an object of lighting fields: sunDirection [x, y, z] toward the sun, Z up, or godotSunBasis (the 9 or 12 numbers of a DirectionalLight3D's Transform3D as the .tscn writes it); sunColor and ambientColor ([r, g, b] 0..16 sRGB, or #rrggbb); sunEnergy and ambientEnergy; exposure; background; lut (a 3D LUT PNG, N*N by N, applied last as Godot's colour correction); preset (the preset these fields go over). Shading is albedo x (sun x energy x max(n.l, 0) + ambient x energy) x exposure in linear light, clamped, with no shadows. Replies lighting with the values used."})
}

/// MCP `preview` `lightingPresets`.
fn lighting_presets_prop() -> Value {
    json!({"type": ["object", "string"], "description": "Lighting presets for this call: name to lighting fields, or the path of a JSON file of them (also under lighting). A built-in name is overridden field by field; a relative lut path is relative to the file."})
}

#[cfg(test)]
#[path = "../tests/mcp.rs"]
mod tests;
