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
                "instructions": "Edits apply to the active object, in that object's local cells; space world on the fills, carve, set_voxels, get_voxels, smooth, loft, brush, carve_from_views, project_paint, and split_object takes world cells instead and subtracts the object's position, so parts moved by a save and reopen or a mirror_object copy need no recomputed boxes. batch applies a list of editing tools in order as one undo step, stopping at the first error with the document as it was: use it instead of dozens of single fill_box and carve calls, and keep render and export calls outside it. Every tool that writes a file (save_file, render, model_view, preview, compare_silhouette, export_mesh, export_models, export_variants, bake_scene, screenshots) creates the missing folders of its path first. Export's bake option (with colorAtlas) paints ray-traced occlusion and a sun with shadows into the atlas, for a game to draw unshaded; preview with glb looks at the written file. bake_scene bakes a placement of many .vox files together, shaded by each other and by ground, into one merged glb and atlas; an instance's lean tilts it about its base, and its sway multiplies how far its moving slots sway. render path-traces a PNG in this process. preview writes a fast lit PNG (the viewport mesh and light, rasterised, well under a second) framed on the model or one object; use it to look after every step. Its preset game is the gameplay camera (a project redefines it with presets, an object or a JSON file, or the user's camera_presets.json), placementYaw sees an asset turned that much about up in a level from that camera, lighting (old_pond, or a project's lightingPresets) shows colour under a game's sun and ambient instead of the viewport light, overlay draws collision boxes, points, and connectors over the model, and inputs, variants, and reference make one labelled contact sheet of several models beside a concept image. Its previewTriangles is the unmerged mesh it draws, not the game's: its export counts (with export set to the game's options, e.g. colorAtlas) are what export_mesh writes. model_view writes the flat model view (palette colours, no light) for comparing with a turnaround sheet. set_view moves the window camera before screenshot_viewport. screenshot, screenshot_viewport, screenshot_region, ui_inspect, and ui_click need an editor started with --control and this server started with --connect. The window path-traces in Render mode. set_palette takes a slot's color as #rrggbb or #rrggbbaa (or r, g, b), so a concept's hex codes go in as they are. Hand sculpting (docs/hand-sculpting.md) models a creature or a unique asset without a script: block out with fill_box, fill_rounded_box, and fill_ellipsoid, grow limbs and roots with fill_tube, cut hollows with carve, soften with smooth, round a boxy blockout with a fill in mode intersect (only the cells inside the shape stay, so a hull becomes a dome), refine with set_voxels, draw seams, markings, and grooves that follow a curved surface with surface_stroke (points snap to the nearest surface cells), swap one palette slot for another (in a box or everywhere, no shape change) with recolor, and render after every step. clear_object empties an object of any size in one step (fill_box with palette 0 stops at 2000000 cells), to start a part again. For a unique organic asset with a mesh sculpted in Blender or made by an image-to-3D tool, import_mesh voxelizes the .glb, .gltf, or .obj (PNG, JPEG, or WebP textures and vertex colours quantized into the palette; fill solid fills a double-walled or open mesh, sealing its openings at their rims, so check interior and openEdges in the reply) as the blockout, and the tools below finish it. Organic forms (trunks, roots, limbs, rocks) should not read as pipes or boxes: loft builds a trunk or limb whose elliptical, flattened, knobbly section changes along a curve; deform bends, tapers, twists, or displaces (lumps, burls) an object or a box; brush inflates, deflates, smooths, flattens, or grabs the surface along a path of points to refine the silhouette (examples/fallen_tree.rhai). Stone surfaces: column_relief breaks a cliff face into columns of uneven width, relief, and height, joints cuts real joint gaps (columns, courses, cracks), weather chips edges, dents pits, and adds colour noise, and moss, algae, and vines grow plants on exposed faces (the caller gives the slots and their motion); moss style cushions grows separate cushions where the surface faces up, with drapes from overhangs, never one band. Wood surfaces: build the wood in one slot, then bark cuts plates along its grain (the same points as its fill_tube, or an axis on a flat deck or plank) split by fissures with dirt at the bottom, instead of painted stripes, and cavity_tint darkens fissures, crevices, and root crooks (and lightens ridges) by neighbour occupancy, making the shades itself; it gives rock depth too. examples/barked_trunk.rhai uses them. Decks, floors, and docks: planks lays separate boards over a rectangle at a height, with real gaps down to a dirt cell, end joints staggered between neighbouring rows, a tone per board, and optional ragged ends, nails, and knots, instead of dozens of fill_box and carve calls or painted stripes (examples/plank_dock.rhai). Tree crowns: foliage packs many separate leaf clumps over ellipsoids (or re-clusters an existing crown's slots) with lit tops, dark undersides, and a dark core in the gaps, leaving the wood alone, instead of one blob of fill_ellipsoid calls (examples/clustered_tree.rhai). Their exclude boxes (and scatter's, bark's, planks', and cavity_tint's) keep walking surfaces whole, with no refill afterwards. sweep fills a cross-section along a curve for curved walls, banks, and ledges, and with palette 0 erases one, for a slanted cut along a curved outline. trim_outline reshapes a finished form from outside: state a closed plan outline and [z, inset] pairs once, and every solid cell outside that surface goes, with a new skin along the cut if asked, so a hull narrows toward its keel with no tubes fitted by hand (examples/trimmed_hull.rhai). For a unique organic asset from a concept's turnaround sheet, carve_from_views fills the visual hull of its front, side, and top silhouettes (PNG crops) as the rough form, and with a depth map per view (depthPath: a grey depth pass of the sheet, white near, from an image generator or Depth Anything V2, calibrated to the hull by default) carves away the cells in front of each view's surface, so heads and domes come out round, project_paint then recolours the surface one view sees from a single image (a shell's plates from the top view, face markings from the front) without adding or removing cells, fit_to_views refines a blockout (from carve_from_views or import_mesh) so its orthographic views match the sheet's views (excess and missing rays, an optional reference object's depth, and colours from the object's slots), replying with each view's IoU and colour error before and after, and compare_silhouette scores the model against a concept view from a preview camera (IoU, missing, excess) and writes an overlay PNG; check it after every step instead of comparing heights. split_object then cuts that one blockout into its rig parts (head, flippers, tail): the cells inside boxes, or of some slots, move into a new or existing object and keep their world positions. After bind_object, place_joints puts each bone's pivot where its part meets its parent's part (contact faces, overlap, or the middle of the gap), so pivots are never placed by eye. set_marker names points, collision boxes, and connectors that the .vox keeps and the glb carries as nodes; collision_from_voxels builds the collision boxes from an object's solid cells within a tolerance (holeSize, layer or zBands, minFeature, only, exclude, maxBoxes), keeps hollows open, replaces its own earlier boxes, and replies with how much solid is covered and how much empty is wrongly filled, so colliders never drift from the model (check them with preview overlay boxes); export_variants writes a .vox and .glb per parameter set and merges them by name into a manifest of parameters, markers, and bounds (replace: true starts it fresh). examples/gorge_wall.rhai uses them all. A Rhai script (set_script) can call the same sculpt, voxel, object, rig, clip, and palette tools, with the MCP options in a trailing map, for a family of organic pieces (docs/scripting.md, examples/hollow_log.rhai)."
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
    if let Err(err) = known_arguments(name, &args).and_then(|()| batch_arguments(name, &args)) {
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
    known_arguments_in(&tools(), name, args)
}

/// Check every item of a `batch` the way a call of its own is checked, so a
/// misspelt option in the last item refuses the batch before the first runs.
fn batch_arguments(name: &str, args: &Value) -> Result<(), String> {
    if name != "batch" {
        return Ok(());
    }
    let tools = tools();
    for (index, item) in remote::batch_items(args)?.iter().enumerate() {
        known_arguments_in(&tools, item.tool, item.arguments)
            .map_err(|err| format!("batch item {index} ({}): {err}", item.tool))?;
    }
    Ok(())
}

