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
            "capabilities": {"tools": {"listChanged": true}},
            "serverInfo": {"name": "Froxel", "version": env!("CARGO_PKG_VERSION")},
            "instructions": "Edits apply to the active object, in that object's local cells; space world on the fills, carve, set_voxels, get_voxels, smooth, loft, blend_shapes, brush, carve_from_views, project_paint, decal, and split_object takes world cells instead and subtracts the object's position, so parts moved by a save and reopen or a mirror_object copy need no recomputed boxes. batch applies a list of editing tools in order as one undo step, stopping at the first error with the document as it was: use it instead of dozens of single fill_box and carve calls, and keep render and export calls outside it. Every tool that writes a file (save_file, render, model_view, preview, compare_silhouette, export_mesh, export_models, export_variants, bake_scene, screenshots) creates the missing folders of its path first. Export's bake option (with colorAtlas) paints ray-traced occlusion and a sun with shadows into the atlas, for a game to draw unshaded; preview with glb looks at the written file. bake_scene bakes a placement of many .vox files together, shaded by each other and by ground, into one merged glb and atlas; an instance's lean tilts it about its base, and its sway multiplies how far its moving slots sway. render path-traces a PNG in this process. preview writes a fast lit PNG (the viewport mesh and light, rasterised, well under a second) framed on the model or one object; use it to look after every step. Its preset game is the gameplay camera (a project redefines it with presets, an object or a JSON file, or the user's camera_presets.json), placementYaw sees an asset turned that much about up in a level from that camera, lighting (old_pond, or a project's lightingPresets) shows colour under a game's sun and ambient instead of the viewport light, overlay draws collision boxes, points, and connectors over the model, and inputs, variants, and reference make one labelled contact sheet of several models beside a concept image. Its previewTriangles is the unmerged mesh it draws, not the game's: its export counts (with export set to the game's options, e.g. colorAtlas) are what export_mesh writes. model_view writes the flat model view (palette colours, no light) for comparing with a turnaround sheet. set_view moves the window camera before screenshot_viewport. screenshot, screenshot_viewport, screenshot_region, ui_inspect, and ui_click need an editor started with --control and this server started with --connect. The window path-traces in Render mode. set_palette takes a slot's color as #rrggbb or #rrggbbaa (or r, g, b), so a concept's hex codes go in as they are. Hand sculpting (docs/hand-sculpting.md) models a creature or a unique asset without a script: block out with fill_box, fill_rounded_box, and fill_ellipsoid, grow limbs and roots with fill_tube (caps flat cuts its ends square, for pipes, discs, rims, and wheels), cut hollows with carve, soften with smooth, round a boxy blockout with a fill in mode intersect (only the cells inside the shape stay, so a hull becomes a dome), refine with set_voxels, draw seams, markings, and grooves that follow a curved surface with surface_stroke (points snap to the nearest surface cells), paint a whole shell's plates (a central plate, rings of plates, an outer band, and a rim, with painted or grooved seams that follow the dome, mirrored left to right) with one shell_plates call, swap one palette slot for another (in a box or everywhere, no shape change) with recolor, and render after every step. For a symmetric creature draw one side: mirror (true for x about the object's centre, an axis, or {axis, at}) on set_voxels, the fills, recolor, surface_stroke, project_paint, and decal repeats the call across the plane on the far side's own surface, mirrored, in the same undo step. clear_object empties an object of any size in one step (fill_box with palette 0 stops at 2000000 cells), to start a part again. For a unique organic asset with a mesh sculpted in Blender or made by an image-to-3D tool, import_mesh voxelizes the .glb, .gltf, or .obj (PNG, JPEG, or WebP textures and vertex colours quantized into the palette; fill solid fills a double-walled or open mesh, sealing its openings at their rims, so check interior and openEdges in the reply) as the blockout (its orient option, or orient_to_views afterwards, turns it to the views sheet's turn by silhouette IoU over the 24 axis-aligned turns and fine yaw, and scales it, so rotation and size are not guessed by eye; orient upright stands a mesh tilted off level from its shape alone, thinnest principal axis up and the taller end of its long axis to -Y, and {upright: true, path: sheet} then searches only the yaw); components then removes what the generator added (keep_largest, remove_small floaters, or a rider cut off the body at its thinnest neck between seed and anchor and deleted or split out), for a small creature its coverage option keeps cells by their solid share on a finer voxelization (legs and fins do not merge into lumps) with keepThin keeping parts thinner than a cell one cell thick, reduce_palette cuts its noisy colours to a few clusters in Oklab (keeping small accents such as eyes) or snaps them to a palette, and despeckles the surface with a majority filter (import_mesh clean does both at import), deterrace breaks the one-cell contour terraces a smooth mesh leaves (blocks re-quantizes into 2- or 3-cell cubes for the chunky concept-cube look; stagger staggers the step edges in seeded teeth), and the tools below finish it. Statues, creatures, topiary, and other organic forms built from a body, a head, eyes, and limbs should not read as separate balls and bricks: blend_shapes fills spheres, ellipsoids, rounded boxes, tapered capsules, and tori blended as signed distance fields with a smooth minimum (k cells of fillet, so a head melts into a body like clay), subtracts shapes softly (a mouth groove, eye sockets), colours each cell from the shape it is deepest inside, and takes mirror (tests/fixtures/examples/blob_snowman.rhai). Organic forms (trunks, roots, limbs, rocks) should not read as pipes or boxes: loft builds a trunk or limb whose elliptical, flattened, knobbly section changes along a curve; deform bends, tapers, twists, or displaces (lumps, burls) an object or a box, and scales a part about a point with a smooth falloff (a small creature's head, eyes, or fins made larger than life so they read at 20 to 60 cells; minFeature reports parts thinner than N cells); brush inflates, deflates, smooths, flattens, or grabs the surface along a path of points to refine the silhouette (tests/fixtures/examples/fallen_tree.rhai). Stone surfaces: column_relief breaks a cliff face into columns of uneven width, relief, and height, joints cuts real joint gaps (columns, courses, cracks), weather chips edges, dents pits, and adds colour noise, and moss, algae, and vines grow plants on exposed faces (the caller gives the slots and their motion); moss style cushions grows separate cushions where the surface faces up, with drapes from overhangs, never one band. Wood surfaces: build the wood in one slot, then bark cuts plates along its grain (the same points as its fill_tube, or an axis on a flat deck or plank) split by fissures with dirt at the bottom, instead of painted stripes, and cavity_tint darkens fissures, crevices, and root crooks (and lightens ridges) by neighbour occupancy, making the shades itself; it gives rock depth too. tests/fixtures/examples/barked_trunk.rhai uses them. Architecture and turned pieces: radial_relief cuts regular shallow recesses repeated evenly round an axis, flutes along a column, pilaster, vase, urn, or barrel side (round or square, following the cells' outer surface, ends cupped or tapered over fade cells) or gores dished between the proud ribs of a dome (fading at the crown and the foot), with a u, v, or flat section 0.5 to 4 cells deep, and carries the slope in colour (shade tones the floors, light the slopes by the ridges, ribs paints and ribHeight raises the lines between them), instead of surface_stroke one flat groove at a time (tests/fixtures/examples/fluted_column.rhai, gored_dome.rhai). Decks, floors, and docks: planks lays separate boards over a rectangle at a height, with real gaps down to a dirt cell, end joints staggered between neighbouring rows, a tone per board, and optional ragged ends, nails, and knots, instead of dozens of fill_box and carve calls or painted stripes (tests/fixtures/examples/plank_dock.rhai). Tree crowns: foliage packs many separate leaf clumps over ellipsoids (or re-clusters an existing crown's slots) with lit tops, dark undersides, and a dark core in the gaps, leaving the wood alone, instead of one blob of fill_ellipsoid calls (tests/fixtures/examples/clustered_tree.rhai). Their exclude boxes (and scatter's, bark's, planks', and cavity_tint's) keep walking surfaces whole, with no refill afterwards. sweep fills a cross-section along a curve for curved walls, banks, and ledges, and with palette 0 erases one, for a slanted cut along a curved outline. trim_outline reshapes a finished form from outside: state a closed plan outline and [z, inset] pairs once, and every solid cell outside that surface goes, with a new skin along the cut if asked, so a hull narrows toward its keel with no tubes fitted by hand (tests/fixtures/examples/trimmed_hull.rhai). For a unique organic asset from a concept's turnaround sheet, measure_sheet first reads the sheet (each view's crop as fitViews, the cube pitch per view, front/side/top agreement in percent, and with parts the top view's flippers, head, and tail) and writes no file, so never measure a sheet with scripts; carve_from_views fills the visual hull of its front, side, and top silhouettes (PNG crops) as the rough form, and with a depth map per view (depthPath: a grey depth pass of the sheet, white near, from an image generator or Depth Anything V2, calibrated to the hull by default) carves away the cells in front of each view's surface, so heads and domes come out round, project_paint then recolours the surface one view sees from a single image (a shell's plates from the top view, markings from the front) without adding or removing cells, paint_from_views recolours every surface cell from the whole sheet in one call (each cell from the views that see it, by the view its normal faces most, blended at seams, quantised into the object's slots or a few new ones with eyes and mouths kept), which gives an image-to-3D import smeared on its hidden sides the approved colours all round, for a small creature (20 to 60 cells long) pixelate_views turns the views into pixel art at the cell grid first (a few shared Oklab colours, the majority colour per cell, eyes, pupils, teeth, nostrils, and spots kept at least a cell, parts thinner than a cell kept inked; rows and legend per view, and a png to check) and downscale on carve_from_views and paint_from_views does the same in one call, so eyes do not average into smears and thin legs do not vanish, optimize_to_views then iterates such a small creature's shape (excess columns cleared, missing rays given a cell touching the body, within band cells, connected, never thinner than minFeature) and surface colours (a vote of the views that see each cell weighted by how squarely they face it, smoothed into patches, eyes kept as accents, edge cells two views paint differently carved off) until its views agree with the pixel art (the views' crops pixelated at the object's box, or pixelate_views' rows), replying with each view's agreement before and after (semantic adds a CLIP patch-similarity term on a froxel built with the clip-metal feature), decal lays an eye, a mouth, or a badge onto a curved part around a point (or a ray from the camera) so it keeps its size and shape where a projection smears it on the slopes (mirror for the pair of eyes), fit_to_views refines a blockout (from carve_from_views or import_mesh) so its orthographic views match the sheet's views (excess and missing rays, an optional reference object's depth, and colours from the object's slots), replying with each view's IoU and colour error before and after, and compare_silhouette scores the model against a concept view from a preview camera (IoU, missing, excess) and writes an overlay PNG; check it after every step instead of comparing heights. A hero view with water, splashes, or a rider needs its background masked first (excludeColors, flood, mask, erase, keepLargest, on compare_silhouette and every view), or its IoU means nothing; white eyes or teeth on a white sheet are keyed out as background and come back with fillHoles. split_parts then cuts that one blockout into all its rig parts in one call (head, jaw, body, four limbs, tail): each cell goes to the part whose named seed point, bone, or marker is nearest through the solid (geodesic), with optional cutting planes, and each part becomes its own object at world [0, 0, 0] ready for bind_object; split_object moves the cells inside boxes, or of some slots, into a new or existing object, keeping their world positions. After bind_object, place_joints puts each bone's pivot where its part meets its parent's part (contact faces, overlap, or the middle of the gap), so pivots are never placed by eye. set_marker names points, collision boxes, and connectors that the .vox keeps and the glb carries as nodes; collision_from_voxels builds the collision boxes from an object's solid cells within a tolerance (holeSize, layer or zBands, minFeature, only, exclude, maxBoxes), keeps hollows open, replaces its own earlier boxes, and replies with how much solid is covered and how much empty is wrongly filled, so colliders never drift from the model (check them with preview overlay boxes); export_variants writes a .vox and .glb per parameter set and merges them by name into a manifest of parameters, markers, and bounds (replace: true starts it fresh); its reply, like export_mesh's and export_models', counts the markers by kind instead of listing them (reply: \"full\" for every one, or read the manifest file or list_markers). tests/fixtures/examples/gorge_wall.rhai uses them all. If a tool the docs mention is missing, this server is older than the froxel binary on disk: reload re-executes the rebuilt binary in place, keeping the connection and the open document, and sends notifications/tools/list_changed (--watch does it by itself). A Rhai script (set_script) can call the same sculpt, voxel, object, rig, clip, and palette tools, with the MCP options in a trailing map, for a family of organic pieces (docs/scripting.md, tests/fixtures/examples/hollow_log.rhai)."
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
        "resources/list" => json!({"resources": []}),
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
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    if !args.is_object() {
        return Err("arguments must be an object".into());
    }
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
    crate::ops::schema::tools()
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
active object's local cells. tools/list describes every tool; docs/mcp.md has examples."
    );
}

#[cfg(test)]
#[path = "../tests/mcp.rs"]
mod tests;
