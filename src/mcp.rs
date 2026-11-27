//! Model Context Protocol on stdin/stdout.
//!
//! `froxel mcp` edits an in-process document and has no window.
//! `froxel mcp --connect 127.0.0.1:PORT` drives an editor started with
//! `--control PORT`. `--launch` opens that editor when the port is closed.
//! Screenshots exist only on the connected window. `render` path-traces a PNG
//! in this process. The window path-traces in Render mode on the GPU.

use std::io::{self, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

mod reload;

use crate::control::ControlClient;
use crate::remote::{self, apply};
use crate::state::Session;

const PROTOCOL: &str = "2025-06-18";
const SUPPORTED: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18", "2026-07-28"];

/// The `initialize` instructions. Clients cut long ones (Claude Code keeps
/// the first 4096 characters, the sandbox note included), so the rest is the
/// `froxel://guide` resource, `docs/agent-guide.md`.
pub(crate) const INSTRUCTIONS: &str = "Froxel edits .vox voxel models. Edits apply to the active object (set_active_object), in its local cells; space: \"world\" (on the fills, carve, set_voxels, get_voxels, smooth, loft, blend_shapes, brush, carve_from_views, project_paint, decal, split_object) takes world cells and subtracts the object's position. A box is x0, y0, z0, x1, y1, z1 (inclusive) or min: [x, y, z] and max: [x, y, z], in batch items too; give one form. batch runs a list of editing tools in order as one undo step and stops at the first error with the document as it was: use it instead of dozens of single fill_box and carve calls, and keep render and export calls outside it. Look after every step with preview (a fast lit PNG, under a second; preset game is the gameplay camera, overlay draws collision boxes and markers); render path-traces a slow final still; model_view is the flat palette view to hold against a turnaround sheet; compare_silhouette scores the model against a concept view. mirror on the fills, set_voxels, recolor, surface_stroke, project_paint, and decal repeats a call across the symmetry plane in the same undo step. Tools that write a file create its missing folders. set_palette takes #rrggbb or #rrggbbaa. screenshot (window, viewport, or a rectangle), ui_inspect, and ui_click need an editor started with --control and this server with --connect. If a tool the docs mention is missing, call reload: the froxel on disk is newer than this server. Which tool for which job (hand sculpting, mesh import, a views sheet, stone, wood, and plant surfaces, rig parts, colliders, export and baking) is the resource froxel://guide (docs/agent-guide.md): read it before a new kind of asset. Hand sculpting step by step is froxel://hand-sculpting (docs/hand-sculpting.md); every tool's options are in docs/mcp.md; Rhai scripts (set_script) in docs/scripting.md.";

/// The documents `resources/list` offers: uri, name, description, text.
const RESOURCES: &[(&str, &str, &str, &str)] = &[
    (
        "froxel://guide",
        "agent-guide.md",
        "Which Froxel tool for which job, workflow by workflow: the detail the short server instructions leave out.",
        include_str!("../docs/agent-guide.md"),
    ),
    (
        "froxel://hand-sculpting",
        "hand-sculpting.md",
        "Hand sculpting a creature or a unique asset with the MCP tools, step by step.",
        include_str!("../docs/hand-sculpting.md"),
    ),
];

fn resource_list() -> Vec<Value> {
    RESOURCES
        .iter()
        .map(|(uri, name, description, text)| {
            json!({"uri": uri, "name": name, "description": description, "mimeType": "text/markdown", "size": text.len()})
        })
        .collect()
}

fn read_resource(params: &Value) -> Result<Value, String> {
    let uri = params
        .get("uri")
        .and_then(Value::as_str)
        .ok_or("resources/read needs a uri")?;
    let (uri, _, _, text) = RESOURCES
        .iter()
        .find(|(known, ..)| *known == uri)
        .ok_or_else(|| format!("unknown resource {uri}; resources/list names them"))?;
    Ok(json!({"contents": [{"uri": uri, "mimeType": "text/markdown", "text": text}]}))
}

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
        Ok(Some(options)) => serve(options, args),
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

struct Options {
    mode: Mode,
    /// Reload before a request when the binary on disk has changed.
    watch: bool,
    /// The state file of a reload to resume.
    resume: Option<std::path::PathBuf>,
}

fn parse_args(args: &[String]) -> Result<Option<Options>, String> {
    let mut connect: Option<String> = None;
    let mut launch = false;
    let mut watch = false;
    let mut resume = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_help();
                return Ok(None);
            }
            reload::CHECK_FLAG => {
                println!("{}", reload::CHECK_REPLY);
                return Ok(None);
            }
            reload::RESUME_FLAG => {
                i += 1;
                let path = args.get(i).ok_or("--resume needs a state file")?;
                resume = Some(std::path::PathBuf::from(path));
            }
            "--watch" => watch = true,
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
    let mode = match connect {
        None => Mode::Headless,
        Some(addr) => {
            if let Err(err) = prepare_remote(&addr, launch) {
                return Err(err.to_string());
            }
            Mode::Remote { addr }
        }
    };
    Ok(Some(Options {
        mode,
        watch,
        resume,
    }))
}