fn known_arguments_in(tools: &[Value], name: &str, args: &Value) -> Result<(), String> {
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
        "batch" => "voxel.batch",
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
        "recolor" => "voxel.recolor",
        "surface_stroke" => "voxel.surface_stroke",
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
        "trim_outline" => "voxel.trim_outline",
        "deform" => "voxel.deform",
        "loft" => "voxel.loft",
        "brush" => "voxel.brush",
        "carve_from_views" => "voxel.carve_from_views",
        "project_paint" => "voxel.project_paint",
        "fit_to_views" => "voxel.fit_to_views",
        "compare_silhouette" => "render.compare_silhouette",
        "import_mesh" => "mesh.import",
        "bark" => "voxel.bark",
        "planks" => "voxel.planks",
        "cavity_tint" => "voxel.cavity_tint",
        "foliage" => "voxel.foliage",
        "set_marker" => "marker.set",
        "remove_marker" => "marker.remove",
        "list_markers" => "marker.list",
        "collision_from_voxels" => "marker.collision",
        "get_voxels" => "voxel.get_box",
        "set_palette" => "palette.set",
        "set_active_object" => "object.set_active",
        "add_object" => "object.add",
        "set_object" => "object.set",
        "remove_object" => "object.remove",
        "mirror_object" => "object.mirror",
        "split_object" => "object.split",
        "clear_object" => "object.clear",
        "add_bone" => "rig.add",
        "set_bone" => "rig.set",
        "place_joints" => "rig.joints",
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
        "bake_scene" => "mesh.bake_scene",
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
        tool("batch", &format!("Apply a list of editing tools to the active object and the palette in order, as one undo step, instead of one call per board or cut. Stops at the first error, says which item failed (items count from 0), and leaves the document as it was with no undo step added. The whole list is checked first: an unknown option or a tool that cannot run here refuses the batch before anything changes. At most {} items; each takes what its tool takes. Replies with items, changed (the sum over the items), voxels, and results, each item's tool and counts in short form. Tools that write files are refused, as are readers, undo, and anything that changes the active object or the document: call set_active_object first and save afterwards.", remote::MAX_BATCH_ITEMS), json!({
            "operations": {"type": "array", "minItems": 1, "maxItems": remote::MAX_BATCH_ITEMS, "description": "The items in order. Each is {tool, arguments}, arguments as the tool itself takes them.", "items": {"type": "object", "properties": {
                "tool": {"type": "string", "enum": remote::batch_tool_names(), "description": "The tool to run."},
                "arguments": {"type": "object", "description": "The tool's arguments."}
            }, "required": ["tool"], "additionalProperties": false}}
        }), &["operations"]),
        tool("new_document", "Replace the session with an empty one-object document and the default palette.", json!({}), &[]),
        tool("open_file", "Open a .vox file, replacing the session document. By default replies with path, objectCount, voxels, world bounds, activeObject, canUndo, canRedo, bones (count), paletteSlots (used slots), motionSlots, the first 20 objectNames, and moreObjects (how many names were left out). A Froxel file opens in the frame it was saved in, older ones with a script or markers included; a foreign file opens centred and floored. When world positions differ from the file's node positions, the reply states it as sceneShift and sceneShiftNote, as inspect_document and the exports do. detail full replies with the inspect_document result instead.", json!({
            "path": str_prop("Path of a .vox file"),
            "detail": {"type": "string", "enum": ["summary", "full"], "description": "summary (default) is the short reply; full is the inspect_document result."},
            "fullPalette": {"type": "boolean", "description": "With detail full, list all 256 palette slots, not only the used ones. Defaults to false."}
        }), &["path"]),
        tool("save_file", "Write every non-empty object as a .vox scene. World voxel and marker positions survive reopening, including negative coordinates, so a reopened document exports the same glTF frame and marker values; local cells may rebase to the model box. Replies with voxels (solid cells in the file) and objects (models written).", json!({"path": str_prop("Where to write. Omit to overwrite the open file. Its folder is created.")}), &[]),
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
            p["palette"] = int_prop("Palette index. 0 erases. Required except with mode intersect.");
            p["mode"] = fill_mode_prop();
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1"]),
        tool("get_voxels", "List non-empty local cells of the active object inside an inclusive box. At most 100000.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner");
            }
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1"]),
        tool("set_palette", "Replace one palette slot, or many via entries, as one undo step. The colour is color, \"#rrggbb\" or \"#rrggbbaa\" as the other tools take colours (or [r, g, b, a?]), or r, g, b (and a) as numbers; not both. motion is none, sway, or bob.", json!({
            "index": int_prop("Palette slot 0..255"),
            "color": {"type": ["string", "array"], "items": {"type": "integer"}, "description": "\"#rrggbb\" or \"#rrggbbaa\" (the # may be left out), or [r, g, b] or [r, g, b, a] in 0..255. Instead of r, g, b."},
            "r": int_prop("Red 0..255"),
            "g": int_prop("Green 0..255"),
            "b": int_prop("Blue 0..255"),
            "a": int_prop("Alpha 0..255. Defaults to 255, or the alpha of a #rrggbbaa color."),
            "motion": str_prop("none, sway, or bob. Omit to leave the slot's motion."),
            "motion_amount": num_prop("Motion strength, 0..4. Omit to leave it. Default 1 when a kind is first set."),
            "entries": {"type": "array", "description": "Batch of {index, color | r,g,b, a?, motion?, motion_amount?}, such as {\"index\": 3, \"color\": \"#5c6838\"}. Omit when passing index."}
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
            "palette": int_prop("Palette index. 0 erases. Required except with mode intersect."),
            "mode": fill_mode_prop()
        }), &["x0", "y0", "z0", "x1", "y1", "z1"]),
        tool("fill_rounded_box", "Fill an inclusive local box with rounded edges. radius is the bevel, clamped to half the shortest side. radius 0 fills the whole box. Same limits and one undo step as fill_box.", json!({
            "x0": int_prop("Inclusive minimum x"),
            "y0": int_prop("Inclusive minimum y"),
            "z0": int_prop("Inclusive minimum z"),
            "x1": int_prop("Inclusive maximum x"),
            "y1": int_prop("Inclusive maximum y"),
            "z1": int_prop("Inclusive maximum z"),
            "palette": int_prop("Palette index. 0 erases. Required except with mode intersect."),
            "radius": num_prop("Edge bevel in voxels. 0 fills the box."),
            "mode": fill_mode_prop()
        }), &["x0", "y0", "z0", "x1", "y1", "z1", "radius"]),
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
            "mode": fill_mode_prop()
        }), &["points"]),
        tool("recolor", "Replace palette slot from with slot to (or every pair of a map) in the active object's solid cells, in an inclusive box or the whole object, with no shape change: a blockout slot becomes the shell slot, two shades merge, a belly is re-tinted. Cells of other slots and empty cells stay. to 0 erases those cells. A slot is mapped once; the map is applied to the cells as they were, so 3 to 4 and 4 to 3 swap. One undo step. Replies with matched (cells of a mapped slot), changed, and voxels.", json!({
            "from": int_prop("Palette slot to replace, 1..255. Pass from and to, or map."),
            "to": int_prop("Palette slot it becomes, 0..255. 0 erases."),
            "map": {"type": ["object", "array"], "description": "Several at once: {\"3\": 5, \"4\": 6} or [[3, 5], [4, 6]]."},
            "x0": int_prop("Optional inclusive box corner; all six or none (the whole object)"),
            "y0": int_prop("Inclusive box corner"),
            "z0": int_prop("Inclusive box corner"),
            "x1": int_prop("Inclusive box corner"),
            "y1": int_prop("Inclusive box corner"),
            "z1": int_prop("Inclusive box corner")
        }), &[]),
        tool("surface_stroke", "Paint a stroke through points that lands on the active object's surface, for seams between shell plates, markings on a curved body, and grooves, instead of a fill_tube in paint mode at a radius that may miss the surface. The path (straight, or a smooth curve) is sampled every half cell; each sample snaps to the nearest surface cell (a solid cell with an empty face neighbour) within reach, so points may float above or sink below the surface. The solid cells within width / 2 of each snapped cell and at most depth cells into the solid (1 is the surface alone) take palette; palette 0 erases them, cutting a groove that deep. only limits it to cells of those slots. Nothing is added. One undo step. Replies with cells, changed, samples, missed (samples with no surface within reach), snap (the farthest a sample moved), length, and voxels.", json!({
            "points": {"type": "array", "description": "2..=1024 points [x, y, z] or {x, y, z} in the active object's local cells (a whole number is the centre of that cell); numbers may be fractional."},
            "palette": int_prop("Slot the stroke paints, 1..255, or 0 for a groove."),
            "curve": {"type": "string", "enum": ["polyline", "smooth"], "description": "polyline (default) or smooth (Catmull-Rom through the points)."},
            "width": num_prop("Width across the surface in cells, 0.5..=32. Defaults to 1, a one-cell line."),
            "depth": int_prop("Cells into the solid, 1..=16. Defaults to 1, the surface alone."),
            "reach": num_prop("How far a sample may be from the surface cell it snaps to, 0..=64. Defaults to 8."),
            "only": {"type": ["integer", "array"], "items": {"type": "integer"}, "description": "Only cells of these slots change, such as the shell slots, so a seam stops at the skin."}
        }), &["points", "palette"]),
        tool("carve", "Remove the solid cells inside a box, ellipsoid, rounded_box, or tube of the active object, for hollows, tunnels, and pits (to cut a polygon or rounded rectangle along a path, closed or open, call sweep with palette 0; to remove everything outside a surface, call trim_outline). box, ellipsoid, and rounded_box take x0..z1 (and radius for rounded_box) exactly as their fill tools; tube takes points, radius, and curve as fill_tube. Same 2000000-cell limits, checked before any cell changes; a tube also has at most 512000000 cells in the boxes around its segments, and a closed path counts like an open one. One undo step. Replies with removed.", json!({
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
        tool("moss", "Grow moss on the exposed faces of an inclusive box of the active object that pass the face filter. style caps (the default): cushions on tops (where the top face passes), moss on undersides when faces takes bottom, and curtains running down side faces from a top edge, each with a ragged end; coverage comes in soft patches mixed with speckle by noise. style cushions: separate irregular cushions and patches of size cells where the surface faces up (its normal within exposure of up, or of facing), kept gap cells apart until amount of those cells is covered, so moss never forms one continuous band (facing [0, 0, -1] with layer paint shades undersides, such as a tree crown's); in layer grow each cushion is domed up to thickness at its middle and drapes run from its edge down the side and hang free below an overhang. layer grow adds the moss on the empty side of each face (only empty cells are written); paint recolours the surface cells. palettes are picked in patches (per cushion), darker and lighter moss. Cut joints, bark, and weather first. Deterministic for a seed. A box of at most 2000000 cells, at most 400000 cells written. One undo step. Replies with added, painted, and curtains, or cushions and drapes, and warning (why) when nothing grew.", merged(growth_props(&[
            ("curtains", num_prop("caps: share of moss-capped top edges a curtain runs down from (any top edge when faces leaves out top), 0..=1. Defaults to 0.3. 0 grows caps only.")),
            ("length", json!({"type": "array", "items": {"type": "integer"}, "description": "Curtain length range [min, max] in cells, 1..=64 (caps), or drape length, 1..=32 (cushions), or one number. Defaults to [2, 8] for caps and [2, 6] for cushions."})),
            ("thickness", int_prop("Cushion thickness, 1..=3 on caps tops, 1..=4 at a cushion's middle (layer grow). Defaults to 1 for caps and 2 for cushions.")),
        ], "Coverage, 0..=1: of the passing faces (caps), or of the exposed cells (cushions). Defaults to 0.5 for caps and 0.45 for cushions.", "top and side"), json!({
            "style": {"type": "string", "enum": ["caps", "cushions"], "description": "caps (default): caps and curtains. cushions: separate cushions on exposed cells with drapes from overhangs."},
            "size": {"type": "array", "items": {"type": "integer"}, "description": "cushions: cushion radius range [min, max] in cells, 1..=16, or one number. Defaults to [2, 5]."},
            "gap": int_prop("cushions: fewest cells between two cushions, 1..=8. Defaults to 2."),
            "exposure": num_prop("cushions: how far a cell must face up (or facing) to take moss, the cosine of its surface normal with that direction, -1..=1. 0 takes the upper half of a log, 0.7 only its crown. Defaults to 0.35."),
            "drapes": num_prop("cushions: share of cushion edges over a drop that a drape hangs from, 0..=1 (layer grow). Defaults to 0.35. 0 hangs none.")
        })), &["x0", "y0", "z0", "x1", "y1", "z1", "palettes"]),
        tool("algae", "Paint algae on the exposed faces of an inclusive box of the active object inside a height band, usually the waterline: dense at the bottom of the band and thinning upward in noisy clumps with stone peeking through. palettes go from the bottom of the band to its top (a darker tone low, a lighter one high). Deterministic for a seed. A box of at most 2000000 cells. One undo step. Replies with added and painted.", growth_props(&[], "Coverage at the bottom of the band, 0..=1. Defaults to 0.9.", "top and side"), &["x0", "y0", "z0", "x1", "y1", "z1", "palettes"]),
        tool("vines", "Hang vine strands from the top edges of an inclusive box of the active object: each strand starts beside an edge cell whose side passes the face filter and falls straight down through empty cells, wandering a little along the edge with noise, until its length or something solid below. palettes are graded from the anchor to the tip, so give them motion sway rising from the anchor (set_palette motion). blooms adds an occasional bloom cell beside a strand. Only empty cells are written. Deterministic for a seed. A box of at most 2000000 cells, at most 400000 cells written. One undo step. Replies with added, strands, and blooms.", growth_props(&[
            ("length", json!({"type": "array", "items": {"type": "integer"}, "description": "Strand length range [min, max] in cells, 1..=128, or one number. Defaults to [4, 16]."})),
            ("blooms", json!({"type": "array", "items": {"type": "integer"}, "description": "Palette indices of blooms beside the strands. Omit for none."})),
            ("bloomChance", num_prop("Chance of a bloom beside each strand cell, 0..=1. Defaults to 0.06.")),
        ], "Share of top edge cells a strand starts from, 0..=1. Defaults to 0.08.", "side"), &["x0", "y0", "z0", "x1", "y1", "z1", "palettes"]),
        tool("bark", "Cut bark into the wood in an inclusive box of the active object, along the grain: the direction is an axis (x, y, z, or a vector, through the middle of the solid cells) or the local direction of points, the same points (and curve) as the fill_tube or sweep that made the trunk, branch, or root. The surface is split into long plates of varied width (around the wood) and length (along it), separated by fissures cut depth cells into the wood, the cell at the bottom of each taking the dirt slot. Plate ends sit at other heights than the next column's, as on a real trunk, and cracks share of the plates have a crack part way across. Each plate takes one of palettes, so the tone changes plate by plate instead of in stripes; end grain (the cut end of a log) and the inside of a hollow are left alone, and a hollow's inner wall is never cut through. A flat or gently curved plate (a deck, a plank, a wall; its section at least three times as wide as deep) gets columns of width across its surface from rim to rim; one too thin to cut from both faces is cut from its free face (the upper one when both are free) and never through, and one cell thick it only takes the plate tones. Use it instead of painted stripes. Then cavity_tint the box to shade the fissures, and grow moss. Deterministic for a seed. A box of at most 2000000 cells. One undo step. Replies with plates, removed, dirtied, painted, and cracks, and warning (why) when a shape could not be cut.", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner");
            }
            p["axis"] = json!({"type": ["string", "array"], "description": "The grain along a straight axis: x, y, z, or a vector [x, y, z]. Pass this or points."});
            p["points"] = json!({"type": "array", "items": {"type": ["array", "object"]}, "description": "The grain along a path: [x, y, z] points, or the fill_tube ([x, y, z, radius]) or sweep ([x, y, z, twist, scale]) points of the wood, whose extra numbers are ignored. Pass this or axis."});
            p["curve"] = str_prop("With points: polyline (default) or smooth, as the tube used.");
            p["palettes"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Plate slots, a few bark tones within a few units of each other; each plate takes one."});
            p["dirt"] = int_prop("Palette index for the cell at the bottom of each fissure, a dark brown. Omit to keep its colour.");
            p["width"] = json!({"type": "array", "items": {"type": "number"}, "description": "Plate width range [min, max] in cells around the wood, or across a flat plate, 1..=64, or one number. Defaults to [3, 6]."});
            p["length"] = json!({"type": "array", "items": {"type": "number"}, "description": "Plate length range [min, max] in cells along the grain, 2..=256, or one number. Defaults to [8, 20]."});
            p["depth"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Fissure depth range [min, max] in cells, 1..=4, or one number. Plate ends and cracks take the minimum. Defaults to [1, 2]."});
            p["cracks"] = num_prop("Share of plates with a crack part way across, 0..=1. Defaults to 0.25.");
            p["only"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Act only on cells of these palette indices (the wood). Omit to act on every slot."});
            p["exclude"] = exclude_prop();
            p["seed"] = int_prop("Seed. Defaults to 0.");
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1", "palettes"]),
        tool("planks", "Lay a deck, floor, or dock of separate boards over a rectangle of the active object at height z, along an axis, instead of dozens of fill_box and carve calls or painted stripes. Rows of random width tile the rectangle across the axis; each row is split into boards of random length, with end joints kept clear of those of the neighbouring rows. Gaps (between rows and at end joints) are empty down through the boards, with a dirt cell under them. Each board takes one tone, never that of the board before it in the row and as seldom as it can that of the boards beside it, and a row with room for a joint always has one, so no layout runs as one uniform stripe. A row's first and last boards are sawn pieces, so they may be shorter than length. Optional ragged ends, and nails and knots in the dark slot. Solid or empty, every cell of the rectangle takes a board or a gap unless only limits it, so to fit a curved hull fill its deck in one slot and pass that slot as only. The same seed gives the same boards. One undo step. Replies with rows, boards, joints, added, painted, removed, dirtied, nails, knots, changed, and voxels.", {
            let mut p = json!({});
            for key in ["x0", "y0", "x1", "y1"] {
                p[key] = int_prop("Inclusive corner of the rectangle");
            }
            p["z"] = int_prop("The top layer of the boards: they run from z down through thickness layers, and the dirt cell lies under them.");
            p["axis"] = json!({"type": "string", "enum": ["x", "y"], "description": "The way the boards run."});
            p["palettes"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Board tones, at least two different slots, ideally three or more within a few units of each other; each board takes one."});
            p["dirt"] = int_prop("Palette index of the cell under each gap, a dark brown. Omit to leave that cell as it is, so a gap over nothing is open.");
            p["dark"] = int_prop("Palette index of nails and knots. Omit for neither.");
            p["width"] = json!({"type": ["array", "number"], "items": {"type": "integer"}, "description": "Board width range across the axis in cells [min, max], 1..=64. Defaults to [3, 6]."});
            p["length"] = json!({"type": ["array", "number"], "items": {"type": "integer"}, "description": "Board length range along the axis in cells [min, max], 2..=256. Defaults to [8, 20]."});
            p["gap"] = json!({"type": ["array", "number"], "items": {"type": "integer"}, "description": "Gap width between boards in cells, a number or [min, max], 1..=8. Defaults to 1."});
            p["thickness"] = int_prop("Layers of board, the gaps run through them all, 1..=16. Defaults to 2.");
            p["ragged"] = num_prop("Chance per cell across a board's end that it is cut back 1 or 2 cells, 0..=1, about 0.3 for old wood. Defaults to 0: square ends.");
            p["nails"] = num_prop("Share of boards with a dark nail near each end, 0..=1, with dark. Defaults to 0.25.");
            p["knots"] = num_prop("Share of boards with a dark knot along the middle, 0..=1, with dark. Defaults to 0.15.");
            p["only"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Act only on solid cells of these palette indices, such as a deck already filled to a curved outline. Omit to lay boards over the whole rectangle."});
            p["exclude"] = exclude_prop();
            p["seed"] = int_prop("Seed. Defaults to 0.");
            p
        }, &["x0", "y0", "x1", "y1", "z", "axis", "palettes"]),
        tool("cavity_tint", "Shade depth into the exposed cells of an inclusive box of the active object without hand painting: each cell counts the solid cells in a ball of radius around it, a curvature or ambient occlusion estimate. Cells in concave spots (fissures, crevices, the crook of a root, the foot of a wall) more than dark take a darker slot; with lighten, cells on convex ridges and corners more than light take a lighter one. pairs names the slots ([slot, darker] or [slot, darker, lighter]); without pairs each slot gets shades of its own colour (darken and brighten times it, with its material) in free palette slots (from 255 down, never one a cell uses), reused on later calls. Run it after bark, joints, or weather and before moss. A box of at most 2000000 cells. One undo step with the palette. Replies with darkened, lightened, and shades (the slots it made).", {
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner");
            }
            p["radius"] = int_prop("Radius of the ball counted around each cell, 1..=4. Larger finds broader hollows. Defaults to 2.");
            p["dark"] = num_prop("How concave a cell must be to darken, 0..=1: 0 a flat face, 1 a closed pocket. Defaults to 0.12.");
            p["lighten"] = json!({"type": "boolean", "description": "Also lighten exposed ridges and corners. Defaults to false."});
            p["light"] = num_prop("With lighten: how convex a cell must be to lighten, 0..=1: 0 a flat face, 1 a lone spike. Defaults to 0.25.");
            p["pairs"] = json!({"type": "array", "items": {"type": "array", "items": {"type": "integer"}}, "description": "[slot, darker] or [slot, darker, lighter] per slot (0 leaves that tint out). Slots without a pair stay. Omit to make shades."});
            p["darken"] = num_prop("Without pairs: the darker shade's colour as a factor of the slot's, 0.05..1. Defaults to 0.72.");
            p["brighten"] = num_prop("Without pairs: the lighter shade's colour as a factor of the slot's, above 1 up to 4. Defaults to 1.18.");
            p["only"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Act only on cells of these palette indices. Omit to act on every slot."});
            p["exclude"] = exclude_prop();
            p
        }, &["x0", "y0", "z0", "x1", "y1", "z1"]),
        tool("foliage", "Build a clustered broadleaf tree crown on the active object, as pixel-art trees draw one: many separate leaf clumps with sunlit tops, darker undersides, and dark gaps between them, instead of one smooth blob. The crown volume is ellipsoids and boxes, or from (the existing cells of some slots, to re-cluster a crown already sculpted; those no clump keeps are removed), or both. Clump centres are packed over the volume's surface with Poisson-like spacing (no two closer than their sizes and gap allow); each clump is a flattened ellipsoid with a ragged, leafy outline that bulges out of the surface. The volume one cell under its surface fills with the core slot, so the gaps show dark and nothing is see-through. Each clump is shaded as one piece: its height in the crown and the way the crown faces the sun there set its tone, inside it the top lightens and the underside darkens along its own curve, and a darker seam marks where two clumps meet; no colour is per-cell noise. masses groups the clumps into lobes split by deeper grooves. Only empty cells and cells of the crown's own slots (palettes, core, from) are written, never protect slots: the trunk and branches stay whole and show through the gaps; exclude boxes keep a space open (in front of the trunk). Palette slots keep their motion, so give the leaf slots and the core the same sway. Writes stay inside the box, or without one inside the shapes and from cells grown by room for the clumps. Deterministic for a seed. A box of at most 2000000 cells, at most 4096 clumps. One undo step. Replies with clumps, masses, added, recoloured, removed, and core.", merged({
            let mut p = json!({});
            for key in ["x0", "y0", "z0", "x1", "y1", "z1"] {
                p[key] = int_prop("Inclusive box corner the writes stay inside. Omit all six to bound the crown by its volume.");
            }
            p["palettes"] = json!({"type": "array", "items": {"type": "integer"}, "description": "1 to 8 leaf slots from the darkest to the lightest: shade, mid, light, sunlit tips."});
            p["ellipsoids"] = json!({"type": "array", "items": {"type": "array", "items": {"type": "number"}}, "description": "Crown volume: [x, y, z, radius] or [x, y, z, rx, ry, rz] per ellipsoid, up to 64 shapes with boxes. A broad middle and a few lobes read as a tree."});
            p["boxes"] = json!({"type": "array", "items": {"type": "array", "items": {"type": "integer"}}, "description": "Crown volume: inclusive [x0, y0, z0, x1, y1, z1] boxes."});
            p["from"] = json!({"type": "array", "items": {"type": "integer"}, "description": "Crown volume: the existing cells of these slots (in the box). Re-clusters a crown already sculpted; its cells no clump or core keeps are removed."});
            p["core"] = int_prop("Palette index of the dark core inside the crown, seen in the gaps. Defaults to the first of palettes.");
            p["size"] = json!({"type": "array", "items": {"type": "number"}, "description": "Clump radius range [min, max] in cells, 1..=32, or one number. Defaults to [3, 6]."});
            p["gap"] = num_prop("Extra cells between neighbouring clumps, 0..=8. Larger shows more dark core between them. Defaults to 1.");
            p["flatten"] = num_prop("Clump height over width, 0.2..=1.5; below 1 clumps are wider than tall. Defaults to 0.7.");
            p["ragged"] = num_prop("Leafy outline noise of each clump, 0..=1. 0 is a smooth ellipsoid. Defaults to 0.4.");
            p
        }, json!({
            "light": num_prop("How strongly sunlit tops lighten toward the last palette, 0..=2. Defaults to 1."),
            "dark": num_prop("How strongly undersides and seams darken toward the first palette, 0..=2. Defaults to 1."),
            "sun": json!({"type": "array", "items": {"type": "number"}, "description": "Direction toward the sun [x, y, z], Z up. Defaults to [-0.35, -0.45, 1], high at the front left."}),
            "masses": int_prop("Lobes the clumps are grouped into, 0..=16, split by deeper grooves and each with its own tone. 0 (the default) is none."),
            "groove": int_prop("With masses: how many cells deeper the grooves between lobes go, 0..=8. Defaults to 2."),
            "protect": json!({"type": "array", "items": {"type": "integer"}, "description": "Slots never written, even when they are also crown slots. Any slot not in palettes, core, or from is already kept (the wood)."}),
            "exclude": exclude_prop(),
            "seed": int_prop("Seed. Defaults to 0.")
        })), &["palettes"]),
        tool("sweep", "Fill a 2D cross-section along a path of 2..=1024 local points, kept perpendicular to it, for curved walls, banks, ledges, and kerbs with no corners. The profile's u runs to the left of the path (seen from above, walking along it) and v up, so a wall profile stands on the path; a whole-number point is a cell centre. profile is a polygon of [u, v] points, or width (across, centred on the path), height (up from it), and radius (rounded corners) give a rounded rectangle. Each point may carry its own twist (degrees about the path) and scale, interpolated along it. curve smooth passes a Catmull-Rom curve through the points; extra slabs fill the outside of a bend. A cell is in when its centre is inside the profile at the nearest slab, so a flat top stays one flat layer. palettes lay out in bands (by height in the profile, strata), rings (along the path), or random. Writes as mode says. palette 0 erases instead: it removes the solid cells the sweep covers, for a slanted cut along a closed or curved outline (mode empty is refused with it). At most 2000000 cells. One undo step. Replies with cells and length, and removed when erasing.", json!({
            "points": {"type": "array", "items": {"type": ["array", "object"]}, "description": "Path points: [x, y, z], [x, y, z, twist], [x, y, z, twist, scale], or {x, y, z, twist?, scale?}. twist is degrees counter-clockwise looking along the path (default 0); scale multiplies the profile, 0.05..=16 (default 1)."},
            "profile": {"type": "array", "items": {"type": "array", "items": {"type": "number"}}, "description": "Closed polygon of 3..=256 [u, v] points in cells: u to the left of the path, v up. Omit to use width, height, and radius."},
            "width": num_prop("Rounded rectangle width across the path, centred on it, 0.5..=512. Used without profile."),
            "height": num_prop("Rounded rectangle height up from the path, 0.5..=512. Used without profile."),
            "radius": num_prop("Rounded rectangle corner radius, 0 up to half the smaller side. Defaults to 0."),
            "curve": str_prop("polyline (default) or smooth"),
            "palette": int_prop("Palette index, 0 to erase the solid cells it covers, or pass palettes"),
            "palettes": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices 1..=255 laid out by pattern"},
            "pattern": {"type": "string", "enum": ["bands", "rings", "random"], "description": "bands (default) by height v in the profile, period cells each, like strata; rings along the path; random per cell"},
            "period": num_prop("Band or ring size in cells, 0.5..=512. Defaults to 2."),
            "seed": int_prop("Seed of the random pattern. Defaults to 0."),
            "mode": mode_prop()
        }), &["points"]),
        tool("trim_outline", "Reshape the active object from outside: remove every solid cell outside a surface given as a closed plan outline and an inset per height, so a finished hull narrows toward its keel in one call. The surface at height z is the outline moved inward by inset(z); a cell stays when its centre is at least that far inside the outline. Use it when everything outside goes; sweep with palette 0 cuts a band along a path instead. skin lays a new skin of that slot along the surface, thickness cells inward, because the cut exposes whatever was inside: where the cut reached, solid cells are recoloured and a hollow is sealed, while a rim, an opening, or a part the cut missed stays as it was, and the object's box never grows. x0..z1 (all six or none) and only limit what is removed or skinned. At most 2000000 solid cells. One undo step. Replies with removed, painted, and added, and a warning when nothing was outside.", json!({
            "outline": {"type": "array", "items": {"type": "array", "items": {"type": "number"}}, "description": "The closed plan outline: 3..=1024 local [x, y] points in either winding, a whole number being a cell centre. Do not repeat the first point (a repeat is ignored)."},
            "inset": {"type": ["number", "array"], "items": {"type": "array", "items": {"type": "number"}}, "description": "How far inward the surface stands: one number for every height, or [z, inset] pairs going up in z, linear between them and the end value beyond them. A negative inset stands outside the outline. Defaults to 0."},
            "curve": str_prop("polyline (default) or smooth, a closed curve through the points"),
            "x0": int_prop("Box corner (optional; with all six, nothing outside the box is removed or written)"),
            "y0": int_prop("Box corner"),
            "z0": int_prop("Box corner"),
            "x1": int_prop("Box corner"),
            "y1": int_prop("Box corner"),
            "z1": int_prop("Box corner"),
            "only": {"type": "array", "items": {"type": "integer"}, "description": "Palette slots to remove or recolour; others stay. Defaults to every slot."},
            "skin": int_prop("Palette slot of a new skin along the surface. Omit for none."),
            "thickness": num_prop("The skin's thickness inward from the surface, 1..=64 cells, across a layer and into the layers above and below on a slope. Defaults to 1.")
        }), &["outline"]),
        tool("deform", "Reshape the active object, or the solid cells of an inclusive box of it (x0..z1, all six or none), so a trunk, root, limb, or rock stops reading as a pipe or a box. op bend curls the cells toward direction by angle degrees over length cells of the axis from origin, and carries the rest on straight; taper scales them across the axis from start at the origin to end at the end of the length; twist turns them about the axis by angle degrees over the length; displace pushes the surface in and out with fractal 3D noise of amplitude cells, bumps about scale cells wide, octaves finer layers, and bias (-1 only dents, 0 both, 1 only bulges), for burls, lumps, and knobs. axis is x, y, or z (default z); origin defaults to the cells' lowest along it, length to their extent, and center (the two other coordinates, in x, y, z order) to their mean. Each target cell takes the slot of the cell it came from, so palette slots are kept. Deterministic for a seed. At most 2000000 cells. One undo step. Replies with cells (read), result (left), added, and removed.", merged(json!({
            "op": {"type": "string", "enum": ["bend", "taper", "twist", "displace"], "description": "The deform."},
            "x0": int_prop("Region corner (optional; with all six only the box's cells change)"),
            "y0": int_prop("Region corner"),
            "z0": int_prop("Region corner"),
            "x1": int_prop("Region corner"),
            "y1": int_prop("Region corner"),
            "z1": int_prop("Region corner"),
            "axis": {"type": "string", "enum": ["x", "y", "z"], "description": "bend, taper, twist: the axis they run along. Defaults to z."},
            "origin": num_prop("bend, taper, twist: where along the axis they start, in local cells. Defaults to the cells' lowest."),
            "length": num_prop("bend, taper, twist: how far along the axis they run, 0.5..=4096. Defaults to the cells' extent."),
            "center": {"type": "array", "items": {"type": "number"}, "description": "bend, taper, twist: the line they work about, [a, b] as the two other coordinates in x, y, z order (for axis z, [x, y]). Defaults to the cells' mean."}
        }), json!({
            "angle": num_prop("bend: degrees, -180..=180 (negative bends away from direction). twist: degrees over the length."),
            "direction": {"type": "string", "enum": ["+x", "-x", "+y", "-y", "+z", "-z"], "description": "bend: the way the cells curl, across the axis. Defaults to +x for axis z, else +z."},
            "start": num_prop("taper: scale at the origin, 0.05..=16. Defaults to 1."),
            "end": num_prop("taper: scale at the end of the length, 0.05..=16. Defaults to 0.5."),
            "amplitude": num_prop("displace: the most the surface moves, 0.5..=32 cells. Defaults to 2."),
            "scale": num_prop("displace: bump size in cells, 1..=512. Defaults to 8."),
            "octaves": int_prop("displace: 1..=6 layers, each half the size and height. Defaults to 2."),
            "seed": int_prop("displace: noise seed. Defaults to 0."),
            "bias": num_prop("displace: -1 only dents, 0 dents and bulges (default), 1 only bulges.")
        })), &["op"]),
        tool("loft", "Fill a solid along a path of 2..=1024 local points whose cross-section changes from point to point, for trunks, roots, limbs, and branches that taper, bend, and are egg-shaped and knobbly in one call instead of a round fill_tube. Each point's section is an ellipse with rx (half-width across the path) and ry (half-height along the frame's normal, up for a level path), turned by roll degrees, cut flat by flatTop (0 none, 1 halves it at the path; negative flattens the bottom, for a log lying on the ground), with an outline noise of noise (a fraction of the radius, 0..=0.9) and about frequency lumps around. Sections run linearly between the points; a value a point leaves out takes the call's own (radius, rx, ry, roll, flatTop, noise, frequency). curve smooth passes a Catmull-Rom curve through the points; polyline (default) joins them straight. caps flat (default) cuts the ends square, round rounds them. A cell is in when its centre is inside the section at the nearest point of the path. Colour with palette, or palettes and a pattern as fill_tube: rings along the length every period cells, stripes in sectors around it, random per cell. seed picks the outline noise and the random pattern. Writes as mode says. At most 2000000 cells. One undo step. Replies with cells and length.", merged(json!({
            "points": {"type": "array", "items": {"type": ["array", "object"]}, "description": "Path points: [x, y, z], [x, y, z, r], [x, y, z, rx, ry], [x, y, z, rx, ry, roll], or {x, y, z, radius?, rx?, ry?, roll?, flatTop?, noise?, frequency?}. Radii 0.5..=128; a whole-number point is a cell centre."},
            "radius": num_prop("Default rx and ry of points that leave them out."),
            "rx": num_prop("Default half-width across the path."),
            "ry": num_prop("Default half-height. Defaults to rx."),
            "roll": num_prop("Default roll of the section about the path, in degrees. Defaults to 0."),
            "flatTop": num_prop("Default flat side, -1..=1: above 0 cuts the top at ry * (1 - flatTop), below 0 the bottom. Defaults to 0."),
            "noise": num_prop("Default outline noise as a fraction of the radius, 0..=0.9. Defaults to 0."),
            "frequency": num_prop("Default lumps around the outline, 0..=64. Defaults to 5.")
        }), json!({
            "curve": str_prop("polyline (default) or smooth"),
            "caps": {"type": "string", "enum": ["flat", "round"], "description": "flat (default) or round ends."},
            "palette": int_prop("Palette index, or pass palettes"),
            "palettes": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices laid out by pattern"},
            "pattern": {"type": "string", "enum": ["rings", "stripes", "random"], "description": "rings (default) along the length, stripes around it, random per cell"},
            "period": num_prop("Ring length in cells, 0.5..=512. Defaults to 2."),
            "sectors": int_prop("Stripes around, 1..=256. Defaults to the number of palettes."),
            "seed": int_prop("Seed of the outline noise and the random pattern. Defaults to 0."),
            "mode": mode_prop()
        })), &["points"]),
        tool("brush", "Sculpt the active object's surface with brush dabs along a path of points, like Blender's clay and grab brushes, to refine a silhouette after the rough form. op inflate pushes the surface out by up to strength x radius / 2 cells at each dab's centre; deflate (or dig) pushes it in the same way; smooth blurs solid and empty cells so bumps and pits melt (strength sets the passes); flatten cuts bumps and fills pits toward the mean plane of the surface under the dab; grab (or drag) moves the cells under each dab by vector cells times strength, with the falloff, so it pulls a branch stub out or bends a lip over. Each point is [x, y, z], [x, y, z, radius], or [x, y, z, radius, strength]; points leave out radius (default 3, 0.5..=64) and strength (0..=1, default 0.5, grab 1). Dabs are laid between points every spacing cells (default half the radius, 0 none; grab lays one per point), and each reads what the dabs before it left. falloff is smooth (default), linear, or constant. only limits the brush to cells of those slots: other cells never change and do not grow. Added cells take the nearest solid cell's slot, or palette. One undo step. Replies with dabs, added, and removed.", json!({
            "op": {"type": "string", "enum": ["inflate", "deflate", "dig", "smooth", "flatten", "grab", "drag"], "description": "The brush."},
            "points": {"type": "array", "items": {"type": ["array", "object"]}, "description": "Dab centres in local cells: [x, y, z], [x, y, z, radius], [x, y, z, radius, strength], or {x, y, z, radius?, strength?}."},
            "radius": num_prop("Default dab radius in cells, 0.5..=64. Defaults to 3."),
            "strength": num_prop("Default strength, 0..=1. Defaults to 0.5, or 1 for grab."),
            "falloff": {"type": "string", "enum": ["smooth", "linear", "constant"], "description": "How a dab fades to its radius. Defaults to smooth."},
            "only": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices the brush may change; other cells stay. Omit for all."},
            "palette": int_prop("Slot of added cells. Defaults to the nearest solid cell's."),
            "vector": vec3_prop("grab: [dx, dy, dz] cells a dab's centre moves at strength 1; at most the radius."),
            "spacing": num_prop("Most cells between dabs, 0..=256; 0 lays one per point. Defaults to half the radius.")
        }), &["op", "points"]),
        tool("carve_from_views", "Fill the visual hull of 2 to 6 orthographic views of a concept's turnaround sheet into the active object: each view's silhouette is extruded through a box along its depth axis and the cells every view covers are kept, so the rough form of a front, side, and top view becomes 3D in one call. With a depth map per view (depth, depthPath) the cells in front of each view's depth surface are carved away too (space carving with depth), so a round head or a dome comes out round instead of a box; one view with depth is enough for a relief. A view's depth is a grey depth map of the same crop (white near, black far; a PNG path, true for the crop of the top-level depthPath, or {path | values (rows of numbers 0..1, null background), crop (defaults to the view's), background (with a colour image every pixel holds a value and the silhouette is the colour image's; without one the map keys its background as a view does; none takes every pixel), invert (black near), gamma (value^gamma first; auto picks the curve whose calibration meets the silhouette contour, where a round body turns away at the hull's middle, for a map not linear in depth such as Depth Anything's), near and far (cells from the camera side of the box where white and black land), calibrate (hull, the default: each image row's nearest pixel is placed where the silhouette hull is nearest in that row, measured by the view looking along the row, so a sphere's rim lands on its middle plane; box: the map's range spans the box, for a relief, and what hull falls back to when no view measures the depth), back (mirror the surface about the hull's middle for the far side, when the opposite view has no map), regions (calibrate each part of the map on its own: one region per peak, apart where the grey jumps or dips between peaks by more than edge, default 0.06; for a map that normalises each part like shading, as an image generator draws it), detail (a second map, a path or {path | values, crop, invert, gamma, edge}: its regions give the curvature inside each part and this map their order and offsets, so a monocular model's map, right in order but flat inside each part, and a generated map, round but wrong in order, make one surface)}). Coloured pixels (a magenta background and its fringe) hold no depth, and a ray reads the median of its footprint, so outlines and specks do not move it. Depth is relative, from an image generator's depth pass or a monocular model such as Depth Anything V2 (grayscale output, white near): leave calibrate at hull and give the views the colour sheet's crops; gamma auto for Depth Anything, regions for a generated map, or Depth Anything as the map and the generated one as its detail; pass near and far only when the scale is known. A cell stays when its centre is at or behind every depth surface, less depthTolerance cells (default 0). Then shape it with carve, smooth, and set_voxels, and check it with compare_silhouette. Views are seen as model_view names them, for a model facing -Y with +Z up: front from -Y (image right is +X), back from +Y, left (also side) from +X (image right is +Y, so the model's front is on the image's left), right from -X, top from +Z (image down is -Y), bottom from -Z; flip mirrors an image. A view is a PNG path with a crop [x, y, width, height] in pixels (several views can share one sheet), or rows of characters with a legend. Its background rule: auto (default: transparent pixels if the crop has any, else the commonest colour of the image border within tolerance), alpha, or a key colour. cellsPerPixel (per view or for all) or size [across, down] maps a view to cells; size [x, y, z] sets the box and stretches any view without its own scale. A view smaller than the box is centred across and stands on the box's bottom. A cell is in a view when at least coverage (default 0.5) of its footprint is inked. Colour: each cell takes the pixel of the view whose visible surface is nearest, quantised to the nearest of slots (default every slot), or palette gives one slot. shell keeps only cells within that many cells of the outside (a hollow log); smooth runs that many smooth passes. Writes as mode says; mode intersect instead keeps only the object's solid cells inside the hull, as they are, and erases the rest, so a blockout is cut to the views without refilling it. At most 4000000 cells in the box. One undo step. A top-level path, background, tolerance, and alphaThreshold apply to every view without its own; a view with depth and no path or rows takes its silhouette from the depth map (then pass palette, or give another view a colour image). Replies with cells, bounds {min, max}, size, and each view's grid, covered cells, and depth {calibrate, near, far, gamma, rim, pairs and residual (cells) for hull, regions and regionResidual with regions or detail, carved}.", merged(json!({
            "views": {"type": "array", "minItems": 1, "maxItems": 6, "items": {"type": "object"}, "description": "2 to 6 views (1 with depth), one per side: {view: front | back | left | side | right | top | bottom, path (a PNG) | rows (strings; . and space are empty) + legend ({char: palette index or #rrggbb}), crop?: [x, y, width, height], cellsPerPixel? | size?: [across, down], background?: auto | alpha | #rrggbb | [r, g, b], tolerance? (RGB distance, default 40), alphaThreshold? (default 128), flip?, depth?: true | path | {path | values, crop, background, invert, gamma (number or auto), near, far, calibrate: hull | box, back, regions, edge, detail}}"},
            "path": str_prop("The sheet PNG for every view without its own path or rows."),
            "background": {"type": ["string", "array"], "description": "auto (default), alpha, #rrggbb, or [r, g, b], for every view without its own."},
            "depthPath": str_prop("A grey depth sheet (white near) laid out as the colour sheet: every view without depth false takes its depth from this image at the view's crop."),
            "depthTolerance": num_prop("Cells in front of a depth surface that still stay, 0..64. Defaults to 0."),
            "size": {"type": "array", "items": {"type": ["integer", "null"]}, "minItems": 3, "maxItems": 3, "description": "Box [x, y, z] in cells, 1..2048 each; null takes the largest view along that axis."},
            "origin": ivec3_prop("Local cell of the box's minimum corner. Defaults to [0, 0, 0]."),
            "cellsPerPixel": num_prop("Cells per image pixel for every view without its own size or cellsPerPixel, such as 0.25 for 4 pixels a cell."),
            "palette": int_prop("One palette index for every cell, instead of the views' colours."),
            "slots": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices image colours quantise to. Defaults to all of 1..255. A legend's palette index is used as it is."}
        }), json!({
            "coverage": num_prop("Share of a cell's footprint in a view that must be inked, 0..1. Defaults to 0.5."),
            "shell": int_prop("Keep only cells within this many cells of the outside, 1..64: a hollow shell. Omit for solid."),
            "smooth": int_prop("Passes of smooth over the hull, 0..64 (fillAt 17, removeAt 9). Defaults to 0."),
            "mode": fill_mode_prop()
        })), &["views"]),
        tool("project_paint", "Recolour the active object's existing surface from ONE image projected along a view direction, such as a shell's plate pattern from a top view or face markings from the front: cells are never added or removed. The image is a PNG path with a crop [x, y, width, height], or rows of characters with a legend, with the background rule of carve_from_views (auto, alpha, or a key colour; alpha on an opaque image inks every pixel); background pixels leave the surface alone. view is front, back, left (also side), right, top, or bottom, seen as model_view and carve_from_views name them (front from -Y with image right +X, left from +X with image right +Y, top from +Z with image down -Y); or yaw and pitch in degrees as preview takes them (yaw 0 looks from +X, -90 from the front, pitch 90 from above) for an oblique projection. flip mirrors the image. The box (origin and size [x, y, z], defaulting to the object's bounds) limits which cells are painted, and the image is placed on the box's projection as a carve_from_views view is placed in its box: stretched to it, or at cellsPerPixel or imageSize [across, down] cells, centred across and standing on the bottom (an oblique view is centred both ways). So the same sheet crop and cellsPerPixel used to carve lands on the same cells. Each inked grid cell casts parallel rays: depth (default 1) paints the solid cells within that many cells of the first one each ray meets, so nothing behind the visible surface changes; cells outside the box or the only slots still hide what is behind them. normalLimit (degrees, 0..90) skips a surface whose normal is further than that from the view direction, so steep sides get no streaks. Colours quantise to the nearest of slots (default every slot), a legend's palette index is used as it is, or palette gives one slot to every inked pixel (a mask). only limits repainting to cells of those slots. A cell under several pixels takes the commonest. One undo step. Replies with cells (painted), changed, bounds, origin, size, grid, covered (inked grid cells), rays (inked rays that met the object), and steep (rays skipped by normalLimit).", merged(json!({
            "view": {"type": "string", "enum": ["front", "back", "left", "side", "right", "top", "bottom"], "description": "The side the image looks from. Or pass yaw and pitch."},
            "yaw": num_prop("Degrees about +Z, as preview takes it: 0 looks from +X, -90 from the front (-Y). Instead of view."),
            "pitch": num_prop("Degrees above the horizon, -90..90; 90 looks straight down. Instead of view."),
            "path": str_prop("The image PNG."),
            "crop": {"type": "array", "items": {"type": "integer"}, "minItems": 4, "maxItems": 4, "description": "[x, y, width, height] of the image in pixels. Defaults to the whole image."},
            "rows": {"type": "array", "items": {"type": "string"}, "description": "The image as rows of characters instead of path; . and space are background."},
            "legend": {"type": "object", "description": "rows: {char: palette index or #rrggbb}."},
            "background": {"type": ["string", "array"], "description": "auto (default), alpha, #rrggbb, or [r, g, b]."},
            "tolerance": num_prop("RGB distance from the key colour that is still background, 0..442. Defaults to 40."),
            "alphaThreshold": num_prop("Alpha below this is background, 1..255. Defaults to 128."),
            "flip": {"type": "boolean", "description": "Mirror the image left to right."},
            "origin": ivec3_prop("Local cell of the box's minimum corner. Defaults to the object's."),
            "size": {"type": "array", "items": {"type": ["integer", "null"]}, "minItems": 3, "maxItems": 3, "description": "Box [x, y, z] in cells, 1..2048 each; null (or leaving it out) reaches the object's far side."},
            "cellsPerPixel": num_prop("Cells per image pixel, such as 0.25 for 4 pixels a cell. Defaults to stretching the image over the box."),
            "imageSize": {"type": "array", "items": {"type": "integer"}, "minItems": 2, "maxItems": 2, "description": "[across, down] cells the image spans, instead of cellsPerPixel."}
        }), json!({
            "palette": int_prop("One palette index for every inked pixel, instead of the image's colours."),
            "slots": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices image colours quantise to. Defaults to all of 1..255."},
            "coverage": num_prop("Share of a grid cell's footprint that must be inked, 0..1. Defaults to 0.5."),
            "depth": int_prop("Solid cells painted per ray from the visible surface, 1..64. Defaults to 1."),
            "normalLimit": num_prop("Degrees, 0..90: leave a surface whose normal is further than this from the view direction. Omit for no limit."),
            "only": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices that may be repainted; other cells keep their slot. Omit for all."}
        })), &[]),
        tool("fit_to_views", "Refine the active object so its orthographic views match an approved views sheet, with colours from a palette: the finishing pass after a carve_from_views or import_mesh blockout (inspired by Voxify3D, CVPR 2026; a CPU discrete search, no neural network). views are 1 to 6 views, one per side, given as carve_from_views takes them (path + crop + background rule, or rows + legend; flip mirrors); a top-level path, background, tolerance, and alphaThreshold apply to every view without its own. Rays are orthographic and parallel: one view pixel is one column of cells. Each crop is placed once over the object's current projection with compare_silhouette's align (fit by default: the scale and offset with the most overlap; height, width, or {scale, offset}, top level or per view), then sampled to one target per cell column (inside when coverage of its footprint is inked, and the mean colour). Per view pixel the energy is maskWeight when the target is inside and nothing covers it, or the target is background and a cell does; depthWeight * min(|d - dRef|, 8) / 8 against a reference object's front surface; colorWeight * min(RGB distance / 255, 1) between the first visible cell's slot and the target; plus smoothWeight per exposed cell face. Shape moves (shape, default on) run first on mask, depth, and smoothness only: excess rays are cleared, missing rays filled with cells every other view sees inside (a run, or one cell touching the body), a covered ray's front moved to the reference's, and rough single cells removed or filled; each kept only if it lowers the energy, in sweeps shuffled by seed until none is kept or iterations (default 8) run out. Then (recolor, default on) every cell a view sees takes the slot of slots (default: the object's own slots) nearest the colours of the views that see it and face it within normalLimit (default 60 degrees); each column's colour is the majority slot of its pixels, so thin outlines and cube bevels do not tint it (and lines under half a cell drop out). Views placed with fit share one scale (sharedScale). A view's color: false uses it for the shape only, and keep protects slots. Views of one feature drawn at different places on a boxy part (a face's eyes in the front and side views) give that feature twice: fit such a part with recolor false, or keep its face slots. Other visible objects cover and hide but never change unless isolate. reference (an object index or name, such as a mesh imported with import_mesh newObject) gives the depth target; a view's depth map (depth and depthPath as carve_from_views takes them, calibrated the same way against the hull of the views' targets) gives that view's depth target instead, so a blockout's front follows the sheet's depth pass. Deterministic, one undo step. Three axis views cannot see a round form's diagonal corners, so the cells they leave ambiguous may follow a rounding prior (roundWeight, off by default; 0.3 for a round head; roundness; roundAxes): superellipses inscribed in each cross-section, or the reference object when one is given, and cells outside it go wherever no silhouette needs them. Replies with views [{view, grid, align {scale, offset} (reuse it to keep the placement), before and after {iou, missing, excess, colorError, depthError with a reference or depth, pixels}, depth (its calibration)}], energy before and after, added, removed, recolored, changed, sweeps, moves, rounded, palette [{slot, cells}] after, origin and size of the box worked in, and voxels.", merged(json!({
            "views": {"type": "array", "minItems": 1, "maxItems": 6, "items": {"type": "object"}, "description": "1 to 6 views, one per side: {view: front | back | left | side | right | top | bottom, path (a PNG) | rows + legend, crop [x, y, width, height], background (auto, alpha, #rrggbb, [r, g, b]), tolerance, alphaThreshold, flip, align, color (false: shape only, its colours ignored), depth (as carve_from_views: true | path | {path | values, crop, background, invert, gamma, near, far, calibrate, back, regions, edge, detail})}."},
            "depthPath": str_prop("A grey depth sheet (white near) laid out as the colour sheet: every view without depth false takes its depth target from it at the view's crop."),
            "depthTolerance": num_prop("Cells in front of a depth surface that still count as on it, 0..64. Defaults to 0."),
            "path": str_prop("The sheet PNG for every view without its own path or rows."),
            "background": {"type": ["string", "array"], "description": "auto (default), alpha, #rrggbb, or [r, g, b], for every view without its own."},
            "tolerance": num_prop("RGB distance from the key colour that is still background, 0..442, for every view without its own. Defaults to 40."),
            "alphaThreshold": num_prop("Alpha below this is background, 1..255, for every view without its own. Defaults to 128."),
            "align": {"type": ["string", "object"], "description": "fit (default), height, width, or {scale, offset}: crop pixel p lands on cell column p * scale + offset. A view's own align overrides it."},
            "slots": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices the colours may take. Defaults to the slots the object holds."},
            "reference": {"type": ["integer", "string"], "description": "Another object (index or name) whose front surface is the depth target, such as a mesh imported with import_mesh newObject. Omit for none."},
            "isolate": {"type": "boolean", "description": "Fit the active object alone; by default other visible objects cover and hide but never change."}
        }), json!({
            "coverage": num_prop("Share of a cell column's footprint that must be inked, 0..1. Defaults to 0.5."),
            "iterations": int_prop("Most shape sweeps, 0..64; 0 only recolours. Defaults to 8."),
            "seed": int_prop("Shuffles the order rays are visited in. Defaults to 1."),
            "margin": int_prop("Empty cells around the bounds the fit may fill, 0..64. Defaults to 15% of the largest extent, at least 4."),
            "maskWeight": num_prop("Cost of a missing or excess view pixel, 0..100. Defaults to 1."),
            "colorWeight": num_prop("Cost of a colour 255 RGB apart, 0..100. Defaults to 1."),
            "depthWeight": num_prop("Cost of a first cell 8 or more cells from the reference's or the depth map's, 0..100. Defaults to 0.5."),
            "smoothWeight": num_prop("Cost of each exposed cell face, 0..100; keeps lone spikes and pits out. Defaults to 0.1."),
            "shape": {"type": "boolean", "description": "Add and remove cells. Defaults to true."},
            "recolor": {"type": "boolean", "description": "Recolour the cells the views see. Defaults to true. Off for a part with a hand-set face, so only its shape is fitted."},
            "normalLimit": num_prop("Degrees, 0..90: a view recolours only cells whose surface faces it within this angle (a seen cell no view faces takes the view facing it most), so a feature does not wrap round a corner. Defaults to 60."),
            "keep": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices whose cells are never removed or recoloured (hand-set eyes); they still cover and hide."},
            "roundWeight": num_prop("Cost of keeping a cell outside the rounding prior, 0..100; 0 keeps the maximal hull. Each cross-section along each axis gets the superellipse inscribed in its bounds (with a reference: the reference itself), and cells outside it that no axis view needs (a box's diagonal corners, which three axis silhouettes cannot see) are removed, so a round head comes out round while every silhouette stays. Defaults to 0 (the maximal hull); 0.3 suits a round part such as a head, not a flat-bottomed dome or a plate, which lose their rim."),
            "roundAxes": str_prop("Axes whose cross-sections the rounding prior rounds, letters of xyz. Defaults to yz: front sections and the plan; an x section runs along a creature facing -Y, through its neck."),
            "roundness": num_prop("Superellipse exponent of the rounding prior, 1..16: 2 is an ellipse, larger is squarer. Defaults to 2.5."),
            "sharedScale": {"type": "boolean", "description": "Views placed with fit share one scale (the median), offsets refitted at it, as a sheet is drawn at one scale. Defaults to true."}
        })), &["views"]),
        tool("compare_silhouette", "Measure how closely the model reads like its concept from one camera, as numbers and a picture: draws the preview silhouette (the camera of preview: preset game, presets, placementYaw, view, yaw, pitch, orthographic, fov, margin, object, isolate, pose) and compares it with a mask cut from a concept PNG (crop and background rule as in carve_from_views). The concept is placed over the preview with one uniform scale and an offset, so its proportions are kept: align fit (default) finds the placement with the most overlap, height or width match that extent with bottoms and centres lined up, and {scale, offset} gives it (concept pixel p lands on output pixel p * scale + offset; reuse a reply's align to hold the placement still between edits). Replies with iou, missing (share of the concept the model does not cover), excess (share of the model outside the concept), pixel counts, align, both bounds and aspect ratios, and writes an overlay PNG to path: the preview with the concept's outline in cyan, missing areas in magenta, excess in orange, and the numbers in the corner. width and height default to 512 tall at the concept crop's aspect. Writes a file, so Rhai scripts do not have it.", merged(json!({
            "path": str_prop("Overlay PNG to write. Its folder is created."),
            "concept": str_prop("Concept PNG path"),
            "crop": {"type": "array", "items": {"type": "integer"}, "minItems": 4, "maxItems": 4, "description": "[x, y, width, height] of the concept view in pixels. Defaults to the whole image."},
            "background": {"type": ["string", "array"], "description": "The concept's background: auto (default), alpha, #rrggbb, or [r, g, b]."},
            "tolerance": num_prop("RGB distance of a key colour, 0..442. Defaults to 40."),
            "alphaThreshold": num_prop("Alpha below which a pixel is background, 1..255. Defaults to 128."),
            "flip": json!({"type": "boolean", "description": "Mirror the concept left to right."}),
            "align": {"type": ["string", "object"], "description": "fit (default), height, width, or {scale, offset: [x, y]}."}
        }), json!({
            "preset": str_prop("Camera preset: game (orthographic, yaw -135, pitch 32: frog-world's gameplay camera, unless presets or the user's camera_presets.json redefines it), default, or one from presets."),
            "presets": camera_presets_prop(),
            "placementYaw": placement_yaw_prop(),
            "view": view_prop(),
            "yaw": num_prop("Degrees. 0 puts the camera on +X looking toward -X."),
            "pitch": num_prop("Degrees, -90..90."),
            "orthographic": json!({"type": "boolean", "description": "Orthographic, as a turnaround sheet is drawn."}),
            "fov": num_prop("Perspective field of view in degrees. Defaults to 50."),
            "margin": num_prop("Empty border on each side, 0..0.45. Defaults to 0.06."),
            "object": json!({"type": ["integer", "string"], "description": "Frame this object (index or name)."}),
            "isolate": json!({"type": "boolean", "description": "Draw only object."}),
            "width": int_prop("16..4096"),
            "height": int_prop("16..4096"),
            "antialias": json!({"type": "boolean", "description": "Defaults to true."}),
            "lighting": lighting_prop(),
            "lightingPresets": lighting_presets_prop(),
            "pose": json!({"type": "object", "description": "Bone name to quaternion, as in preview."})
        })), &["path", "concept"]),
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
        tool("collision_from_voxels", "Build collision box markers from an object's solid cells, for a game to read, instead of writing boxes by hand: a walkable deck, a hollow hold, and open holes, which one convex hull cannot give. Boxes are named <prefix>_1, <prefix>_2, ... and are ordinary box markers: the .vox saves them, export writes them as glTF nodes, list_markers lists them, and preview with overlay: \"boxes\" draws them. A rerun with the same prefix replaces that prefix's boxes on the object, so run it again whenever the shape changes. Hollows stay open: no box ever holds an empty block holeSize wide on each axis (clipped to the box, so a hatch through a one-cell deck counts). Surface dents and gaps smaller than that may be filled, and the reply says how much. Set holeSize a little under the smallest opening the player must use. A box's faces sit on its cells, so walking surfaces stay exact. When maxBoxes cannot be met without closing a hollow, the boxes with the least solid are dropped and the reply says so (dropped, warning). Coordinates are the object's local mesh units, and the boxes move with it. One undo step. Replies with boxes (the count), solidCells, ignoredBySlot, ignoredThin, targetCells (what the boxes aim to cover), coveredCells, missedCells, coveredPercent, boxCells, emptyFilledCells and emptyFilledPercent (wrongly filled empty cells, as a share of the boxes' volume), merged, dropped, replaced, prefix, and markers.", json!({
            "prefix": str_prop("Marker names are <prefix>_<number>. Default <object name>_collider. An existing marker with one of those names that this tool would not replace is an error."),
            "layer": {"type": "integer", "minimum": 1, "maximum": 64, "description": "Horizontal cuts between boxes lie on planes this many cells apart, from the lowest cell, unless only another height opens a hollow. Default 1, any height. Not with zBands."},
            "zBands": {"type": "array", "items": {"type": "integer"}, "description": "Horizontal cuts by hand: ascending z planes in local mesh units, at least two, such as [0, 3, 8, 12]. Put one on each deck and walking level. Not with layer."},
            "minFeature": {"type": "integer", "minimum": 1, "maximum": 32, "description": "Ignore details thin on two axes, such as vines and rope: a cell is covered only if it lies in a full square this wide in an axis plane. A deck or a wall thin on one axis stays; a one-cell diagonal does not hold such a square. Default 1, keep everything."},
            "holeSize": {"type": "integer", "minimum": 1, "maximum": 64, "description": "The tolerance: the smallest empty block that stays open. Default 3. Larger means fewer boxes and more empty volume filled; 1 fills nothing, an exact cover."},
            "only": {"type": "array", "items": {"type": "integer"}, "description": "Cover only cells of these palette indices. Omit for every slot."},
            "exclude": {"type": "array", "items": {"type": "integer"}, "description": "Palette indices to leave out, such as moss, vines, and rope. These are slots, not the boxes the stone tools take under this name."},
            "maxBoxes": {"type": "integer", "minimum": 1, "maximum": 4096, "description": "Most boxes. With boxes to spare the emptiest boxes split further; with too many, neighbours merge, cheapest added empty volume first, as far as holeSize allows, and then the boxes with the least solid are dropped. Omit to split only until no box holds a hollow."},
            "object": {"type": ["integer", "string"], "description": "Object index or name. Defaults to the active object."}
        }), &[]),
        tool("list_markers", "Markers with their kind, object, and local, world, and glTF (Y-up) coordinates: a point, a box's min, max, centre, and size, a connector's point, direction, and node rotation.", json!({}), &[]),
        tool("import_mesh", "Voxelize a .glb, .gltf, or .obj mesh (with its .mtl and textures) into the active object, or a new object with newObject, to bring a form sculpted in Blender or made by an image-to-3D tool into the voxel workflow. Every triangle primitive of the default glTF scene is read under its node transforms; embedded and external buffers and PNG, JPEG, and WebP textures (EXT_texture_webp, as image-to-3D tools write) are read; KTX2/Basis textures are skipped with a warning; Draco and meshopt compression are refused. The file's Y-up becomes Z-up (upAxis z keeps it). Size by size (cells along the longest axis, default 64), scale (cells per file unit), or voxelSize (metres per cell, so 0.08 keeps a glTF's real size at 0.08 m voxels). fill solid (default) fills every cell a flood from the box border cannot reach, so a hollow or double-walled mesh comes out solid; an open mesh (openEdges above 0: holes, seams, an open underside) first has its openings sealed by cells enclosed in at least 17 of 26 directions, so a turtle shell open underneath is filled up to its rim; parity is the older majority vote of ray parity along x, y, and z; shell writes only the cells the surface touches. Each surface cell takes the base-colour texture times the factor times the vertex colours at the nearest point of its nearest triangle; inside cells take the nearest surface colour. Colours within tolerance of a palette slot reuse it; the rest are reduced to at most maxNewSlots colours written into slots no cell uses, from 255 down; slots maps only to the slots given, and palette paints one slot. At most 256 cells per axis and 2000000 cells, refused before anything changes. One undo step. Replies with cells, shell, interior, openEdges (edges of one triangle once equal positions are welded; 0 is watertight), written, changed, slots, newSlots, bounds (min, max, size in local cells), scale, triangles, primitives, textures, and warnings.", json!({
            "path": str_prop("Path of the .glb, .gltf, or .obj"),
            "size": num_prop("Cells along the longest axis, 1..=256. Defaults to 64 when neither scale nor voxelSize is given."),
            "scale": num_prop("Cells per file unit (per metre for a glTF). Instead of size."),
            "voxelSize": num_prop("Metres per cell, such as 0.08: the mesh keeps its real size. Instead of size."),
            "upAxis": {"type": "string", "enum": ["y", "z"], "description": "The file's up axis. y (default, glTF and most OBJ) turns it to Z-up; z keeps the axes."},
            "rotation": vec3_prop("Degrees about x, then y, then z (cell axes), after the up axis. Defaults to [0, 0, 0]."),
            "position": vec3_prop("Where the anchor lands, in the active object's local cells (a cell corner). With newObject, the new object's world position, and the anchor is its local origin. Defaults to [0, 0, 0]."),
            "anchor": {"type": "string", "enum": ["min", "center", "bottom"], "description": "Point of the mesh's box placed at position: min (default) the low corner, center, or bottom (the centre of the bottom face)."},
            "fill": {"type": "string", "enum": ["solid", "parity", "shell"], "description": "solid (default) fills all a flood from the box border cannot reach, sealing an open mesh's openings at their rims; parity is the older ray-parity vote; shell writes only cells the surface touches."},
            "colors": {"type": "string", "enum": ["auto", "vertex"], "description": "auto (default): base-colour texture times factor times vertex colours; vertex: vertex colours only."},
            "palette": int_prop("Paint every cell this palette index instead of sampling colours."),
            "slots": {"type": "array", "items": {"type": "integer"}, "description": "Map colours only to these palette indices (the nearest of them); the palette is unchanged."},
            "maxNewSlots": int_prop("Most palette slots written with new colours, 0..=255. Defaults to 32; 0 maps to the nearest existing slots."),
            "tolerance": num_prop("sRGB distance (0..=442) under which an existing slot is reused. Defaults to 12."),
            "mode": mode_prop(),
            "newObject": {"type": "boolean", "description": "Create a new object for the mesh and make it active. Defaults to false."},
            "name": str_prop("Name of the new object (with newObject). Omit for Object N.")
        }), &["path"]),
        tool("mirror_object", "Copy an object mirrored across a world axis through the origin. Cells use -coord-1 on that axis. One undo step. mirrorBones also copies the bound bone and its descendants, swapping an _l/_r suffix. The copy is unbound unless mirrorBones is true.", json!({
            "index": int_prop("Object index to copy"),
            "axis": str_prop("x, y, or z"),
            "name": str_prop("Name of the copy"),
            "position": ivec3_prop("World cell of the copy's local (0, 0, 0). Omit to use the minimum corner of the mirrored cells."),
            "mirrorBones": {"type": "boolean", "description": "Copy the bound bone subtree and swap _l/_r. Default false."}
        }), &["index", "axis", "name"]),
        tool("clear_object", "Empty an object of all its cells at once, whatever its size: fill_box with palette 0 stops at 2000000 cells, this has no limit. index is the object (an index or a name), the active one when omitted. Its name, position, bone, script, and markers stay; remove_object deletes it instead. One undo step; an empty object adds none. Replies with index, name, removed, and voxels.", json!({
            "index": {"type": ["integer", "string"], "description": "Object index or name. Defaults to the active object."}
        }), &[]),
        tool("split_object", "Move (or copy) the cells of an object inside one or more boxes, or of some palette slots, or both, into a new or an existing object, keeping each cell's world position: cut one carve_from_views blockout of a creature into its rig parts (head, flippers, tail) instead of one carve per part plus complement erases. A new object takes the source's position, so its local cells are the source's; an existing target (into) takes them at its own offset, replacing what it holds there. invert takes every cell the boxes and slots do not select. One undo step. Replies with target, name, created, cells (taken), replaced, position, sourceVoxels, targetVoxels, and bounds (target local).", json!({
            "index": {"type": ["integer", "string"], "description": "Source object index or name. Defaults to the active object."},
            "boxes": {"type": "array", "description": "One inclusive box [x0, y0, z0, x1, y1, z1] in the source's local cells, or a list of up to 64. A cell inside any of them is selected. Omit to select by slots alone."},
            "slots": {"type": ["integer", "array"], "items": {"type": "integer"}, "description": "Only cells of these palette slots (the mask). Omit for every slot."},
            "invert": {"type": "boolean", "description": "Take every cell the boxes and slots do not select. Default false."},
            "copy": {"type": "boolean", "description": "Leave the cells in the source too. Default false (move)."},
            "into": {"type": ["integer", "string"], "description": "Existing target object, by index or name. Omit to create a new object."},
            "name": str_prop("Name of the new object. Defaults to the source's name with _part."),
            "makeActive": {"type": "boolean", "description": "Make the target the active object. Default false."}
        }), &[]),
        tool("add_bone", "Add a bone. One undo step. parent is omitted for a root. pivot is a fractional world coordinate, so a joint can sit between cells.", json!({
            "name": str_prop("Unique bone name"),
            "parent": {"type": ["string", "null"], "description": "Parent bone name. Omit or null for a root."},
            "pivot": vec3_prop("World pivot [x, y, z]. Fractional values sit between cells."),
            "rotation": vec4_prop("Quaternion [x, y, z, w] in voxel space. Defaults to identity.")
        }), &["name", "pivot"]),
        tool("set_bone", "Change a bone's parent, pivot, rotation, or name. One undo step. Omit a field to leave it. parent null makes a root. joint true puts the pivot where the bone's part meets its parent's part instead of a given pivot, as place_joints does for one bone (with the new parent when parent is given too); the reply then has the joint's method, samples, gap, and previous pivot.", json!({
            "name": str_prop("Bone to edit"),
            "parent": {"type": ["string", "null"], "description": "Parent bone. null makes a root. Omit to leave the parent."},
            "pivot": vec3_prop("World pivot [x, y, z]"),
            "joint": {"type": "boolean", "description": "Measure the pivot at the joint with the parent's part (see place_joints). Not with pivot."},
            "snap": {"type": "string", "enum": ["none", "corner", "center", "half"], "description": "With joint: round the pivot to cell corners, cell centres, or half cells. Default none."},
            "band": num_prop("With joint, parts apart: cells past the closest gap that still count. Default 0.5."),
            "rotation": vec4_prop("Quaternion [x, y, z, w]"),
            "rename": str_prop("New unique name")
        }), &["name"]),
        tool("place_joints", "Put bone pivots at the joints between rig parts, after split_object and bind_object, instead of placing them by eye. A bone's part is the cells of the objects bound to it; its parent part is the cells bound to the nearest ancestor bone that has any. The pivot is the centroid of the cells both parts hold (method overlap), else of the faces where the parts touch (contact), else of the midpoints between the child's surface cells and the parent's nearest surface cells within band cells of the closest gap (nearest; gap is the empty cells between them). snap rounds it to cell corners, cell centres, or half cells. Every bone by default: roots and bones with no part are skipped with the reason; a named bone that cannot be placed is an error. One undo step. Replies with placed [{name, parent, pivot, previous, method, samples, gap}] and skipped [{name, reason}].", json!({
            "bones": {"type": "array", "items": {"type": "string"}, "description": "Bone names. Default every bone."},
            "snap": {"type": "string", "enum": ["none", "corner", "center", "half"], "description": "Round the pivots: none (default), corner (integer), center (n + 0.5), or half."},
            "band": num_prop("Parts apart: cells past the closest gap that still count toward the pivot. Default 0.5.")
        }), &[]),
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
            "path": str_prop("PNG file to write. Its folder is created."),
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
            "path": str_prop("PNG file to write. Its folder is created."),
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
        tool("preview", "Fast lit preview to a PNG in well under a second: the editor viewport's mesh and light (sun plus ambient), rasterised on the CPU with no path tracer and no window. The camera orbits at yaw and pitch and fits the model, or one object, inside the image with a margin. Use it to check shape and framing often; use render for lighting and materials and model_view for palette-exact colours. lighting lights it as a game does instead (old_pond is frog-world's Old Pond: warm sun, blue ambient, its background), to judge colour under the game's light; lightingPresets adds a project's presets. preset names a camera: game is the gameplay camera (orthographic, yaw -135, pitch 32), and presets (an object or a JSON file) defines or overrides presets, so a project's own game camera is one file. placementYaw shows an asset as it stands when placed turned that many degrees about up in a level, from that same camera, with no angles to work out. inputs (.vox files, or {path?, label?, params?}), variants (parameter sets of the open scripted document, or a variants JSON path), or reference (a concept PNG placed as the first tile) make a contact sheet instead: one tile per model in a grid with its name and parameters under it; width and height are then each tile's (default 512). The open document and its undo history are left as they were. overlay draws the markers over the model, collision boxes as outlines, with their names. Two triangle counts come back and they are different meshes: previewTriangles is the unmerged viewport mesh the image draws; export.triangles and export.quads are what export_mesh or export_models writes with the export options given in export (merge, colorAtlas, ...; all off by default), so pass the game's export options to see the game's count. countsLabel writes both in the corner. Replies with tiles (label, x, y, width, height, previewTriangles, export, voxels, params).", merged(json!({
            "path": str_prop("PNG file to write. Its folder is created."),
            "glb": str_prop("Draw this written .glb instead of the document, as an unlit material does: nearest texel times COLOR_0, no light, so a baked export looks as it will unshaded in a game. Takes the camera, size, background and antialias arguments only."),
            "width": int_prop("Pixels, 1..4096. Default 1024; a contact sheet tile's, default 512."),
            "height": int_prop("Pixels, 1..4096. Default 1024; a contact sheet tile's, default 512."),
            "preset": str_prop("Named camera: game (orthographic, yaw -135, pitch 32: frog-world's gameplay camera, unless presets or the user's camera_presets.json redefines it), default (perspective, yaw 30, pitch 25), or one from presets. yaw, pitch, view, orthographic, fov, and margin given here win over it."),
            "presets": camera_presets_prop(),
            "placementYaw": placement_yaw_prop(),
            "inputs": {"type": "array", "items": {"type": ["string", "object"]}, "description": "Contact sheet: .vox paths, or {path?, label?, params?} (no path is the open document; params are set_params values applied before the tile, so a scripted file is baked with them). At most 64."},
            "variants": {"type": ["array", "string"], "description": "Contact sheet of the open scripted document: parameter sets [{name?, params}] as export_variants takes, or the path of a variants JSON file. Each is baked in a scratch run; the document is restored."},
            "reference": {"type": ["string", "object"], "description": "Contact sheet: a PNG (a concept or turnaround) or {path, label?}, scaled to fit the first tile with its aspect kept. Alone, it sits beside the open document."},
            "columns": int_prop("Contact sheet columns, 1..65. Default about square."),
            "labels": {"type": "boolean", "description": "Contact sheet: each tile's name and parameters under it. Default true."},
            "overlay": {"type": ["boolean", "string", "array"], "description": "Draw the markers (set_marker, Rhai marker, collision_box, connector) over the model: true or all, or boxes (collision boxes as outlines), points, connectors (an arrow toward their direction), markers, or a list. What the model hides is drawn faded and dashed, so a collision box can be checked against the stone before export. Replies with overlay: each drawn marker's name, kind, object, anchor pixel, and whether it is hidden. Works on contact sheet tiles too."},
            "overlayLabels": {"type": "boolean", "description": "Each overlay marker's name beside it. Default true."},
            "export": {"type": ["object", "boolean"], "description": "The export options the reply's export counts use, as export_mesh takes them: merge, colorAtlas, vertexColors, exportHidden, splitObjects, aoStrength, aoRadius, aoTarget, bake (all off when absent, like export_mesh). Replies export {triangles, quads, and these options}, or error when that export would fail. The counts are of the whole document whatever object frames, and match the glb. Unmerged counting is quick; merge costs about what the export does. false skips it."},
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
            "preset": str_prop("Named camera: game (orthographic, yaw -135, pitch 32: frog-world's gameplay camera, unless presets or the user's camera_presets.json redefines it), default, or one from presets; yaw, pitch, view, and orthographic given here win over it."),
            "presets": camera_presets_prop(),
            "placementYaw": placement_yaw_prop(),
            "bones": {"type": "boolean", "description": "Draw the skeleton overlay in the viewport. false hides it for a screenshot. Omit to keep it."}
        }), &[]),
        tool("screenshot", "PNG of the whole editor window. Only when connected to a window.", json!({"path": str_prop("Optional file to write. Its folder is created.")}), &[]),
        tool("screenshot_viewport", "PNG of the 3D viewport. Only when connected to a window.", json!({"path": str_prop("Optional file to write. Its folder is created.")}), &[]),
        tool("screenshot_region", "PNG of a rectangle in window points, origin at the top-left of the client area. Only when connected to a window.", json!({
            "x": num_prop("Left edge, window points"),
            "y": num_prop("Top edge, window points"),
            "width": num_prop("Width in window points"),
            "height": num_prop("Height in window points"),
            "path": str_prop("Optional file to write. Its folder is created.")
        }), &["x", "y", "width", "height"]),
        tool("ui_inspect", "Controls from the previous frame: tool buttons, mode, outliner, sliders, palette cells, viewport chips, and the viewport rectangle, in window points. Only when connected to a window.", json!({}), &[]),
        tool("ui_click", "Click the topmost registered control at a window point. A viewport hit stamps the active tool. A slider hit sets that slider from the point along its track. A palette cell selects that slot. Only when connected to a window.", json!({
            "x": num_prop("X in window points, origin at the top-left of the client area"),
            "y": num_prop("Y in window points, origin at the top-left of the client area")
        }), &["x", "y"]),
        tool("set_script", "Store a Rhai script on an object and read its parameters. One undo step. Voxels change only when regenerate runs. A failure keeps the source and reports the script line. Scripts have the sculpt tools as functions (fill_box with a mode, fill_ellipsoid, fill_rounded_box, fill_tube, carve_box, carve_ellipsoid, carve_rounded_box, carve_tube, recolor, surface_stroke, smooth, break_end, scatter, joints, column_relief, weather, bark, cavity_tint, foliage, moss, algae, vines, sweep, trim_outline, loft, deform, brush, voxelize (import_mesh's voxelizer on an inline mesh; scripts read no files), carve_from_views with views given as rows and a legend, project_paint with its image as rows and a legend, fit_to_views with views as rows and a legend (no other objects in view; reference names an object the run builds), and marker, collision_box, connector, collision_from_voxels for markers) with the MCP options in a trailing map such as #{mode: \"empty\"}, get_voxels, set_voxels, and the document tools (set_palette, set_object, remove_object, clear_object, mirror_object, split_object, add_bone, set_bone, place_joints, remove_bone, bind_object, list_bones, set_pose, clear_pose, add_clip, set_clip_keys, list_clips, remove_clip), which apply after the script in the same undo step, plus sqrt, pow, PI, min, max, clamp, push on arrays, and strings (\"ledge_\" + 4, `ledge_${i}`, to_string); docs/scripting.md lists them.", json!({
            "object": int_prop("Object index. Omit for the active object."),
            "source": str_prop("Rhai source. Empty clears the script.")
        }), &["source"]),
        tool("get_script", "Script source, auto flag, and declared parameters.", json!({
            "object": int_prop("Object index. Omit for the active object.")
        }), &[]),
        tool("set_params", "Set script parameters by name. One undo step. Does not run the script.", json!({
            "object": int_prop("Object index. Omit for the active object."),
            "values": {"type": "object", "description": "Map of parameter name to a number, bool, [r, g, b] (whole 0..255, or 0..1 when every channel is at most 1 and one has a decimal point: [1, 1, 1] is near black, [1.0, 1.0, 1.0] white), a list of numbers (floats), or a list of [x, y] or [x, y, z] points (vec2s, vec3s). Out of range or too long is an error."}
        }), &["values"]),
        tool("list_params", "Parameters from the last successful run of the object's script.", json!({
            "object": int_prop("Object index. Omit for the active object.")
        }), &[]),
        tool("regenerate", "Run the object's script and replace the voxels it writes. One undo step. There is no operation or voxel cap; the run stops at timeoutSeconds. A timeout or runtime error leaves the document unchanged and includes the script line.", json!({
            "object": int_prop("Object index. Omit for the active object."),
            "timeoutSeconds": {"type": "number", "description": "Stop the run after this many seconds. Default 300. 0 waits as long as it takes."}
        }), &[]),
        tool("export_mesh", "Write a glTF 2.0 .glb of every object. Exposed faces only. The default material is a 256-wide palette texture. A swaying or bobbing slot adds TEXCOORD_1 (sway, bob) on every vertex. A skeleton writes one rigid skin: each vertex belongs to one bone with weight 1. The reply includes the file's node, mesh, skin and animation structure, quads, and triangles (every triangle in the glb).", json!({
            "path": str_prop("Where to write the .glb. Its folder is created."),
            "merge": {"type": "boolean", "description": "Greedy-merge coplanar faces of one palette slot. Defaults to false."},
            "vertexColors": {"type": "boolean", "description": "Store a linear COLOR_0 on each vertex instead of the palette texture. Defaults to false."},
            "exportHidden": {"type": "boolean", "description": "Include hidden objects. Default false, which omits them."},
            "splitObjects": {"type": "boolean", "description": "One mesh node per object, still sharing one skin, so a game can hide a part. Default false is a single mesh."},
            "colorAtlas": {"type": "boolean", "description": "Merge coplanar faces whatever their palette slot and paint each cell into a per-mesh colour atlas texture (nearest filtering) instead of the palette, for far fewer triangles on noisy palettes. Faces of different motion or opacity stay apart. One rigid mesh only: not with a skeleton, splitObjects or vertexColors. Defaults to false."},
            "aoStrength": {"type": "number", "minimum": 0, "maximum": 1, "description": "Bake ambient occlusion into COLOR_0 so crevices, joints and the undersides of overhangs export darker, at no runtime cost. 0..1, how much light a fully occluded corner loses. Multiplied into vertexColors, otherwise a grey COLOR_0 that glTF (and Godot's StandardMaterial3D) multiplies with the texture. aoTarget picks vertex colours or the colour atlas texels. Defaults to 0, off."},
            "aoRadius": {"type": "integer", "minimum": 0, "maximum": 8, "description": "Reach of the occlusion in cells beyond the four that touch a corner, as a cosine-weighted hemisphere, so deep crevices and overhangs darken more. 0 or 1 is the corner term alone. Defaults to 3."},
            "aoTarget": {"type": "string", "enum": ["auto", "vertex", "texture", "both"], "description": "Where aoStrength goes. vertex: a per-corner COLOR_0; faces whose corners shade differently are not merged, which costs triangles. texture: each cell's colour atlas texel is darkened by the mean of its corners and faces merge exactly as without occlusion; needs colorAtlas. both: the two at once, for a shader that uses one (a material multiplying texture by COLOR_0 darkens twice). auto (default): texture with colorAtlas, else vertex."},
            "bake": bake_prop()
        }), &["path"]),
        tool("export_variants", "Bake a scripted document once per parameter set and write <name>.vox and <name>.glb for each into dir, with the export_mesh mesh options, then a manifest JSON (default dir/variants.json) with each set's requested and resolved parameters, markers (local, world, glTF), bounds (voxel and glTF), quads, voxels, and mesh options. An existing manifest is updated by variant name: the sets exported now replace their entries in place, the other entries are kept in order, and new names are appended, so a subset can be redone; replace: true writes a fresh manifest with only these sets. The top-level options are present while every entry shares them. An entry records sceneShift when the document opened with one (see open_file). Parameters are keyed by name, or object/name when two objects share one, and take the values set_params does (a colour is whole 0..255, or 0..1 with a decimal point), so a manifest entry's params can be passed back unchanged. Every script runs per set, as regenerate does. The open document and its undo history are left as they were. One failing set stops the call with its name, before the manifest is written. Replies with the whole manifest, its path, and the names it added, replaced, and kept.", json!({
            "variants": {"type": "array", "minItems": 1, "maxItems": 64, "items": {"type": "object"}, "description": "1 to 64 sets: {name?, params}. name is a file stem of letters, digits, _, -, . (default variant_1, ...); params maps parameter names to values, as set_params takes them."},
            "dir": str_prop("Folder for the .vox, .glb, and manifest files. It is created."),
            "manifest": str_prop("Manifest path. Defaults to dir/variants.json. Its folder is created."),
            "replace": {"type": "boolean", "description": "Write a fresh manifest with only these sets instead of merging into the existing one by name. Defaults to false."},
            "vox": {"type": "boolean", "description": "Write a .vox per set too. Defaults to true."},
            "timeoutSeconds": num_prop("Script deadline per object and set, up to 3600. Defaults to 300."),
            "merge": {"type": "boolean", "description": "As export_mesh. Defaults to false."},
            "vertexColors": {"type": "boolean", "description": "As export_mesh. Defaults to false."},
            "colorAtlas": {"type": "boolean", "description": "As export_mesh. Defaults to false."},
            "aoStrength": num_prop("As export_mesh, 0..1. Defaults to 0, off."),
            "aoRadius": int_prop("As export_mesh, 0..8. Defaults to 3."),
            "aoTarget": {"type": "string", "enum": ["auto", "vertex", "texture", "both"], "description": "As export_mesh. Defaults to auto."},
            "bake": {"type": ["object", "boolean"], "description": "As export_mesh. Defaults to off."},
            "exportHidden": {"type": "boolean", "description": "As export_mesh. Defaults to false."},
            "splitObjects": {"type": "boolean", "description": "As export_mesh. Defaults to false."}
        }), &["variants", "dir"]),
        tool("export_models", "Write 1 to 16 .glb files and report each file's quads, triangles, and structure: nodes, meshes (vertices, indices, bounds), skins, and animations. A warning names an empty mesh, a skin with no joints, a nameless or duplicated node, or an animation with no channels. source opens that .vox in its own session, so the open document is not replaced. Omit source to export the open document. One model's failure is reported on that model and does not skip the others.", json!({
            "models": {"type": "array", "minItems": 1, "maxItems": 16, "description": "Each item is {path, source?, merge?, vertexColors?, exportHidden?, splitObjects?, colorAtlas?, aoStrength?, aoRadius?, aoTarget?, bake?}, with the export_mesh meanings. path ends in .glb. source, when set, is a .vox file.", "items": {"type": "object"}}
        }), &["models"]),
        bake_scene_tool(),
    ]
}

/// MCP `bake_scene`. Its own function keeps `json!` within the recursion limit.
fn bake_scene_tool() -> Value {
    tool("bake_scene", "Bake one placement of many .vox assets together, so each is shaded by its neighbours and by ground, into one merged .glb with one colour atlas (colorAtlas faces, one texel per cell face, material KHR_materials_unlit, to draw unshaded). The open document is not touched. A placement is the frame of the glb export_mesh writes, as a game places it: world = translation + Lean x scale x Ry(yaw) x (x, z, -y) of a cell corner, Y up, so a level's translation, rotation.y in degrees, and uniform scale copy across. An optional lean {degrees, toward} tilts a placement by degrees (-90..90) about its base so its top moves toward the yaw toward (0 is -Z, 90 is -X, as yaw); without it the placement is upright. An instance's optional sway (0..4, default 1) multiplies the sway amount (TEXCOORD_1 x) of its moving slots in the written mesh, so kinds of plant sway apart in one baked mesh; bob is kept. Any yaw or lean works, and repeats of one .vox share one grid. Occluders stop light and are not written. Costs: triangles are the sum of each placement's colorAtlas triangles, as nothing merges across placements, and the atlas holds one RGBA texel per exposed cell face of every placement. Rays read one bit grid per distinct source; occluder triangles take about 112 bytes each. The reply gives memory in bytes. Replies with triangles, vertices, atlas, texels, bytes, memory, bake, and structure.", json!({
        "scene": {"type": ["object", "string"], "description": "The scene, or the path of a JSON file of it (relative paths inside start at that file's folder; for an object, at the server's working directory): {instances: [{source: a .vox path, translation?: [x, y, z], yaw?: degrees about +Y, scale?: uniform, lean?: {degrees: tilt, toward: yaw the top moves toward}, sway?: multiplier of its slots' sway, 0..4}], occluders?: [{mesh: a .glb, .gltf or .obj path, translation?, yaw?, scale?, lean?} | {plane: height, solid below it} | {height: {origin?: [x, z], cell?: spacing, rows: [[y at x...] per z...]}}], bake?, exportHidden?}. 1 to 4096 instances."},
        "path": str_prop(".glb file to write. Its folder is created."),
        "bake": {"type": ["object", "boolean"], "description": "The export_mesh bake option, over the scene's own; distance is in scene units here. Defaults to occlusion alone."}
    }), &["scene", "path"])
}

fn tool(name: &str, description: &str, mut properties: Value, required: &[&str]) -> Value {
    let [read_only, destructive, idempotent] = hints(name);
    // Every tool whose method takes `space` lists it.
    if map_tool(name, &json!({}))
        .is_ok_and(|(method, _)| remote::WORLD_METHODS.contains(&method.as_str()))
    {
        properties["space"] = remote::space_prop();
    }
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
        "remove_object" | "split_object" | "undo" | "redo" | "ui_click" | "regenerate"
        | "import_mesh" | "batch" => [false, true, false],
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
        | "recolor"
        | "surface_stroke"
        | "clear_object"
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
        | "trim_outline"
        | "deform"
        | "loft"
        | "brush"
        | "carve_from_views"
        | "project_paint"
        | "fit_to_views"
        | "compare_silhouette"
        | "bark"
        | "planks"
        | "cavity_tint"
        | "foliage"
        | "set_marker"
        | "remove_marker"
        | "collision_from_voxels"
        | "set_palette"
        | "set_object"
        | "set_bone"
        | "place_joints"
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
        | "export_variants"
        | "bake_scene" => [false, true, true],
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
    p["facing"] = vec3_prop("Only faces whose outward normal points within 60 degrees of this direction, such as [0, -1, 0] for the front or [0, 0, -1] for undersides. Without faces, facing considers every face (vines: side); a facing that no face in faces points along is an error.");
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

/// `mode` of the fills and `carve_from_views`, which also intersect.
fn fill_mode_prop() -> Value {
    json!({"type": "string", "enum": ["replace", "empty", "paint", "intersect"], "description": "replace (default) writes every cell of the shape; empty writes only empty cells, so existing detail stays; paint recolours only solid cells; intersect keeps only the object's solid cells inside the shape, as they are, and erases the rest (a boxy hull cut to an ellipsoid dome), so palette is not used."})
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
render path-traces a PNG in this process. The window path-traces in Render mode.

Organic shaping: loft (a section that changes along a curve), deform (bend,
taper, twist, displace), and brush (inflate, deflate, smooth, flatten, grab
along points). trim_outline removes every solid cell outside a closed plan
outline and an inset per height, with a new skin along the cut if asked. Tree
crowns: foliage packs separate leaf clumps with lit tops and dark gaps. Decks
and floors: planks lays separate boards with real gaps, staggered joints, and
a tone per board. batch applies a list of editing tools as one undo step and
leaves the document as it was when one fails; tools that write files are
refused in it. Colliders: collision_from_voxels builds collision box markers
from an object's solid cells and keeps hollows open.
Parts: split_object moves the cells inside boxes, or of some slots, into a
new or existing object with their world positions, to cut a blockout into
rig parts. place_joints puts bone pivots where each bound part meets its
parent's part (set_bone joint for one bone). carve_from_views and fit_to_views take a grey depth map per view
(depth, depthPath) and carve the cells in front of its surface, so a round
form is round. set_palette takes color #rrggbb or #rrggbbaa. mode intersect on the fills and carve_from_views keeps only
the cells inside the shape. surface_stroke paints or grooves a stroke that
snaps to the surface. clear_object empties an object with no box limit. recolor replaces palette slots in a box or the whole object
with no shape change. space world gives the editing tools world cells instead of the
active object's local cells. tools/list describes every tool; docs/mcp.md has examples."
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

/// The export option `bake`. Its own function keeps `json!` within the
/// recursion limit, like [`lighting_prop`].
fn bake_prop() -> Value {
    json!({"type": ["object", "boolean"], "description": "Paint light into the colour atlas by real rays, one texel per cell face, to draw unshaded; needs colorAtlas. The glb material gets KHR_materials_unlit and extras.froxelBake. A texel is albedo x (ambient x ambientEnergy x (1 - occlusion x blocked) + sun x sunEnergy x max(n.l, 0) x open) in linear light, clamped, encoded sRGB: blocked is the share of cosine-weighted rays that hit an opaque cell within distance (a hit at t counts 1 - (t/distance)^2), open the share of sun rays that get out. Fields, all optional: occlusion 0..1 (default 1), distance in cells up to 4096 (default 16), rays 1..1024 per cell face (default 64), preset (a lighting preset such as old_pond) or sunDirection [x, y, z] toward the sun, Z up, or godotSunBasis; sunColor and ambientColor ([r, g, b] 0..16 sRGB, or #rrggbb); sunEnergy; ambientEnergy; lightingPresets (as preview takes it); sunSoftness, the sun disc's half angle in degrees 0..45 (default 0, a hard shadow), and sunRays 1..256 (default 1 hard, 16 soft). With no preset and no sun or ambient field it is occlusion alone under a white ambient of 1 (sun off); named lighting starts from sun 1, ambient 0.3. A preset's exposure, background and lut are not baked, because a game's tonemapper and grade reach unshaded surfaces too. Deterministic: no noise, and the same input gives the same bytes. Triangle counts do not change. true is every default. Absent or false is off and leaves the file byte for byte."})
}

/// MCP `preview` `lighting`. Its own function keeps `json!` within the
/// macro recursion limit.
fn lighting_prop() -> Value {
    json!({"type": ["string", "object"], "description": "A game's light instead of the viewport's: a preset name (viewport, the default; old_pond, frog-world's Old Pond region; or one from lightingPresets or the user's lighting_presets.json), or an object of lighting fields: sunDirection [x, y, z] toward the sun, Z up, or godotSunBasis (the 9 or 12 numbers of a DirectionalLight3D's Transform3D as the .tscn writes it); sunColor and ambientColor ([r, g, b] 0..16 sRGB, or #rrggbb); sunEnergy and ambientEnergy; exposure; background; lut (a 3D LUT PNG, N*N by N, applied last as Godot's colour correction); preset (the preset these fields go over). Shading is albedo x (sun x energy x max(n.l, 0) + ambient x energy) x exposure in linear light, clamped, with no shadows. Replies lighting with the values used."})
}

/// MCP `preview`, `compare_silhouette`, and `set_view` `presets`.
fn camera_presets_prop() -> Value {
    json!({"type": ["object", "string"], "description": "A project's camera presets: an object of name to {yaw, pitch, orthographic, fov, margin}, or the path of a JSON file of them (or with them under presets). godotLook [x, y, z], a Godot camera's look direction, replaces yaw and pitch. A name that exists further down (game, default, or one in the user's camera_presets.json or the file FROXEL_CAMERA_PRESETS names) is overridden field by field; another name is a new preset over default."})
}

/// MCP `preview`, `compare_silhouette`, and `set_view` `placementYaw`.
fn placement_yaw_prop() -> Value {
    num_prop("Degrees: the asset's turn about up where it is placed in a level, positive counter-clockwise from above, the same number as a Godot node's rotation_degrees.y. The camera (and a lighting sun) turn the other way, so the image is the asset as it stands in the level from the same camera. yaw in the reply is the angle used, yaw minus this.")
}

/// MCP `preview` `lightingPresets`.
fn lighting_presets_prop() -> Value {
    json!({"type": ["object", "string"], "description": "Lighting presets for this call: name to lighting fields, or the path of a JSON file of them (also under lighting). A built-in name is overridden field by field; a relative lut path is relative to the file."})
}

#[cfg(test)]
#[path = "../tests/mcp.rs"]
mod tests;