fn prepare_remote(addr: &str, launch: bool) -> io::Result<()> {
    crate::sandbox::check_port("control")?;
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

fn serve(options: Options, args: &[String]) -> i32 {
    let mut stdout = io::stdout();
    let mut state = Proto {
        headless: matches!(options.mode, Mode::Headless),
        ..Proto::default()
    };
    let mut session = Session::new();
    session.replace_empty();
    let mut remote = match &options.mode {
        Mode::Headless => None,
        Mode::Remote { addr } => match ControlClient::connect(addr) {
            Ok(client) => Some(client),
            Err(err) => {
                eprintln!("{err}");
                return 1;
            }
        },
    };
    let mut pending = Vec::new();
    if let Some(path) = &options.resume {
        match reload::resume(path, &mut session) {
            Ok(resumed) => {
                state.initialized = true;
                state.version = resumed.version;
                pending = resumed.pending;
                eprintln!(
                    "froxel mcp: reloaded from {} with {} objects",
                    state.exe.display(),
                    resumed.objects
                );
                let note = json!({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"});
                if writeln!(stdout, "{note}").is_err() || stdout.flush().is_err() {
                    return 0;
                }
            }
            Err(err) => eprintln!("froxel mcp: {err}; starting with an empty document"),
        }
    }
    let mut lines = reload::Lines::stdin(pending);
    loop {
        let line = match lines.next_line() {
            Ok(Some(line)) => line,
            Ok(None) => return 0,
            Err(err) => {
                eprintln!("stdin: {err}");
                return 1;
            }
        };
        if options.watch && state.initialized {
            if let Some(plan) = state.watch_reload(&mut session) {
                // The new binary answers this request: it goes first in the
                // input handed over.
                let mut handed = line.clone();
                handed.push(b'\n');
                handed.extend_from_slice(lines.rest());
                let _ = stdout.flush();
                let err = reload::exec(&plan, args, &handed);
                eprintln!("froxel mcp: {err}");
                plan.clean_up();
            }
        }
        if line.len() > 16 * 1024 * 1024 {
            let _ = writeln!(
                stdout,
                r#"{{"jsonrpc":"2.0","id":null,"error":{{"code":-32700,"message":"line too long"}}}}"#
            );
            let _ = stdout.flush();
            continue;
        }
        let Ok(line) = String::from_utf8(line) else {
            let _ = writeln!(
                stdout,
                r#"{{"jsonrpc":"2.0","id":null,"error":{{"code":-32700,"message":"input is not UTF-8"}}}}"#
            );
            let _ = stdout.flush();
            continue;
        };
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
        if let Some(plan) = state.reload.take() {
            let err = reload::exec(&plan, args, lines.rest());
            // Still here: the new binary did not start. Keep serving.
            eprintln!("froxel mcp: {err}");
            plan.clean_up();
        }
    }
}

struct Proto {
    initialized: bool,
    /// The protocol version `initialize` agreed, kept across a reload.
    version: String,
    /// The document lives in this process (no `--connect`).
    headless: bool,
    /// This binary and its stamp when the server started.
    exe: std::path::PathBuf,
    stamp: Option<reload::Stamp>,
    /// A binary `--watch` found unable to resume, not asked again.
    refused: Option<reload::Stamp>,
    /// A `reload` to carry out once its reply is written.
    reload: Option<reload::Plan>,
}

impl Default for Proto {
    fn default() -> Self {
        let exe = reload::exe().unwrap_or_default();
        let stamp = reload::stamp(&exe);
        Self {
            initialized: false,
            version: PROTOCOL.to_string(),
            headless: true,
            exe,
            stamp,
            refused: None,
            reload: None,
        }
    }
}

impl Proto {
    /// `--watch`: a plan when the binary on disk changed and can resume.
    fn watch_reload(&mut self, session: &mut Session) -> Option<reload::Plan> {
        let now = reload::stamp(&self.exe);
        if now.is_none() || now == self.stamp || now == self.refused {
            return None;
        }
        if let Err(err) = reload::check(&self.exe) {
            eprintln!("froxel mcp: {err}");
            self.refused = now;
            return None;
        }
        match reload::save(session, self.headless, &self.version) {
            Ok(plan) => Some(plan),
            Err(err) => {
                eprintln!("froxel mcp: reload: {err}");
                self.refused = now;
                None
            }
        }
    }

    /// MCP `reload`: plan the re-exec when the binary changed (or `force`).
    fn reload_tool(&mut self, session: &mut Session, args: &Value) -> Result<Value, String> {
        let force = match args.get("force") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(b)) => *b,
            Some(_) => return Err("force must be true or false".into()),
        };
        let exe = reload::exe().map_err(|err| format!("this server's binary: {err}"))?;
        let changed = reload::stamp(&exe) != self.stamp;
        if !changed && !force {
            return Ok(json!({
                "reloaded": false,
                "binary": exe.display().to_string(),
                "reason": "the binary has not changed since this server started; pass force: true to reload anyway"
            }));
        }
        if !cfg!(unix) {
            return Err("reload re-executes the server in place, which needs Unix; restart the server instead".into());
        }
        reload::check(&exe)?;
        let plan = reload::save(session, self.headless, &self.version)?;
        let document = if self.headless {
            "kept: reopened from a temporary .vox with its path, active object, and unsaved state; undo history and the preview pose are not kept"
        } else {
            "in the connected editor, unchanged"
        };
        let reply = json!({
            "reloaded": true,
            "binary": exe.display().to_string(),
            "changed": changed,
            "document": document,
            "next": "the server restarts after this reply and sends notifications/tools/list_changed; a client that honours listChanged asks tools/list again"
        });
        self.reload = Some(plan);
        Ok(reply)
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
        state.version = version.to_string();
        let mut result = json!({
            "protocolVersion": version,
            "capabilities": {"tools": {"listChanged": true}, "resources": {}},
            "serverInfo": {"name": "Froxel", "version": env!("CARGO_PKG_VERSION")},
            "instructions": INSTRUCTIONS,
        });
        if let Some(sandbox) = crate::sandbox::summary() {
            let text = result["instructions"].as_str().unwrap_or_default();
            let text = format!("{text} {sandbox}");
            result["instructions"] = json!(text);
        }
        return Some(rpc_ok(id, result));
    }
    if !state.initialized {
        return Some(rpc_error(id, -32600, "server not initialized"));
    }
    let result = match method {
        "tools/list" => json!({"tools": tools()}),
        "tools/call" if params.get("name").and_then(Value::as_str) == Some("reload") => {
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match known_arguments("reload", &args).and_then(|()| state.reload_tool(session, &args))
            {
                Ok(value) => tool_ok(value),
                Err(err) => tool_error(&err),
            }
        }
        "tools/call" => match call_tool(session, remote, &params) {
            Ok(result) => result,
            Err(message) => return Some(rpc_error(id, -32602, &message)),
        },
        "resources/list" => json!({"resources": resource_list()}),
        "resources/read" => match read_resource(&params) {
            Ok(result) => result,
            Err(message) => return Some(rpc_error(id, -32602, &message)),
        },
        "resources/templates/list" => json!({"resourceTemplates": []}),
        "prompts/list" => json!({"prompts": []}),
        "logging/setLevel" => json!({}),
        other => return Some(rpc_error(id, -32601, &format!("method not found: {other}"))),
    };
    Some(rpc_ok(id, result))
}

/// A `tools/call`. A request that is not a call of a listed tool (params
/// that are not an object, no name, an unknown name, arguments that are not
/// an object) is a JSON-RPC error, as MCP asks; everything the tool itself
/// refuses or fails at is a result with `isError`.
fn call_tool(
    session: &mut Session,
    remote: Option<&mut ControlClient>,
    params: &Value,
) -> Result<Value, String> {
    if !params.is_object() {
        return Err("params must be an object".into());
    }
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Err("missing tool name".into());
    };
    let mut args = params.get("arguments").cloned().unwrap_or(json!({}));
    if !args.is_object() {
        return Err("arguments must be an object".into());
    }
    // The screenshot tools merged into `screenshot`; their old names still
    // run it, unlisted, so callers written for them keep working.
    let name = match name {
        "screenshot_viewport" => {
            args["region"] = json!("viewport");
            "screenshot"
        }
        "screenshot_region" => "screenshot",
        other => other,
    };
    if crate::ops::by_tool(name).is_none() {
        return Err(format!("unknown tool `{name}`"));
    }
    Ok(call_listed(session, remote, name, &args))
}

fn call_listed(
    session: &mut Session,
    remote: Option<&mut ControlClient>,
    name: &str,
    args: &Value,
) -> Value {
    let args = args.clone();
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
        Err(err) => tool_error(&crate::sandbox::explain(&err)),
    }
}

/// Refuse an argument the tool's schema does not list, so a misspelt or
/// unsupported option is an error rather than silently ignored.
/// Every tool's `tools/list` entry, from the registry.
fn tools() -> &'static [Value] {
    crate::ops::schema::mcp_tools()
}

fn known_arguments(name: &str, args: &Value) -> Result<(), String> {
    known_arguments_in(tools(), name, args)
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
    let method = crate::ops::by_tool(name)
        .ok_or_else(|| format!("unknown tool `{name}`"))?
        .method;
    let params = args.clone();
    if name == "screenshot" {
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

fn print_help() {
    eprintln!(
        "\
usage: froxel mcp [--connect HOST:PORT] [--launch] [--watch]

Model Context Protocol on stdin/stdout, one JSON-RPC message per line.
Without --connect the session is in-process and has no window.
Screenshots need the editor:

  froxel --control 7979
  froxel mcp --connect 127.0.0.1:7979

--launch opens the editor with that control port when nothing is listening.
reload (a tool) restarts this server on a rebuilt froxel binary in place:
the client keeps its connection and the open document is reopened, and the
server sends notifications/tools/list_changed so the client lists the new
tools. --watch reloads by itself before the first request after the binary
changes. The client must honour tools/list_changed to see new tools.
render path-traces a PNG in this process. The window path-traces in Render mode.

Organic shaping: blend_shapes (spheres, ellipsoids, boxes, capsules, and tori
blended as signed distance fields, so they flow into each other like clay),
loft (a section that changes along a curve), deform (bend,
taper, twist, displace), and brush (inflate, deflate, smooth, flatten, grab
along points). trim_outline removes every solid cell outside a closed plan
outline and an inset per height, with a new skin along the cut if asked. Tree
crowns: foliage packs separate leaf clumps with lit tops and dark gaps. Decks
and floors: planks lays separate boards with real gaps, staggered joints, and
a tone per board. batch applies a list of editing tools as one undo step and
leaves the document as it was when one fails; tools that write files are
refused in it. Colliders: collision_from_voxels builds collision box markers
from an object's solid cells and keeps hollows open.
Parts: split_parts cuts a blockout into one object per rig part at world
[0, 0, 0], each cell to the part whose seed (point, bone, or marker) is
nearest through the solid, with optional cutting planes. split_object
moves the cells inside boxes, or of some slots, into a new or existing
object with their world positions. place_joints puts bone pivots where each bound part meets its
parent's part (set_bone joint for one bone). carve_from_views and fit_to_views take a grey depth map per view
(depth, depthPath) and carve the cells in front of its surface, so a round
form is round. measure_sheet reads a views sheet (crops, the 2 x 2 layout, cube pitch per
view, front/side/top agreement in percent, and the top view's parts) and
writes no file. compare_silhouette and the views take mask edits (excludeColors,
flood, mask, erase, keepLargest) for a concept with water, splashes, or a
rider in it, and fillHoles for white eyes keyed out on a white sheet. set_palette takes color #rrggbb or #rrggbbaa. mode intersect on the fills and carve_from_views keeps only
the cells inside the shape. surface_stroke paints or grooves a stroke that
snaps to the surface. components lists connected parts, keeps the largest,
removes floaters, and deletes, moves, or splits out a part by a seed, cut
at its neck from an anchor. deterrace re-quantizes an object into blocks of
2 to 4 cells, or staggers its one-cell terrace steps, so a voxelized smooth
mesh has no long contour bands. import_mesh orient upright stands a tilted
mesh level from its principal axes, head to -Y. reduce_palette clusters an object's colours
into a few (eyes and mouths kept) or snaps them to a palette, and
despeckles the surface (import_mesh clean at import). orient_to_views (and import_mesh orient) turns
and scales an object to the axis-aligned turn and fine yaw whose
silhouettes best match a views sheet. paint_from_views recolours every surface
cell from a whole views sheet, each from the views that see it.
optimize_to_views iterates a small creature's shape and colours until its
orthographic views match its pixel-art views. clear_object empties an object with no box limit. recolor replaces palette slots in a box or the whole object
with no shape change. space world gives the editing tools world cells instead of the
active object's local cells. tools/list describes every tool; docs/mcp.md has examples.
The initialize instructions are a short summary; resources/read froxel://guide
(docs/agent-guide.md) says which tool for which job, and froxel://hand-sculpting
is docs/hand-sculpting.md."
    );
}

#[cfg(test)]
#[path = "../tests/mcp.rs"]
mod tests;
