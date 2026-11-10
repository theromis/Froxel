//! `froxel render`, `froxel preview`, `froxel export`, `froxel import`,
//! `froxel carve`, `froxel paint`, `froxel fit`, `froxel compare`, and
//! `froxel voxrt`.

use std::path::{Path, PathBuf};

use crate::state::{rotation_codec_errors, GlassMode, Session};
use crate::still::Still;
use crate::trace;

pub fn render(args: &[String]) -> i32 {
    match render_args(args) {
        Ok(None) => 0,
        Ok(Some(job)) => match run_render(&job) {
            Ok(()) => 0,
            Err(err) => {
                eprintln!("{err}");
                1
            }
        },
        Err(err) => {
            eprintln!("{err}");
            2
        }
    }
}

pub fn voxrt(args: &[String]) -> i32 {
    match voxrt_args(args) {
        Ok(None) => 0,
        Ok(Some(job)) => match run_voxrt(&job) {
            Ok(()) => 0,
            Err(err) => {
                eprintln!("{err}");
                1
            }
        },
        Err(err) => {
            eprintln!("{err}");
            2
        }
    }
}

struct RenderJob {
    input: PathBuf,
    output: PathBuf,
    still: Still,
    albedo: Option<PathBuf>,
    normal: Option<PathBuf>,
    gpu: bool,
}

struct VoxJob {
    input: PathBuf,
    dump: bool,
}

/// `froxel export`: a `.vox` to a `.glb` with the `export_mesh` options.
pub fn export(args: &[String]) -> i32 {
    match export_args(args) {
        Ok(None) => 0,
        Ok(Some((input, output, options))) => {
            let mut session = Session::new();
            let written = session.open_vox(&input).and_then(|()| {
                if let Some(note) = session.scene_shift_note() {
                    eprintln!("{note}");
                }
                if let Some(parent) = output
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                {
                    std::fs::create_dir_all(parent)
                        .map_err(|err| format!("Could not create {}: {err}", parent.display()))?;
                }
                crate::export::export_with(&session, &output, &options)
            });
            match written {
                Ok(quads) => {
                    eprintln!("wrote {} ({quads} quads)", output.display());
                    0
                }
                Err(err) => {
                    eprintln!("{err}");
                    1
                }
            }
        }
        Err(err) => {
            eprintln!("{err}");
            2
        }
    }
}

const BAKE_SCENE_USAGE: &str = "\
usage: froxel bake-scene --scene <scene.json> --output <file.glb> [--bake JSON|FILE]

Bakes one placement of many .vox files together, so each is shaded by its
neighbours and by ground, into one merged mesh with one colour atlas, to draw
unshaded (MCP bake_scene). scene.json, with paths relative to its folder:

  {\"instances\": [{\"source\": \"stone.vox\", \"translation\": [2, 0, -1],
                   \"yaw\": 7, \"scale\": 0.1},
                  {\"source\": \"reed.vox\", \"translation\": [1, 0, 2],
                   \"lean\": {\"degrees\": 12, \"toward\": 180}, \"sway\": 1.6}],
   \"occluders\": [{\"mesh\": \"ground.glb\", \"translation\": [0, 0, 0]},
                 {\"plane\": 0},
                 {\"height\": {\"origin\": [-4, -4], \"cell\": 1,
                             \"rows\": [[0, 0, 0], [0, 1, 0], [0, 0, 0]]}}],
   \"bake\": {\"preset\": \"old_pond\", \"distance\": 2}}

A placement is the frame of the glb froxel export writes, as a game places it:
world = translation + Lean x scale x Ry(yaw) x (x, z, -y) of a cell corner, Y
up, with yaw in degrees about +Y (0 faces -Z). lean {degrees, toward}, on an
instance or a mesh occluder, tilts it by degrees (-90..90) about its base so its
top moves toward the yaw toward (0 is -Z, 90 is -X); without it, upright. sway
(0..4, default 1) on an instance multiplies the sway of its moving slots. Occluders stop light and are not written: mesh is a
.glb, .gltf or .obj, plane is solid below that height, height is y at grid
points, rows[z][x]. bake is the option of froxel export, with distance in scene
units, and --bake replaces it. Without either it is occlusion alone. It prints
the triangles, the atlas size and the memory used. See docs/mcp.md.";

/// `froxel bake-scene`: MCP `bake_scene` with no window.
pub fn bake_scene(args: &[String]) -> i32 {
    match bake_scene_args(args) {
        Ok(None) => 0,
        Ok(Some(params)) => {
            let started = std::time::Instant::now();
            match crate::remote::bake_scene(&params) {
                Ok(reply) => {
                    eprintln!(
                        "wrote {} ({} instances, {} triangles, {}x{} atlas, {} ms)",
                        reply["path"].as_str().unwrap_or(""),
                        reply["instances"],
                        reply["triangles"],
                        reply["atlas"][0],
                        reply["atlas"][1],
                        started.elapsed().as_millis()
                    );
                    0
                }
                Err(err) => {
                    eprintln!("{err}");
                    1
                }
            }
        }
        Err(err) => {
            eprintln!("{err}");
            2
        }
    }
}

fn bake_scene_args(args: &[String]) -> Result<Option<serde_json::Value>, String> {
    use serde_json::json;
    let mut params = json!({});
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        let mut value = || {
            index += 1;
            args.get(index)
                .cloned()
                .ok_or_else(|| format!("missing value for {arg}"))
        };
        match arg {
            "-h" | "--help" => {
                eprintln!("{BAKE_SCENE_USAGE}");
                return Ok(None);
            }
            "--scene" => params["scene"] = json!(value()?),
            "--output" => params["path"] = json!(value()?),
            "--bake" => {
                let text = value()?;
                params["bake"] = crate::bridge::parse_bake(Some(&text))?;
            }
            other => return Err(format!("unknown argument `{other}`\n{BAKE_SCENE_USAGE}")),
        }
        index += 1;
    }
    if params.get("scene").is_none() || params.get("path").is_none() {
        return Err(BAKE_SCENE_USAGE.into());
    }
    Ok(Some(params))
}

/// `froxel preview`: the fast lit raster of MCP `preview`, or a contact
/// sheet of several inputs or of the variants of one.
pub fn preview(args: &[String]) -> i32 {
    match preview_args(args) {
        Ok(None) => 0,
        Ok(Some((open, params))) => {
            let started = std::time::Instant::now();
            let mut session = Session::new();
            let written = match &open {
                Some(input) => session.open_vox(input),
                None => Ok(()),
            }
            .and_then(|()| crate::remote::apply(&mut session, "render.preview", &params));
            match written {
                Ok(reply) => {
                    let path = reply["path"].as_str().unwrap_or("");
                    match reply["tiles"].as_array() {
                        Some(tiles) => eprintln!(
                            "wrote {path} ({}x{}, {} tiles, {} ms)",
                            reply["width"],
                            reply["height"],
                            tiles.len(),
                            started.elapsed().as_millis()
                        ),
                        None => eprintln!(
                            "wrote {path} ({} ms)\n{}",
                            started.elapsed().as_millis(),
                            counts_text(&reply)
                        ),
                    }
                    0
                }
                Err(err) => {
                    eprintln!("{err}");
                    1
                }
            }
        }
        Err(err) => {
            eprintln!("{err}");
            2
        }
    }
}

/// The two triangle counts of a preview reply, one per line.
fn counts_text(reply: &serde_json::Value) -> String {
    let mut text = format!(
        "preview: {} triangles (the unmerged viewport mesh it draws)",
        reply["previewTriangles"]
    );
    let export = &reply["export"];
    if let Some(err) = export["error"].as_str() {
        text += &format!("\nexport: {err}");
    } else if !export.is_null() {
        text += &format!(
            "\nexport: {} triangles, {} quads (merge {}, colorAtlas {}, as froxel export writes it)",
            export["triangles"], export["quads"], export["merge"], export["colorAtlas"]
        );
    }
    text
}

const PREVIEW_USAGE: &str = "\
usage: froxel preview --input <file.vox|file.glb> [--input <file.vox> ...] --out <file.png>
                      [--width N] [--height N] [--yaw DEG] [--pitch DEG]
                      [--view front|back|left|right|top|bottom]
                      [--preset game|default|NAME] [--presets <presets.json>]
                      [--placement-yaw DEG]
                      [--lighting viewport|old_pond|NAME] [--lighting-presets <lighting.json>]
                      [--ortho] [--fov DEG] [--margin F]
                      [--object NAME|INDEX] [--isolate]
                      [--background #rrggbb[aa]] [--no-antialias]
                      [--variants <sets.json>] [--reference <concept.png>]
                      [--reference-label TEXT] [--columns N] [--no-labels]
                      [--overlay all|markers|boxes|points|connectors]...
                      [--no-overlay-labels] [--counts-label] [--no-export-counts]
                      [--merge] [--color-atlas] [--vertex-colors] [--export-hidden]
                      [--split-objects] [--ao STRENGTH] [--ao-radius CELLS]
                      [--ao-target vertex|texture|both] [--bake JSON|FILE]

The editor viewport's mesh and light (sun plus ambient), rasterised on the
CPU: no path tracer, no window, no GPU. The camera orbits at yaw and pitch
and fits the model, or --object, inside the image with --margin (fraction
of the image per side, default 0.06). --isolate draws only --object.
Defaults: 1024x1024 (1..4096), yaw 30, pitch 25, perspective with the
viewport's 50 degree vertical --fov (10..120), the viewport grey
background, and 2x2 antialiasing up to 2048x2048. --output is an alias of
--out.

--preset game is the gameplay camera: frog-world's, orthographic, yaw -135,
pitch 32. --presets reads a JSON object of name: {yaw, pitch, godotLook,
orthographic, fov, margin} (or one with it under presets) that defines
presets or overrides the built-in ones, so a project's own game camera is
one file; the user's camera_presets.json in Froxel's support folder (or
FROXEL_CAMERA_PRESETS) is read under it. --yaw, --pitch, --ortho, --fov, and
--margin still win over a preset. --placement-yaw shows the asset as it is
when placed turned that many degrees about up (a Godot rotation_degrees.y),
from the same camera: the camera and a --lighting sun turn the other way.

--lighting lights the model as a game does instead of with the viewport's
light: a sun (direction, colour, energy), a flat ambient (colour, energy),
exposure, an optional background, and an optional LUT grade, with no
shadows. old_pond is frog-world's Old Pond region; viewport is the default.
--lighting-presets reads a JSON object of name: {sunDirection or
godotSunBasis, sunColor, sunEnergy, ambientColor, ambientEnergy, exposure,
background, lut, preset}; the user's lighting_presets.json in Froxel's
support folder (or FROXEL_LIGHTING_PRESETS) is read as well. --background
still wins over a preset's.

An --input ending in .glb draws that written file instead, as an unlit material
does: nearest texel times COLOR_0, no light. Use it to look at a baked export
or bake-scene file; the camera flags work, and --lighting and --object do not.

A contact sheet: several --input files, --variants (the parameter sets of
froxel variants, baked from the one --input), or --reference (a PNG placed
as the first tile, labelled with --reference-label or its file name) draw
one tile per model in a grid of --columns (default about square) with each
tile's name and parameters under it unless --no-labels. --width and
--height are then each tile's size (default 512x512); the sheet is at most
8192 pixels a side and holds at most 64 models.

--overlay draws the markers over the model: boxes (collision boxes as
outlines), points, connectors (an arrow toward their direction), or all
(markers is the same), each named unless --no-overlay-labels. What the
model hides is drawn faded and dashed. Repeat it to combine kinds. MCP
collision_from_voxels builds collision boxes from a model's cells to check.

Two triangle counts are printed and they are not the same mesh. preview is
the unmerged viewport mesh the image draws (every exposed cell face).
export is what froxel export (MCP export_mesh, export_models) writes with
the export flags given here, the same flags and defaults as froxel export:
--merge and --color-atlas usually cut it several times. The counts are of
the whole document, whatever --object frames. --counts-label also writes
both in the image's bottom-left corner; --no-export-counts skips the export
count.";

#[allow(clippy::type_complexity)]
fn preview_args(args: &[String]) -> Result<Option<(Option<PathBuf>, serde_json::Value)>, String> {
    use serde_json::{json, Value};
    let mut inputs: Vec<PathBuf> = Vec::new();
    let mut params = json!({});
    let mut overlay: Vec<Value> = Vec::new();
    let mut export = json!({});
    let mut no_counts = false;
    let mut angles = false;
    let mut view = false;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        let value = |index: &mut usize| -> Result<String, String> {
            *index += 1;
            args.get(*index)
                .cloned()
                .ok_or_else(|| format!("missing value for {arg}"))
        };
        let max = crate::preview::MAX_SIZE;
        match arg {
            "-h" | "--help" => {
                eprintln!("{PREVIEW_USAGE}");
                return Ok(None);
            }
            "--input" => inputs.push(PathBuf::from(value(&mut index)?)),
            "--out" | "--output" => params["path"] = json!(value(&mut index)?),
            "--width" => params["width"] = json!(parse_u32(&value(&mut index)?, arg, 1, max)?),
            "--height" => params["height"] = json!(parse_u32(&value(&mut index)?, arg, 1, max)?),
            "--yaw" => {
                params["yaw"] = json!(parse_f32(&value(&mut index)?, arg)?);
                angles = true;
            }
            "--pitch" => {
                params["pitch"] = json!(parse_f32(&value(&mut index)?, arg)?);
                angles = true;
            }
            "--view" => {
                let name = value(&mut index)?;
                crate::remote::named_view(&name)?;
                params["view"] = json!(name);
                view = true;
            }
            "--preset" => params["preset"] = json!(value(&mut index)?),
            "--presets" => params["presets"] = json!(value(&mut index)?),
            "--placement-yaw" => {
                params["placementYaw"] = json!(parse_f32(&value(&mut index)?, arg)?)
            }
            "--lighting" => params["lighting"] = json!(value(&mut index)?),
            "--lighting-presets" => params["lightingPresets"] = json!(value(&mut index)?),
            "--ortho" | "--orthographic" => params["orthographic"] = json!(true),
            "--fov" => params["fov"] = json!(parse_f32(&value(&mut index)?, arg)?),
            "--margin" => params["margin"] = json!(parse_f32(&value(&mut index)?, arg)?),
            "--object" => {
                let key = value(&mut index)?;
                params["object"] = match key.parse::<u64>() {
                    Ok(number) => json!(number),
                    Err(_) => json!(key),
                };
            }
            "--isolate" => params["isolate"] = json!(true),
            "--background" => {
                let text = value(&mut index)?;
                crate::remote::background(&json!({"background": text}))?;
                params["background"] = json!(text);
            }
            "--no-antialias" => params["antialias"] = json!(false),
            "--variants" => {
                params["variants"] =
                    crate::variants::read_sets(std::path::Path::new(&value(&mut index)?))?;
            }
            "--reference" => params["reference"] = json!({"path": value(&mut index)?}),
            "--reference-label" => params["referenceLabel"] = json!(value(&mut index)?),
            "--columns" => {
                params["columns"] = json!(parse_u32(
                    &value(&mut index)?,
                    arg,
                    1,
                    crate::sheet::MAX_TILES as u32
                )?)
            }
            "--no-labels" => params["labels"] = json!(false),
            "--overlay" => overlay.push(json!(value(&mut index)?)),
            "--no-overlay-labels" => params["overlayLabels"] = json!(false),
            "--counts-label" => params["countsLabel"] = json!(true),
            "--no-export-counts" => no_counts = true,
            "--merge" => export["merge"] = json!(true),
            "--color-atlas" => export["colorAtlas"] = json!(true),
            "--vertex-colors" => export["vertexColors"] = json!(true),
            "--export-hidden" => export["exportHidden"] = json!(true),
            "--split-objects" => export["splitObjects"] = json!(true),
            "--ao" => {
                let text = value(&mut index)?;
                export["aoStrength"] = json!(crate::bridge::parse_ao_strength(Some(&text))?);
            }
            "--ao-radius" => {
                let text = value(&mut index)?;
                export["aoRadius"] = json!(crate::bridge::parse_ao_radius(Some(&text))?);
            }
            "--ao-target" => export["aoTarget"] = json!(value(&mut index)?),
            "--bake" => {
                let text = value(&mut index)?;
                export["bake"] = crate::bridge::parse_bake(Some(&text))?;
            }
            other => return Err(format!("unknown argument `{other}`\n{PREVIEW_USAGE}")),
        }
        index += 1;
    }
    if angles && view {
        return Err("pass --view or --yaw and --pitch, not both".into());
    }
    if params.get("isolate").is_some() && params.get("object").is_none() {
        return Err("--isolate needs --object".into());
    }
    if !overlay.is_empty() {
        params["overlay"] = json!(overlay);
    }
    if no_counts {
        if export.as_object().is_some_and(|map| !map.is_empty()) {
            return Err("--no-export-counts takes no export flags".into());
        }
        params["export"] = json!(false);
    } else {
        crate::remote::preview_export(&json!({"export": export}))?;
        params["export"] = export;
    }
    if let Some(label) = params
        .as_object_mut()
        .and_then(|map| map.remove("referenceLabel"))
    {
        if params.get("reference").is_none() {
            return Err("--reference-label needs --reference".into());
        }
        params["reference"]["label"] = label;
    }
    let Some(output) = params
        .get("path")
        .and_then(Value::as_str)
        .map(PathBuf::from)
    else {
        return Err(PREVIEW_USAGE.into());
    };
    if inputs.is_empty() {
        return Err(PREVIEW_USAGE.into());
    }
    if !output.extension().is_some_and(|ext| ext == "png") {
        return Err("--out must end in .png".into());
    }
    if inputs.len() == 1 && inputs[0].extension().is_some_and(|ext| ext == "glb") {
        // A written file, drawn unlit as a game draws a baked one.
        params["glb"] = json!(inputs[0].display().to_string());
        params["export"] = json!(false);
        return Ok(Some((None, params)));
    }
    if inputs.len() == 1 {
        return Ok(Some((inputs.pop(), params)));
    }
    if params.get("variants").is_some() {
        return Err("--variants bakes one --input; pass a single one".into());
    }
    params["inputs"] = json!(inputs
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>());
    Ok(Some((None, params)))
}

const EXPORT_USAGE: &str = "\
usage: froxel export --input <file.vox> --output <file.glb>
                     [--merge] [--vertex-colors] [--color-atlas]
                     [--export-hidden] [--split-objects]
                     [--ao STRENGTH] [--ao-radius CELLS]
                     [--ao-target vertex|texture|both] [--bake JSON|FILE]

The same options as the MCP tool export_mesh, all off by default.
--ao bakes ambient occlusion: 0..1, the light a fully occluded corner loses.
--ao-radius is its hemisphere reach in cells (0..8, default 3; 0 or 1 is the
corner term alone). --ao-target vertex writes it into COLOR_0 per corner
(faces that shade differently stay unmerged); texture darkens each cell's
colour atlas texel and merges as without occlusion (needs --color-atlas);
both writes the two. The default is texture with --color-atlas, else vertex.
--bake paints light into the colour atlas by real rays, one texel per cell
face, to draw unshaded (needs --color-atlas). It takes the export_mesh bake
option as a JSON object or a JSON file: occlusion, distance, rays, preset or
sunDirection, sunColor, sunEnergy, ambientColor, ambientEnergy, sunSoftness,
sunRays. --bake '{}' is occlusion alone. See docs/mcp.md.
If the input opens with a scene shift (world positions differ from its node
positions: a foreign import's centring, or a save that kept it), it is printed.";

fn export_args(
    args: &[String],
) -> Result<Option<(PathBuf, PathBuf, crate::export::MeshExport)>, String> {
    let mut input = None;
    let mut output = None;
    let mut options = crate::export::MeshExport {
        vertex_colors: false,
        merge: false,
        export_hidden: false,
        split_objects: false,
        color_atlas: false,
        ao_strength: 0.0,
        ao_radius: crate::bridge::DEFAULT_AO_RADIUS,
        ao_target: crate::export::AoTarget::Auto,
        bake: None,
    };
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "-h" | "--help" => {
                eprintln!("{EXPORT_USAGE}");
                return Ok(None);
            }
            "--input" => {
                index += 1;
                input = Some(PathBuf::from(
                    args.get(index).ok_or("--input needs a .vox path")?,
                ));
            }
            "--output" => {
                index += 1;
                output = Some(PathBuf::from(
                    args.get(index).ok_or("--output needs a .glb path")?,
                ));
            }
            "--merge" => options.merge = true,
            "--vertex-colors" => options.vertex_colors = true,
            "--color-atlas" => options.color_atlas = true,
            "--export-hidden" => options.export_hidden = true,
            "--split-objects" => options.split_objects = true,
            "--ao" => {
                index += 1;
                options.ao_strength =
                    crate::bridge::parse_ao_strength(args.get(index).map(String::as_str))?;
            }
            "--ao-radius" => {
                index += 1;
                options.ao_radius =
                    crate::bridge::parse_ao_radius(args.get(index).map(String::as_str))?;
            }
            "--ao-target" => {
                index += 1;
                options.ao_target =
                    crate::bridge::parse_ao_target(args.get(index).map(String::as_str))?;
            }
            "--bake" => {
                index += 1;
                let value = crate::bridge::parse_bake(args.get(index).map(String::as_str))?;
                options.bake = crate::export::bake::Bake::parse(Some(&value))?;
            }
            other => return Err(format!("unknown argument `{other}`\n{EXPORT_USAGE}")),
        }
        index += 1;
    }
    match (input, output) {
        (Some(input), Some(output)) if output.extension().is_some_and(|ext| ext == "glb") => {
            Ok(Some((input, output, options)))
        }
        (Some(_), Some(_)) => Err("--output must end in .glb".into()),
        _ => Err(EXPORT_USAGE.into()),
    }
}

fn render_args(args: &[String]) -> Result<Option<RenderJob>, String> {
    let mut input = None;
    let mut output = None;
    let mut width = 512u32;
    let mut height = 512u32;
    let mut samples = 64u32;
    let mut bounces = 5i32;
    let mut seed = 1u32;
    let mut aces = true;
    let mut denoise = false;
    let mut atmospheric = None;
    let mut sky_intensity = None;
    let mut sun_intensity = None;
    let mut aperture = None;
    let mut focus = None;
    let mut glass = None;
    let mut stochastic_gi = None;
    let mut yaw = 30.0f32;
    let mut pitch = 25.0f32;
    let mut distance = 1.0f32;
    let mut albedo = None;
    let mut normal = None;
    let mut gpu = false;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        let value = |index: &mut usize, name: &str| -> Result<String, String> {
            *index += 1;
            args.get(*index)
                .cloned()
                .ok_or_else(|| format!("missing value for {name}"))
        };
        match arg {
            "-h" | "--help" => {
                print_render_help();
                return Ok(None);
            }
            "--input" => input = Some(PathBuf::from(value(&mut index, arg)?)),
            "--output" => output = Some(PathBuf::from(value(&mut index, arg)?)),
            "--width" => width = parse_u32(&value(&mut index, arg)?, arg, 1, 8192)?,
            "--height" => height = parse_u32(&value(&mut index, arg)?, arg, 1, 8192)?,
            "--samples" => samples = parse_u32(&value(&mut index, arg)?, arg, 1, 8192)?,
            "--bounces" => bounces = parse_i32(&value(&mut index, arg)?, arg, 0, 8)?,
            "--seed" => {
                seed = value(&mut index, arg)?
                    .parse()
                    .map_err(|_| "invalid --seed".to_string())?;
            }
            "--no-aces" => aces = false,
            "--denoise" => denoise = true,
            "--atmospheric" => atmospheric = Some(true),
            "--sky-intensity" => sky_intensity = Some(parse_f32(&value(&mut index, arg)?, arg)?),
            "--sun-intensity" => sun_intensity = Some(parse_f32(&value(&mut index, arg)?, arg)?),
            "--aperture" => aperture = Some(parse_f32(&value(&mut index, arg)?, arg)?),
            "--focus" => focus = Some(parse_f32(&value(&mut index, arg)?, arg)?),
            "--glass-mode" => {
                glass = Some(parse_glass(&value(&mut index, arg)?)?);
            }
            "--no-stochastic-gi" | "--no-area-nee" => stochastic_gi = Some(false),
            "--yaw" => yaw = parse_f32(&value(&mut index, arg)?, arg)?,
            "--pitch" => {
                pitch = parse_f32(&value(&mut index, arg)?, arg)?;
                if !(-90.0..=90.0).contains(&pitch) {
                    return Err("--pitch is out of range".into());
                }
            }
            "--distance" => {
                distance = parse_f32(&value(&mut index, arg)?, arg)?;
                if distance <= 0.0 || distance > 100.0 {
                    return Err("--distance is out of range".into());
                }
            }
            "--aov-albedo" => albedo = Some(PathBuf::from(value(&mut index, arg)?)),
            "--aov-normal" => normal = Some(PathBuf::from(value(&mut index, arg)?)),
            "--gpu" => gpu = true,
            "--reference" | "--clamp-L" => {
                return Err(format!(
                    "{arg} was removed. See --help for the flags the Rust tracer still has."
                ));
            }
            other => return Err(format!("unknown arg: {other}")),
        }
        index += 1;
    }
    let (Some(input), Some(output)) = (input, output) else {
        print_render_help();
        return Err("missing --input or --output".into());
    };
    Ok(Some(RenderJob {
        input,
        output,
        still: Still {
            width,
            height,
            samples,
            bounces,
            seed,
            aces,
            denoise,
            atmospheric,
            sky_intensity,
            sun_intensity,
            aperture,
            focus,
            glass,
            stochastic_gi,
            yaw,
            pitch,
            distance,
            orthographic: None,
            frame: None,
            margin: crate::still::MARGIN,
            pixels_per_voxel: None,
        },
        albedo,
        normal,
        gpu,
    }))
}

fn print_render_help() {
    eprintln!(
        "\
usage: froxel render --input <file.vox> --output <file.png>
                       [--width N] [--height N]
                       [--samples N] [--bounces N] [--seed N]
                       [--no-aces] [--denoise]
                       [--aperture F] [--focus F]
                       [--glass-mode probabilistic|absorption]
                       [--sky-intensity F] [--sun-intensity F]
                       [--atmospheric]
                       [--no-stochastic-gi | --no-area-nee]
                       [--yaw DEG] [--pitch DEG] [--distance F]
                       [--aov-albedo <file.png>] [--aov-normal <file.png>]
                       [--gpu]

Defaults:
512×512, 64 samples, 5 bounces, seed 1, ACES on, denoise off,
probabilistic glass, stochastic GI on. Sky intensity, sun intensity,
aperture, and focus change only when the flag is present.
The camera orbits at yaw 30°, pitch 25° and fits the visible cells in the
frame; --distance multiplies that distance.

--no-area-nee is an alias of --no-stochastic-gi.

--gpu uses the wgpu tracer and exits with an error if no adapter is
available. The CPU tracer is byte-identical for a given seed."
    );
}

fn run_render(job: &RenderJob) -> Result<(), String> {
    let mut session = Session::new();
    session.open_vox(&job.input)?;
    let voxels = session
        .objects
        .iter()
        .filter(|object| !object.hidden)
        .map(|object| object.cells.len())
        .sum::<usize>();
    if voxels == 0 {
        return Err("document has no voxels".into());
    }
    eprintln!(
        "rendering {} → {}  {}x{}  {} samples  {} bounces  seed={}{}",
        job.input.display(),
        job.output.display(),
        job.still.width,
        job.still.height,
        job.still.samples,
        job.still.bounces,
        job.still.seed,
        if job.gpu { "  gpu" } else { "" }
    );
    if job.gpu {
        let saved = crate::still::prepare(&mut session, &job.still);
        let frame = crate::trace_gpu::render_headless(
            &session,
            [job.still.width, job.still.height],
            job.still.seed,
        )?;
        crate::still::finish(&mut session, saved);
        trace::write_rgba_png(&job.output, frame.width, frame.height, &frame.rgba)?;
        write_guide(
            &job.albedo,
            frame.width,
            frame.height,
            &trace::albedo_rgba(&frame.albedo),
        )?;
        write_guide(
            &job.normal,
            frame.width,
            frame.height,
            &trace::normal_rgba(&frame.normal),
        )?;
    } else {
        let saved = crate::still::prepare(&mut session, &job.still);
        let image = trace::render_still(
            &session,
            job.still.width,
            job.still.height,
            job.still.samples,
            job.still.seed,
        );
        let written = trace::write_png(&job.output, &image, &session);
        let albedo = write_guide(
            &job.albedo,
            image.width,
            image.height,
            &trace::albedo_rgba(image.albedo()),
        );
        let normal = write_guide(
            &job.normal,
            image.width,
            image.height,
            &trace::normal_rgba(image.normal()),
        );
        crate::still::finish(&mut session, saved);
        written?;
        albedo?;
        normal?;
    }
    eprintln!("wrote {}", job.output.display());
    Ok(())
}

fn write_guide(path: &Option<PathBuf>, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let Some(path) = path else {
        return Ok(());
    };
    trace::write_rgba_png(path, width, height, rgba)?;
    eprintln!("wrote {}", path.display());
    Ok(())
}

fn voxrt_args(args: &[String]) -> Result<Option<VoxJob>, String> {
    let mut input = None;
    let mut dump = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "-h" | "--help" => {
                eprintln!("usage: froxel voxrt --input <file.vox> [--dump]");
                return Ok(None);
            }
            "--input" => {
                index += 1;
                input = Some(PathBuf::from(
                    args.get(index)
                        .ok_or_else(|| "missing value for --input".to_string())?,
                ));
            }
            "--dump" => dump = true,
            "--output" => {
                return Err(
                    "--output was removed; voxrt writes a temporary file when a check fails".into(),
                );
            }
            other => return Err(format!("unknown arg: {other}")),
        }
        index += 1;
    }
    let Some(input) = input else {
        return Err("usage: froxel voxrt --input <file.vox> [--dump]".into());
    };
    Ok(Some(VoxJob { input, dump }))
}

fn run_voxrt(job: &VoxJob) -> Result<(), String> {
    if job.dump {
        return dump_vox(&job.input);
    }
    check_round_trip(&job.input)
}

pub(crate) fn check_round_trip(path: &std::path::Path) -> Result<(), String> {
    let codec = rotation_codec_errors();
    if !codec.is_empty() {
        for line in &codec {
            eprintln!("voxrt: FAIL: {line}");
        }
        return Err(format!("voxrt: {} failure(s)", codec.len()));
    }
    eprintln!(
        "voxrt: rotation codec ok ({} valid bytes)",
        valid_rotations()
    );
    let mut first = Session::new();
    first.open_vox(path)?;
    let bytes = first.export_bytes()?;
    let mut second = Session::new();
    second.import_bytes(&bytes)?;
    let mut failures = first.round_trip_differences(&second);
    let again = second.export_bytes()?;
    if bytes != again {
        failures.push("save fixpoint: second save differs from first".into());
    }
    eprintln!("voxrt: {}: {} objects", path.display(), first.objects.len());
    if failures.is_empty() {
        eprintln!("voxrt: OK ({})", path.display());
        return Ok(());
    }
    for line in &failures {
        eprintln!("voxrt: FAIL: {line}");
    }
    let kept = std::env::temp_dir().join("froxel-voxrt.vox");
    let _ = std::fs::write(&kept, &bytes);
    Err(format!(
        "voxrt: {} failure(s); round-trip file kept at {}",
        failures.len(),
        kept.display()
    ))
}

fn valid_rotations() -> u32 {
    let mut checked = 0;
    for byte in 0..128u8 {
        let c0 = (byte & 3) as i32;
        let c1 = ((byte >> 2) & 3) as i32;
        let c2 = 3 - c0 - c1;
        if c0 != c1 && c0 <= 2 && c1 <= 2 && (0..=2).contains(&c2) {
            checked += 1;
        }
    }
    checked
}

fn dump_vox(path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(path)
        .map_err(|err| format!("voxrt: cannot read {}: {err}", path.display()))?;
    if bytes.len() < 20 || &bytes[..4] != b"VOX " {
        return Err(format!("voxrt: {} is not a .vox file", path.display()));
    }
    println!("{} ({} bytes)", path.display(), bytes.len());
    let children = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    let end = (20 + children).min(bytes.len());
    let mut pos = 20usize;
    while pos + 12 <= end {
        let id = &bytes[pos..pos + 4];
        let content = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let child = u32::from_le_bytes(bytes[pos + 8..pos + 12].try_into().unwrap()) as usize;
        let label = String::from_utf8_lossy(id);
        println!("  {label} content={content} child={child}");
        let body_end = (pos + 12 + content).min(end);
        if body_end > pos + 12 {
            dump_body(id, &bytes[pos + 12..body_end]);
        }
        pos += 12 + content + child;
    }
    Ok(())
}

fn dump_body(id: &[u8], body: &[u8]) {
    match id {
        b"SIZE" if body.len() >= 12 => {
            let x = i32::from_le_bytes(body[0..4].try_into().unwrap());
            let y = i32::from_le_bytes(body[4..8].try_into().unwrap());
            let z = i32::from_le_bytes(body[8..12].try_into().unwrap());
            println!("    size {x} {y} {z}");
        }
        b"XYZI" if body.len() >= 4 => {
            let n = u32::from_le_bytes(body[0..4].try_into().unwrap());
            println!("    voxels {n}");
        }
        b"RGBA" => println!("    palette {}", body.len() / 4),
        b"MATL" => {
            if let Some((slot, attrs)) = dict_after_id(body) {
                println!("    id={slot} attrs{{{attrs}}}");
            }
        }
        b"nTRN" => dump_trn(body),
        b"nGRP" => dump_grp(body),
        b"nSHP" => dump_shp(body),
        b"LAYR" => {
            if let Some((slot, attrs)) = dict_after_id(body) {
                println!("    id={slot} attrs{{{attrs}}}");
            }
        }
        b"rOBJ" => {
            if let Some(attrs) = dict_only(body) {
                println!("    attrs{{{attrs}}}");
            }
        }
        b"MVRG" => println!("    rig {} bytes", body.len()),
        b"MVSC" => println!("    script {} bytes", body.len()),
        _ => {}
    }
}

fn dump_trn(body: &[u8]) {
    let mut cursor = body;
    let Ok(id) = take_i32(&mut cursor) else {
        return;
    };
    let Ok(attrs) = take_dict(&mut cursor) else {
        return;
    };
    let Ok(child) = take_i32(&mut cursor) else {
        return;
    };
    let Ok(_reserved) = take_i32(&mut cursor) else {
        return;
    };
    let Ok(layer) = take_i32(&mut cursor) else {
        return;
    };
    let Ok(frames) = take_i32(&mut cursor) else {
        return;
    };
    println!("    id={id} child={child} layer={layer} frames={frames} attrs{{{attrs}}}");
    for index in 0..frames {
        let Ok(frame) = take_dict(&mut cursor) else {
            break;
        };
        println!("      frame {index} {{{frame}}}");
    }
}

fn dump_grp(body: &[u8]) {
    let mut cursor = body;
    let Ok(id) = take_i32(&mut cursor) else {
        return;
    };
    let Ok(attrs) = take_dict(&mut cursor) else {
        return;
    };
    let Ok(count) = take_i32(&mut cursor) else {
        return;
    };
    let mut children = Vec::new();
    for _ in 0..count {
        let Ok(child) = take_i32(&mut cursor) else {
            break;
        };
        children.push(child.to_string());
    }
    println!(
        "    id={id} attrs{{{attrs}}} children[{}]",
        children.join(" ")
    );
}

fn dump_shp(body: &[u8]) {
    let mut cursor = body;
    let Ok(id) = take_i32(&mut cursor) else {
        return;
    };
    let Ok(attrs) = take_dict(&mut cursor) else {
        return;
    };
    let Ok(count) = take_i32(&mut cursor) else {
        return;
    };
    let mut models = Vec::new();
    for _ in 0..count {
        let Ok(model) = take_i32(&mut cursor) else {
            break;
        };
        let Ok(model_attrs) = take_dict(&mut cursor) else {
            break;
        };
        models.push(format!("{model}{{{model_attrs}}}"));
    }
    println!("    id={id} attrs{{{attrs}}} models[{}]", models.join(" "));
}

fn dict_after_id(body: &[u8]) -> Option<(i32, String)> {
    let mut cursor = body;
    let id = take_i32(&mut cursor).ok()?;
    let attrs = take_dict(&mut cursor).ok()?;
    Some((id, attrs))
}

fn dict_only(body: &[u8]) -> Option<String> {
    let mut cursor = body;
    take_dict(&mut cursor).ok()
}

fn take_i32(cursor: &mut &[u8]) -> Result<i32, ()> {
    if cursor.len() < 4 {
        return Err(());
    }
    let value = i32::from_le_bytes(cursor[..4].try_into().unwrap());
    *cursor = &cursor[4..];
    Ok(value)
}

fn take_dict(cursor: &mut &[u8]) -> Result<String, ()> {
    let count = take_i32(cursor)?;
    if count < 0 {
        return Err(());
    }
    let mut out = String::new();
    for _ in 0..count {
        let key = take_string(cursor)?;
        let value = take_string(cursor)?;
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&key);
        out.push('=');
        out.push_str(&value);
    }
    Ok(out)
}

fn take_string(cursor: &mut &[u8]) -> Result<String, ()> {
    let len = take_i32(cursor)?;
    if len < 0 || cursor.len() < len as usize {
        return Err(());
    }
    let text = String::from_utf8_lossy(&cursor[..len as usize]).into_owned();
    *cursor = &cursor[len as usize..];
    Ok(text)
}

fn parse_glass(text: &str) -> Result<GlassMode, String> {
    match text {
        "probabilistic" => Ok(GlassMode::Probabilistic),
        "absorption" => Ok(GlassMode::Absorption),
        _ => Err("--glass-mode must be 'probabilistic' or 'absorption'".into()),
    }
}

fn parse_u32(text: &str, name: &str, lo: u32, hi: u32) -> Result<u32, String> {
    let value: u32 = text.parse().map_err(|_| format!("invalid {name}"))?;
    if value < lo || value > hi {
        return Err(format!("{name} is out of range"));
    }
    Ok(value)
}

fn parse_i32(text: &str, name: &str, lo: i32, hi: i32) -> Result<i32, String> {
    let value: i32 = text.parse().map_err(|_| format!("invalid {name}"))?;
    if value < lo || value > hi {
        return Err(format!("{name} is out of range"));
    }
    Ok(value)
}

fn parse_f32(text: &str, name: &str) -> Result<f32, String> {
    let value: f32 = text.parse().map_err(|_| format!("invalid {name}"))?;
    if !value.is_finite() {
        return Err(format!("invalid {name}"));
    }
    Ok(value)
}

/// `froxel carve`: MCP `carve_from_views` from a JSON file of its
/// arguments, written to a `.vox`.
pub fn carve(args: &[String]) -> i32 {
    match carve_args(args) {
        Ok(None) => 0,
        Ok(Some((input, output, params))) => {
            let mut session = Session::new();
            let done = match &input {
                Some(input) => session.open_vox(input),
                None => {
                    session.replace_empty();
                    Ok(())
                }
            }
            .and_then(|()| crate::remote::apply(&mut session, "voxel.carve_from_views", &params))
            .and_then(|reply| {
                session.write_vox(&output)?;
                Ok(reply)
            });
            match done {
                Ok(reply) => {
                    eprintln!(
                        "wrote {} ({} cells, box {}, bounds {})",
                        output.display(),
                        reply["cells"],
                        reply["size"],
                        reply["bounds"]
                    );
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&reply).unwrap_or_default()
                    );
                    0
                }
                Err(err) => {
                    eprintln!("{err}");
                    1
                }
            }
        }
        Err(err) => {
            eprintln!("{err}");
            2
        }
    }
}

const CARVE_HELP: &str = "\
usage: froxel carve --views <views.json> --out <file.vox> [--input <base.vox>]

Fills the visual hull of 2 to 6 orthographic views (front, side, top) into
the active object, as MCP carve_from_views does, saves the .vox, and
prints the reply (each view's grid, covered cells, and depth calibration).
views.json holds that tool's arguments: {views: [{view, path, crop,
cellsPerPixel | size, background, tolerance, flip, depth}, ...], path,
depthPath, size, origin, cellsPerPixel, palette | slots, coverage, shell,
smooth, mode, depthTolerance}. A view's depth is a grey depth map (white
near) of the same crop, true for the crop of depthPath, or {path | values,
crop, background, invert, gamma (or auto), near, far, calibrate: hull |
box, back, regions, edge, detail}:
cells in front of its surface are carved away, so a round head is round,
not a box. One view with depth is enough for a relief. A relative path
is relative to views.json. --input carves into a copy of that
.vox; without it the document is empty with the default palette.";

fn carve_args(
    args: &[String],
) -> Result<Option<(Option<PathBuf>, PathBuf, serde_json::Value)>, String> {
    let (mut views, mut output, mut input) = (None, None, None);
    let mut index = 0;
    while index < args.len() {
        let value = |index: &mut usize| -> Result<String, String> {
            *index += 1;
            args.get(*index)
                .cloned()
                .ok_or_else(|| format!("{} needs a value", args[*index - 1]))
        };
        match args[index].as_str() {
            "-h" | "--help" => {
                eprintln!("{CARVE_HELP}");
                return Ok(None);
            }
            "--views" => views = Some(PathBuf::from(value(&mut index)?)),
            "--out" | "--output" => output = Some(PathBuf::from(value(&mut index)?)),
            "--input" => input = Some(PathBuf::from(value(&mut index)?)),
            other => return Err(format!("unknown argument `{other}`\n\n{CARVE_HELP}")),
        }
        index += 1;
    }
    let views = views.ok_or_else(|| format!("--views is required\n\n{CARVE_HELP}"))?;
    let output = output.ok_or_else(|| format!("--out is required\n\n{CARVE_HELP}"))?;
    let text = std::fs::read_to_string(&views)
        .map_err(|err| format!("Could not read {}: {err}", views.display()))?;
    let mut params: serde_json::Value = serde_json::from_str(&text)
        .map_err(|err| format!("{} is not JSON: {err}", views.display()))?;
    views_relative(&mut params, views.parent().unwrap_or(Path::new("")));
    Ok(Some((input, output, params)))
}

/// The paint tools `froxel paint --tool` runs, by MCP name.
const PAINT_TOOLS: &[(&str, &str)] = &[
    ("project_paint", "voxel.project_paint"),
    ("decal", "voxel.decal"),
    ("shell_plates", "voxel.shell_plates"),
    ("paint_from_views", "voxel.paint_from_views"),
];

/// `froxel paint`: MCP `project_paint` (or another paint tool, `--tool`)
/// from a JSON file of its arguments, on a copy of a `.vox`.
pub fn paint(args: &[String]) -> i32 {
    match paint_args(args) {
        Ok(None) => 0,
        Ok(Some((input, output, params, method))) => {
            let mut session = Session::new();
            let done = session
                .open_vox(&input)
                .and_then(|()| crate::remote::apply(&mut session, method, &params))
                .and_then(|reply| {
                    session.write_vox(&output)?;
                    Ok(reply)
                });
            match done {
                Ok(reply) => {
                    eprintln!(
                        "wrote {} ({} cells painted, {} changed)",
                        output.display(),
                        reply.get("cells").unwrap_or(&reply["painted"]),
                        reply["changed"],
                    );
                    0
                }
                Err(err) => {
                    eprintln!("{err}");
                    1
                }
            }
        }
        Err(err) => {
            eprintln!("{err}");
            2
        }
    }
}

const PAINT_HELP: &str = "\
usage: froxel paint --args <paint.json> --input <base.vox> --out <file.vox> [--tool <name>]

Recolours the active object's surface with one MCP paint tool and saves the
.vox. Cells are never added or removed. paint.json holds that tool's
arguments. --tool picks it:
  project_paint (default)  one image projected along a view: {view | yaw +
      pitch, path, crop, background, tolerance, flip, cellsPerPixel |
      imageSize, origin, size, palette | slots, coverage, depth,
      normalLimit, only, mirror}
  decal  one picture laid onto the curved surface around a point, keeping
      its size and shape: {point | ray {origin, direction}, size, up, path,
      crop, background, tolerance, flip, palette | slots, coverage, depth,
      reach, normalLimit, only, mirror}
  shell_plates  a turtle-shell plate pattern over the dome's surface:
      {palettes {ring, seam, central, band, rim}, layout (rings | voronoi),
      rings, central, band, bandCount, rim, center, seamWidth, seamDepth,
      depth, facing, zRange, x0..z1, only, jitter, symmetric, seed}
  paint_from_views  every surface cell from a whole views sheet, each
      from the views that see it by the view its normal faces most:
      {views [{view, path | rows, crop}] | path alone (the measure_sheet
      layout), objects, blend (nearest | soft), seam, sharpness, fit (ink
      | crop), reach, mirror (a missing right view from the left), slots
      | newSlots (with accents, minShare, contrast, maxAccents), only,
      x0..z1}
mirror (true, an axis, or {axis, at}) paints both sides of a plane;
paint_from_views' mirror is true or false. A relative path is relative to
paint.json.";

type PaintArgs = (PathBuf, PathBuf, serde_json::Value, &'static str);

fn paint_args(args: &[String]) -> Result<Option<PaintArgs>, String> {
    let (mut file, mut output, mut input) = (None, None, None);
    let mut method = "voxel.project_paint";
    let mut index = 0;
    while index < args.len() {
        let value = |index: &mut usize| -> Result<String, String> {
            *index += 1;
            args.get(*index)
                .cloned()
                .ok_or_else(|| format!("{} needs a value", args[*index - 1]))
        };
        match args[index].as_str() {
            "-h" | "--help" => {
                eprintln!("{PAINT_HELP}");
                return Ok(None);
            }
            "--args" => file = Some(PathBuf::from(value(&mut index)?)),
            "--out" | "--output" => output = Some(PathBuf::from(value(&mut index)?)),
            "--input" => input = Some(PathBuf::from(value(&mut index)?)),
            "--tool" => {
                let name = value(&mut index)?;
                method = PAINT_TOOLS
                    .iter()
                    .find(|(tool, _)| *tool == name)
                    .map(|(_, method)| *method)
                    .ok_or_else(|| {
                        format!(
                            "--tool must be project_paint, decal, shell_plates, or paint_from_views, not `{name}`"
                        )
                    })?;
            }
            other => return Err(format!("unknown argument `{other}`\n\n{PAINT_HELP}")),
        }
        index += 1;
    }
    let file = file.ok_or_else(|| format!("--args is required\n\n{PAINT_HELP}"))?;
    let input = input.ok_or_else(|| format!("--input is required\n\n{PAINT_HELP}"))?;
    let output = output.ok_or_else(|| format!("--out is required\n\n{PAINT_HELP}"))?;
    let text = std::fs::read_to_string(&file)
        .map_err(|err| format!("Could not read {}: {err}", file.display()))?;
    let mut params: serde_json::Value = serde_json::from_str(&text)
        .map_err(|err| format!("{} is not JSON: {err}", file.display()))?;
    views_relative(&mut params, file.parent().unwrap_or(Path::new("")));
    Ok(Some((input, output, params, method)))
}

/// `froxel fit`: MCP `fit_to_views` from a JSON file of its arguments, on
/// a copy of a `.vox`.
pub fn fit(args: &[String]) -> i32 {
    match fit_args(args) {
        Ok(None) => 0,
        Ok(Some((input, output, object, params))) => {
            let mut session = Session::new();
            let done = session
                .open_vox(&input)
                .and_then(|()| {
                    if let Some(object) = &object {
                        session.selected = crate::preview::find_object(&session, object)?;
                    }
                    crate::remote::apply(&mut session, "voxel.fit_to_views", &params)
                })
                .and_then(|reply| {
                    session.write_vox(&output)?;
                    Ok(reply)
                });
            match done {
                Ok(reply) => {
                    for view in reply["views"].as_array().into_iter().flatten() {
                        eprintln!(
                            "{}: IoU {} -> {}, colour error {} -> {}",
                            view["view"].as_str().unwrap_or("?"),
                            view["before"]["iou"],
                            view["after"]["iou"],
                            view["before"]["colorError"],
                            view["after"]["colorError"]
                        );
                    }
                    eprintln!(
                        "wrote {} ({} added, {} removed, {} recoloured, {} sweeps)",
                        output.display(),
                        reply["added"],
                        reply["removed"],
                        reply["recolored"],
                        reply["sweeps"]
                    );
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&reply).unwrap_or_default()
                    );
                    0
                }
                Err(err) => {
                    eprintln!("{err}");
                    1
                }
            }
        }
        Err(err) => {
            eprintln!("{err}");
            2
        }
    }
}

const FIT_HELP: &str = "\
usage: froxel fit --args <fit.json> --input <base.vox> --out <file.vox> [--object <index|name>]

Refines the active object (or --object) so its orthographic views match a
views sheet, as MCP fit_to_views does, saves the .vox, and prints the reply
(per-view IoU, missing, excess, and colour error before and after, the
cells added, removed, and recoloured, and the palette used). fit.json holds
that tool's arguments: {views: [{view, path, crop, background, tolerance,
flip, align, color, depth}, ...], path, background, tolerance (for every view
without its own), depthPath, align, slots, coverage, iterations, seed,
margin, maskWeight, colorWeight, depthWeight, smoothWeight, shape, recolor,
normalLimit, keep, sharedScale, roundWeight, roundness, roundAxes,
reference, isolate, depthTolerance}. A view's depth (as in froxel carve)
is that view's depth target, in place of a reference object's front. Fit a part with a
hand-set face with recolor false, so the front and side views do not
paint its eyes twice.
A relative path is relative to fit.json.";

type FitArgs = (PathBuf, PathBuf, Option<String>, serde_json::Value);

fn fit_args(args: &[String]) -> Result<Option<FitArgs>, String> {
    let (mut file, mut output, mut input, mut object) = (None, None, None, None);
    let mut index = 0;
    while index < args.len() {
        let value = |index: &mut usize| -> Result<String, String> {
            *index += 1;
            args.get(*index)
                .cloned()
                .ok_or_else(|| format!("{} needs a value", args[*index - 1]))
        };
        match args[index].as_str() {
            "-h" | "--help" => {
                eprintln!("{FIT_HELP}");
                return Ok(None);
            }
            "--args" => file = Some(PathBuf::from(value(&mut index)?)),
            "--out" | "--output" => output = Some(PathBuf::from(value(&mut index)?)),
            "--input" => input = Some(PathBuf::from(value(&mut index)?)),
            "--object" => object = Some(value(&mut index)?),
            other => return Err(format!("unknown argument `{other}`\n\n{FIT_HELP}")),
        }
        index += 1;
    }
    let file = file.ok_or_else(|| format!("--args is required\n\n{FIT_HELP}"))?;
    let input = input.ok_or_else(|| format!("--input is required\n\n{FIT_HELP}"))?;
    let output = output.ok_or_else(|| format!("--out is required\n\n{FIT_HELP}"))?;
    let text = std::fs::read_to_string(&file)
        .map_err(|err| format!("Could not read {}: {err}", file.display()))?;
    let mut params: serde_json::Value = serde_json::from_str(&text)
        .map_err(|err| format!("{} is not JSON: {err}", file.display()))?;
    views_relative(&mut params, file.parent().unwrap_or(Path::new("")));
    Ok(Some((input, output, object, params)))
}

/// Resolve a view's `depth` path, and its `detail` map's.
fn fix_depth(depth: &mut serde_json::Value, fix: &dyn Fn(&mut serde_json::Value)) {
    match depth {
        serde_json::Value::String(_) => fix(depth),
        serde_json::Value::Object(map) => {
            if let Some(value) = map.get_mut("path") {
                fix(value);
            }
            if let Some(detail) = map.get_mut("detail") {
                fix_depth(detail, fix);
            }
        }
        _ => {}
    }
}

/// Resolve the image paths of `carve_from_views` and `fit_to_views`
/// arguments against `base`: `path` and `depthPath` at the top, and each
/// view's `path` and `depth` (a path, or an object with one).
fn views_relative(params: &mut serde_json::Value, base: &Path) {
    let fix = |value: &mut serde_json::Value| {
        if let Some(path) = value.as_str() {
            if Path::new(path).is_relative() {
                *value = serde_json::json!(base.join(path).display().to_string());
            }
        }
    };
    for key in ["path", "depthPath"] {
        if let Some(value) = params.get_mut(key) {
            fix(value);
        }
    }
    if let Some(list) = params.get_mut("views").and_then(|v| v.as_array_mut()) {
        for view in list {
            if let Some(value) = view.get_mut("path") {
                fix(value);
            }
            if let Some(depth) = view.get_mut("depth") {
                fix_depth(depth, &fix);
            }
        }
    }
}

/// `froxel pixelate`: MCP `pixelate_views` on a views sheet.
pub fn pixelate(args: &[String]) -> i32 {
    match pixelate_args(args) {
        Ok(None) => 0,
        Ok(Some((params, out))) => {
            let mut session = Session::new();
            match crate::remote::apply(&mut session, "image.pixelate_views", &params) {
                Ok(reply) => {
                    let text = format!("{reply:#}");
                    match out {
                        Some(path) => {
                            if let Err(err) = crate::state::create_parent(&path).and_then(|_| {
                                std::fs::write(&path, &text).map_err(|err| {
                                    format!("Could not write {}: {err}", path.display())
                                })
                            }) {
                                eprintln!("{err}");
                                return 1;
                            }
                            for view in reply["views"].as_array().into_iter().flatten() {
                                println!(
                                    "{}: {} x {} cells, {} inked, {} thin, {} features",
                                    view["view"].as_str().unwrap_or("?"),
                                    view["grid"][0],
                                    view["grid"][1],
                                    view["inked"],
                                    view["thin"],
                                    view["features"].as_array().map_or(0, Vec::len)
                                );
                            }
                            println!("{} colours; wrote {}", reply["colors"], path.display());
                        }
                        None => println!("{text}"),
                    }
                    0
                }
                Err(err) => {
                    eprintln!("{err}");
                    1
                }
            }
        }
        Err(err) => {
            eprintln!("{err}");
            2
        }
    }
}

const PIXELATE_HELP: &str = "\
usage: froxel pixelate (--args <views.json> | --sheet <views.png>)
                       --size X,Y,Z [--colors N] [--no-features] [--no-thin]
                       [--out <views.json>] [--png <pixels.png>] [--png-scale N]

Turns an orthographic views sheet into pixel art at the cell grid, as MCP
pixelate_views does, and writes no voxels: one small palette shared by
every view (k-means in Oklab with the accent guard), each cell the colour
that holds most of its footprint, small salient features (eyes, pupils,
teeth, nostrils, spots) kept at least a cell, and a cell on the midline of a
part thinner than a cell (a fin, a leg) kept inked. --args is a JSON file of
pixelate_views arguments ({views: [{view, path, crop}], ...}); --sheet reads
a sheet's 2 x 2 layout as froxel measure finds it. --size gives the box in
cells, x (width), y (length), z (height); 0 or _ follows the views'
proportions, so --size _,20,_ makes a creature 20 cells long. --out writes
the reply, whose views (rows and legend) go straight into carve_from_views
and paint_from_views; without it the reply is printed. --png writes the
pixelated views side by side at --png-scale pixels a cell (default 8).";

pub(crate) fn pixelate_args(
    args: &[String],
) -> Result<Option<(serde_json::Value, Option<PathBuf>)>, String> {
    use serde_json::json;
    let mut params = json!({});
    let mut out = None;
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].clone();
        let mut value = || -> Result<String, String> {
            index += 1;
            args.get(index)
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "-h" | "--help" => {
                println!("{PIXELATE_HELP}");
                return Ok(None);
            }
            "--args" => {
                let file = value()?;
                let text = std::fs::read_to_string(&file)
                    .map_err(|err| format!("Could not read {file}: {err}"))?;
                let found: serde_json::Value = serde_json::from_str(&text)
                    .map_err(|err| format!("{file} is not JSON: {err}"))?;
                let Some(map) = found.as_object() else {
                    return Err(format!("{file} must hold a JSON object"));
                };
                for (key, v) in map {
                    params[key] = v.clone();
                }
            }
            "--sheet" => params["path"] = json!(value()?),
            "--size" => {
                let text = value()?;
                let parts: Vec<&str> = text.split(',').map(str::trim).collect();
                if parts.len() != 3 {
                    return Err(format!("--size needs X,Y,Z, not `{text}`"));
                }
                let mut size = Vec::new();
                for part in parts {
                    size.push(match part {
                        "_" | "0" | "" => serde_json::Value::Null,
                        n => json!(n
                            .parse::<u32>()
                            .map_err(|_| format!("--size needs whole numbers, not `{text}`"))?),
                    });
                }
                params["size"] = json!(size);
            }
            "--colors" => {
                params["colors"] = json!(value()?
                    .parse::<u32>()
                    .map_err(|_| "--colors needs a whole number".to_string())?)
            }
            "--no-features" => params["features"] = json!(false),
            "--no-thin" => params["thin"] = json!(false),
            "--out" => out = Some(PathBuf::from(value()?)),
            "--png" => params["png"] = json!(value()?),
            "--png-scale" => {
                params["pngScale"] = json!(value()?
                    .parse::<u32>()
                    .map_err(|_| "--png-scale needs a whole number".to_string())?)
            }
            other => return Err(format!("unknown flag {other}\n\n{PIXELATE_HELP}")),
        }
        index += 1;
    }
    if params.get("views").is_none() && params.get("path").is_none() {
        return Err(format!("pass --args or --sheet\n\n{PIXELATE_HELP}"));
    }
    if params.get("size").is_none() {
        return Err(format!("--size is required\n\n{PIXELATE_HELP}"));
    }
    Ok(Some((params, out)))
}

/// `froxel measure`: MCP `measure_sheet` on a views sheet.
pub fn measure(args: &[String]) -> i32 {
    match measure_args(args) {
        Ok(None) => 0,
        Ok(Some((params, as_json))) => {
            let mut session = Session::new();
            match crate::remote::apply(&mut session, "image.measure_sheet", &params) {
                Ok(reply) => {
                    if as_json {
                        println!("{reply:#}");
                    } else {
                        print!("{}", measure_text(&reply));
                    }
                    0
                }
                Err(err) => {
                    eprintln!("{err}");
                    1
                }
            }
        }
        Err(err) => {
            eprintln!("{err}");
            2
        }
    }
}

const MEASURE_HELP: &str = "\
usage: froxel measure --sheet <views.png> [--background auto|alpha|#rrggbb]
                      [--tolerance N] [--crop X,Y,W,H] [--gap PX]
                      [--min-share F] [--layout front,side,top,back]
                      [--pitch-range LOW,HIGH] [--parts [VIEW]]
                      [--check-tolerance PERCENT] [--flood N] [--json]

Reads a views sheet as MCP measure_sheet does and writes nothing: each
view's crop (its inked region's tight box), the 2 x 2 layout's names, the
cube pitch in pixels across and down (from the autocorrelation of the cube
seams), the size in cubes, and whether front height = side height, front
width = top width, and side length = top length (in pixels and in cubes,
within --check-tolerance percent, default 3). --parts counts the pieces
sticking out of the top view's body (or VIEW's): flippers, legs, a head, a
tail. --json prints the whole reply, whose fitViews go straight into
carve_from_views and fit_to_views.";

pub(crate) fn measure_args(args: &[String]) -> Result<Option<(serde_json::Value, bool)>, String> {
    use serde_json::json;
    let mut params = json!({});
    let mut as_json = false;
    let mut index = 0;
    let numbers = |text: &str, flag: &str, count: usize| -> Result<Vec<f64>, String> {
        let list: Vec<f64> = text
            .split(',')
            .map(|part| part.trim().parse::<f64>())
            .collect::<Result<_, _>>()
            .map_err(|_| format!("{flag} needs {count} numbers, not `{text}`"))?;
        if list.len() != count {
            return Err(format!("{flag} needs {count} numbers, not `{text}`"));
        }
        Ok(list)
    };
    while index < args.len() {
        let flag = args[index].clone();
        let mut value = || -> Result<String, String> {
            index += 1;
            args.get(index)
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "-h" | "--help" => {
                eprintln!("{MEASURE_HELP}");
                return Ok(None);
            }
            "--sheet" | "--input" => params["path"] = json!(value()?),
            "--background" => params["background"] = json!(value()?),
            "--tolerance" => params["tolerance"] = json!(numbers(&value()?, &flag, 1)?[0]),
            "--crop" => params["crop"] = json!(numbers(&value()?, &flag, 4)?),
            "--gap" => params["gap"] = json!(numbers(&value()?, &flag, 1)?[0]),
            "--min-share" => params["minShare"] = json!(numbers(&value()?, &flag, 1)?[0]),
            "--layout" => {
                params["layout"] = json!(value()?.split(',').map(str::trim).collect::<Vec<_>>())
            }
            "--pitch-range" => params["pitchRange"] = json!(numbers(&value()?, &flag, 2)?),
            "--check-tolerance" => {
                params["checkTolerance"] = json!(numbers(&value()?, &flag, 1)?[0])
            }
            "--flood" => params["flood"] = json!(numbers(&value()?, &flag, 1)?[0]),
            "--parts" => {
                let view = args.get(index + 1).filter(|next| !next.starts_with('-'));
                params["parts"] = match view {
                    Some(view) => {
                        index += 1;
                        json!({"view": view})
                    }
                    None => json!(true),
                };
            }
            "--json" => as_json = true,
            other => return Err(format!("unknown argument `{other}`\n\n{MEASURE_HELP}")),
        }
        index += 1;
    }
    if params.get("path").is_none() {
        return Err(format!("--sheet is required\n\n{MEASURE_HELP}"));
    }
    Ok(Some((params, as_json)))
}

/// The plain report of `froxel measure`.
fn measure_text(reply: &serde_json::Value) -> String {
    let mut out = String::new();
    for (index, view) in reply["views"].as_array().into_iter().flatten().enumerate() {
        out.push_str(&format!(
            "{:<6} crop {}  pitch {} x {} px  cubes {}\n",
            view["view"]
                .as_str()
                .map_or(index.to_string(), str::to_string),
            view["crop"],
            view["pitchX"],
            view["pitchY"],
            view["cubes"]
        ));
    }
    for check in reply["checks"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "{:<4} {}: {}% in pixels, {}% in cubes\n",
            if check["ok"] == true { "ok" } else { "OFF" },
            check["check"].as_str().unwrap_or(""),
            check["percent"],
            check["percentCubes"]
        ));
    }
    if let Some(parts) = reply.get("parts") {
        let dirs: Vec<&str> = parts["parts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p["direction"].as_str())
            .collect();
        out.push_str(&format!(
            "parts of {}: {} ({})\n",
            parts["view"].as_str().unwrap_or(""),
            parts["count"],
            dirs.join(" ")
        ));
    }
    for note in reply["notes"].as_array().into_iter().flatten() {
        out.push_str(&format!("note: {}\n", note.as_str().unwrap_or("")));
    }
    out
}

/// `froxel compare`: MCP `compare_silhouette` on a `.vox`.
pub fn compare(args: &[String]) -> i32 {
    match compare_args(args) {
        Ok(None) => 0,
        Ok(Some((input, params, as_json))) => {
            let mut session = Session::new();
            let done = session.open_vox(&input).and_then(|()| {
                crate::remote::apply(&mut session, "render.compare_silhouette", &params)
            });
            match done {
                Ok(reply) => {
                    if as_json {
                        println!("{reply:#}");
                    } else {
                        let pct = |key: &str| reply[key].as_f64().unwrap_or(0.0) * 100.0;
                        println!(
                            "IoU {:.3}  missing {:.1}%  excess {:.1}%  (align {}, scale {}, offset {})",
                            reply["iou"].as_f64().unwrap_or(0.0),
                            pct("missing"),
                            pct("excess"),
                            reply["align"]["mode"],
                            reply["align"]["scale"],
                            reply["align"]["offset"]
                        );
                    }
                    eprintln!("wrote {}", reply["path"].as_str().unwrap_or(""));
                    0
                }
                Err(err) => {
                    eprintln!("{err}");
                    1
                }
            }
        }
        Err(err) => {
            eprintln!("{err}");
            2
        }
    }
}

const COMPARE_HELP: &str = "\
usage: froxel compare --input <file.vox> --concept <concept.png> --out <overlay.png>
                      [--crop X,Y,W,H] [--background auto|alpha|#rrggbb]
                      [--tolerance N] [--flip] [--align fit|height|width]
                      [--preset game|NAME] [--presets <presets.json>]
                      [--placement-yaw DEG]
                      [--view front|back|left|right|top|bottom]
                      [--yaw DEG] [--pitch DEG] [--ortho] [--margin F]
                      [--object INDEX|NAME] [--isolate]
                      [--width PX] [--height PX] [--json]
                      [--exclude #RRGGBB,...] [--exclude-tolerance N]
                      [--flood N] [--mask <mask.png>] [--invert-mask]
                      [--erase X,Y,W,H]... [--keep-largest]

Compares the model's preview silhouette from a camera with a concept mask,
as MCP compare_silhouette does, and writes an overlay PNG: the preview with
the concept's outline in cyan, what the model misses in magenta, and what
it has in excess in orange. Prints IoU, missing (share of the concept not
covered), and excess (share of the model outside the concept); --json
prints the whole reply. The concept keeps its proportions: --align fit
(default) fits one scale and offset for the most overlap, height and width
match that extent. --crop cuts one view out of a turnaround sheet.
--preset game is the gameplay camera; --presets (with the user's
camera_presets.json or FROXEL_CAMERA_PRESETS) redefines it for a project, and
--placement-yaw checks an asset placed turned that many degrees about up, as
froxel preview does.

A hero view with water, splashes, or a rider needs its background masked
first: --exclude drops pixels near those colours (--exclude-tolerance,
default 40), --flood N floods the background from the crop's border in
colour steps of N (a photo or gradient; 24 is a good start), --mask keeps
only the pixels under a mask PNG's ink (--invert-mask drops them), --erase
drops a rectangle in image pixels (repeat it), and --keep-largest keeps the
largest connected region. The reply's maskEdits counts what each removed.";

pub(crate) fn compare_args(
    args: &[String],
) -> Result<Option<(PathBuf, serde_json::Value, bool)>, String> {
    use serde_json::json;
    let mut input = None;
    let mut params = json!({});
    let mut as_json = false;
    let mut index = 0;
    while index < args.len() {
        let value = |index: &mut usize| -> Result<String, String> {
            *index += 1;
            args.get(*index)
                .cloned()
                .ok_or_else(|| format!("{} needs a value", args[*index - 1]))
        };
        let number = |text: String, flag: &str| -> Result<f64, String> {
            text.parse::<f64>()
                .map_err(|_| format!("{flag} needs a number, not `{text}`"))
        };
        let flag = args[index].clone();
        match flag.as_str() {
            "-h" | "--help" => {
                eprintln!("{COMPARE_HELP}");
                return Ok(None);
            }
            "--input" => input = Some(PathBuf::from(value(&mut index)?)),
            "--concept" => params["concept"] = json!(value(&mut index)?),
            "--out" | "--output" => params["path"] = json!(value(&mut index)?),
            "--crop" => {
                let text = value(&mut index)?;
                let crop: Vec<u64> = text
                    .split(',')
                    .map(|part| part.trim().parse::<u64>())
                    .collect::<Result<_, _>>()
                    .map_err(|_| format!("--crop needs X,Y,W,H, not `{text}`"))?;
                if crop.len() != 4 {
                    return Err(format!("--crop needs X,Y,W,H, not `{text}`"));
                }
                params["crop"] = json!(crop);
            }
            "--background" => params["background"] = json!(value(&mut index)?),
            "--tolerance" => params["tolerance"] = json!(number(value(&mut index)?, &flag)?),
            "--flip" => params["flip"] = json!(true),
            "--align" => params["align"] = json!(value(&mut index)?),
            "--preset" => params["preset"] = json!(value(&mut index)?),
            "--presets" => params["presets"] = json!(value(&mut index)?),
            "--placement-yaw" => params["placementYaw"] = json!(number(value(&mut index)?, &flag)?),
            "--view" => params["view"] = json!(value(&mut index)?),
            "--yaw" => params["yaw"] = json!(number(value(&mut index)?, &flag)?),
            "--pitch" => params["pitch"] = json!(number(value(&mut index)?, &flag)?),
            "--ortho" => params["orthographic"] = json!(true),
            "--margin" => params["margin"] = json!(number(value(&mut index)?, &flag)?),
            "--object" => {
                let text = value(&mut index)?;
                params["object"] = match text.parse::<u64>() {
                    Ok(number) => json!(number),
                    Err(_) => json!(text),
                };
            }
            "--isolate" => params["isolate"] = json!(true),
            "--width" => params["width"] = json!(number(value(&mut index)?, &flag)? as i64),
            "--height" => params["height"] = json!(number(value(&mut index)?, &flag)? as i64),
            "--json" => as_json = true,
            "--exclude" => {
                let text = value(&mut index)?;
                params["excludeColors"] = json!(text
                    .split(',')
                    .map(str::trim)
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>());
            }
            "--exclude-tolerance" => {
                params["excludeTolerance"] = json!(number(value(&mut index)?, &flag)?)
            }
            "--flood" => params["flood"] = json!(number(value(&mut index)?, &flag)?),
            "--mask" => {
                let invert = params["mask"]["invert"].as_bool().unwrap_or(false);
                params["mask"] = json!({"path": value(&mut index)?, "invert": invert});
            }
            "--invert-mask" => {
                if params.get("mask").is_none() {
                    params["mask"] = json!({});
                }
                params["mask"]["invert"] = json!(true);
            }
            "--erase" => {
                let text = value(&mut index)?;
                let rect: Vec<i64> = text
                    .split(',')
                    .map(|part| part.trim().parse::<i64>())
                    .collect::<Result<_, _>>()
                    .map_err(|_| format!("--erase needs X,Y,W,H, not `{text}`"))?;
                if rect.len() != 4 {
                    return Err(format!("--erase needs X,Y,W,H, not `{text}`"));
                }
                if params.get("erase").is_none() {
                    params["erase"] = json!([]);
                }
                params["erase"].as_array_mut().unwrap().push(json!(rect));
            }
            "--keep-largest" => params["keepLargest"] = json!(true),
            other => return Err(format!("unknown argument `{other}`\n\n{COMPARE_HELP}")),
        }
        index += 1;
    }
    let input = input.ok_or_else(|| format!("--input is required\n\n{COMPARE_HELP}"))?;
    for (key, flag) in [("concept", "--concept"), ("path", "--out")] {
        if params.get(key).is_none() {
            return Err(format!("{flag} is required\n\n{COMPARE_HELP}"));
        }
    }
    Ok(Some((input, params, as_json)))
}

const IMPORT_USAGE: &str = "\
usage: froxel import --mesh <file.glb|.gltf|.obj> --output <file.vox>
                     [--input <base.vox>] [--size CELLS | --voxel-size METRES | --scale CELLS_PER_UNIT]
                     [--z-up] [--rotation X,Y,Z] [--position X,Y,Z] [--anchor min|center|bottom]
                     [--fill solid|parity|shell | --shell] [--vertex-colors] [--palette SLOT | --slots A,B,...]
                     [--max-new-slots N] [--tolerance D] [--mode replace|empty|paint]
                     [--new-object] [--name NAME] [--orient upright|<orient.json>]
                     [--clean true|JSON] [--coverage SHARE [--no-keep-thin]] [--min-feature N]

MCP import_mesh from the command line: voxelize the mesh into the first object
of --input (or a new document), or a new object with --new-object, and save
the .vox. --size is cells along the longest axis (default 64); --voxel-size
0.08 keeps a glTF's real size at 0.08 m voxels. The file's Y-up becomes Z-up
unless --z-up. --fill solid (default) fills all a flood from the box
border cannot reach, sealing the openings of an open mesh at their rims;
parity is the older ray-parity vote; shell (or --shell) writes only the
surface. PNG, JPEG, and WebP textures are read. Colours come from the
base-colour texture, factor, and vertex colours (--vertex-colors: vertex
colours only), reuse palette slots within --tolerance (default 12), and add
at most --max-new-slots (default 32) in slots no cell uses; --slots maps to
those slots only, --palette paints one. --orient reads a views sheet's
orient_to_views arguments (views, turns, yawStep, ...; paths relative to the
file) and turns the mesh to the axis-aligned turn and yaw whose silhouettes
match the views best before voxelizing it (with upright: true in it, the
mesh is stood up first and only its yaw searched); --orient upright alone
stands a tilted mesh level from its shape (thinnest principal axis up,
narrowing upward, the taller end of its long axis toward -Y). --clean runs MCP reduce_palette
over the imported cells before they are written: true is
{\"colors\": 8, \"despeckle\": 1}, or pass its options as JSON (colors |
palette, accents, minShare, contrast, maxAccents, despeckle, minSame,
protect). --coverage 0.5 keeps a cell only when half of it is solid
(measured 2 to 4 times finer) instead of every cell the surface touches, so
a small creature's legs and fins do not merge; a part thinner than a cell
then keeps the one cell its midline crosses unless --no-keep-thin.
--min-feature N reports the parts thinner than N cells. Prints the import
report as JSON.";

/// `froxel import`: a mesh voxelized into a `.vox` with the `import_mesh`
/// options.
pub fn import(args: &[String]) -> i32 {
    match import_args(args) {
        Ok(None) => 0,
        Ok(Some((input, output, params))) => {
            let mut session = Session::new();
            let done = match &input {
                Some(input) => session.open_vox(input),
                None => {
                    session.replace_empty();
                    Ok(())
                }
            }
            .and_then(|()| crate::remote::apply(&mut session, "mesh.import", &params))
            .and_then(|reply| {
                if let Some(parent) = output
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                {
                    std::fs::create_dir_all(parent)
                        .map_err(|err| format!("Could not create {}: {err}", parent.display()))?;
                }
                session.write_vox(&output)?;
                Ok(reply)
            });
            match done {
                Ok(reply) => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&reply).unwrap_or_default()
                    );
                    eprintln!("wrote {}", output.display());
                    0
                }
                Err(err) => {
                    eprintln!("{err}");
                    1
                }
            }
        }
        Err(err) => {
            eprintln!("{err}");
            2
        }
    }
}

fn import_args(
    args: &[String],
) -> Result<Option<(Option<PathBuf>, PathBuf, serde_json::Value)>, String> {
    use serde_json::json;
    let mut input = None;
    let mut output = None;
    let mut params = json!({});
    let mut index = 0;
    let numbers = |text: &str, what: &str, count: usize| -> Result<Vec<f64>, String> {
        let values = text
            .split(',')
            .map(|part| part.trim().parse::<f64>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| format!("{what} must be {count} numbers separated by commas"))?;
        if values.len() == count || count == 0 {
            Ok(values)
        } else {
            Err(format!(
                "{what} must be {count} numbers separated by commas"
            ))
        }
    };
    while index < args.len() {
        let flag = args[index].as_str();
        let mut value = || -> Result<&String, String> {
            index += 1;
            args.get(index)
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag {
            "-h" | "--help" => {
                eprintln!("{IMPORT_USAGE}");
                return Ok(None);
            }
            "--mesh" => params["path"] = json!(value()?),
            "--input" => input = Some(PathBuf::from(value()?)),
            "--output" => output = Some(PathBuf::from(value()?)),
            "--size" | "--voxel-size" | "--scale" | "--max-new-slots" | "--tolerance"
            | "--palette" => {
                let text = value()?;
                let number: f64 = text
                    .parse()
                    .map_err(|_| format!("{flag} must be a number, not `{text}`"))?;
                let key = match flag {
                    "--size" => "size",
                    "--voxel-size" => "voxelSize",
                    "--scale" => "scale",
                    "--max-new-slots" => "maxNewSlots",
                    "--tolerance" => "tolerance",
                    _ => "palette",
                };
                params[key] = if number.fract() == 0.0 {
                    json!(number as i64)
                } else {
                    json!(number)
                };
            }
            "--rotation" => params["rotation"] = json!(numbers(value()?, "--rotation", 3)?),
            "--position" => params["position"] = json!(numbers(value()?, "--position", 3)?),
            "--slots" => {
                let slots: Vec<i64> = numbers(value()?, "--slots", 0)?
                    .into_iter()
                    .map(|v| v as i64)
                    .collect();
                params["slots"] = json!(slots);
            }
            "--anchor" => params["anchor"] = json!(value()?),
            "--orient" => {
                let text = value()?;
                params["orient"] = if text == "upright" {
                    serde_json::json!("upright")
                } else {
                    read_views_json(Path::new(text))?
                };
            }
            "--clean" => {
                let text = value()?;
                params["clean"] = serde_json::from_str(&text)
                    .map_err(|err| format!("--clean must be true or JSON: {err}"))?;
            }
            "--coverage" => {
                params["coverage"] = json!(value()?
                    .parse::<f64>()
                    .map_err(|_| "--coverage needs a share such as 0.5".to_string())?)
            }
            "--no-keep-thin" => params["keepThin"] = json!(false),
            "--min-feature" => {
                params["minFeature"] = json!(value()?
                    .parse::<u32>()
                    .map_err(|_| "--min-feature needs a whole number".to_string())?)
            }
            "--mode" => params["mode"] = json!(value()?),
            "--name" => params["name"] = json!(value()?),
            "--z-up" => params["upAxis"] = json!("z"),
            "--shell" => params["fill"] = json!("shell"),
            "--fill" => params["fill"] = json!(value()?),
            "--vertex-colors" => params["colors"] = json!("vertex"),
            "--new-object" => params["newObject"] = json!(true),
            other => return Err(format!("unknown argument `{other}`\n{IMPORT_USAGE}")),
        }
        index += 1;
    }
    match (params.get("path"), output) {
        (Some(_), Some(output)) if output.extension().is_some_and(|ext| ext == "vox") => {
            Ok(Some((input, output, params)))
        }
        (Some(_), Some(_)) => Err("--output must end in .vox".into()),
        _ => Err(IMPORT_USAGE.into()),
    }
}

/// Open `input`, make `object` active, run one MCP method on it, save
/// `output`, and print the reply.
fn edit_vox(
    input: &Path,
    output: &Path,
    object: Option<&str>,
    method: &str,
    params: &serde_json::Value,
) -> i32 {
    let mut session = Session::new();
    let done = session
        .open_vox(input)
        .and_then(|()| {
            if let Some(object) = object {
                session.selected = crate::preview::find_object(&session, object)?;
            }
            crate::remote::apply(&mut session, method, params)
        })
        .and_then(|reply| {
            if let Some(parent) = output
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent)
                    .map_err(|err| format!("Could not create {}: {err}", parent.display()))?;
            }
            session.write_vox(output)?;
            Ok(reply)
        });
    match done {
        Ok(reply) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&reply).unwrap_or_default()
            );
            eprintln!("wrote {}", output.display());
            0
        }
        Err(err) => {
            eprintln!("{err}");
            1
        }
    }
}

/// The flags every `.vox` edit command takes, and its own ones as MCP
/// arguments: `--flag value` becomes `flag` in camel case, a number or a
/// list `a,b,c` of numbers when it parses as one, else a string.
type EditArgs = (PathBuf, PathBuf, Option<String>, serde_json::Value);

fn edit_args(args: &[String], help: &str, flags: &[&str]) -> Result<Option<EditArgs>, String> {
    use serde_json::json;
    let (mut input, mut output, mut object) = (None, None, None);
    let mut params = json!({});
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        if flag == "-h" || flag == "--help" {
            eprintln!("{help}");
            return Ok(None);
        }
        index += 1;
        let value = args
            .get(index)
            .cloned()
            .ok_or_else(|| format!("{flag} needs a value"))?;
        match flag {
            "--input" => input = Some(PathBuf::from(value)),
            "--out" | "--output" => output = Some(PathBuf::from(value)),
            "--object" => object = Some(value),
            _ if flags.contains(&flag) => {
                let mut key = String::new();
                let mut upper = false;
                for c in flag.trim_start_matches("--").chars() {
                    if c == '-' {
                        upper = true;
                    } else if upper {
                        key.extend(c.to_uppercase());
                        upper = false;
                    } else {
                        key.push(c);
                    }
                }
                let number = |text: &str| -> Option<serde_json::Value> {
                    let n: f64 = text.trim().parse().ok()?;
                    Some(if n.fract() == 0.0 && n.abs() < 1e15 {
                        json!(n as i64)
                    } else {
                        json!(n)
                    })
                };
                params[key] = if value.contains(',') {
                    value
                        .split(',')
                        .map(number)
                        .collect::<Option<Vec<_>>>()
                        .map(serde_json::Value::from)
                        .ok_or_else(|| format!("{flag} must be numbers separated by commas"))?
                } else {
                    number(&value).unwrap_or(json!(value))
                };
            }
            other => return Err(format!("unknown argument `{other}`\n\n{help}")),
        }
        index += 1;
    }
    let input = input.ok_or_else(|| format!("--input is required\n\n{help}"))?;
    let output = output.ok_or_else(|| format!("--out is required\n\n{help}"))?;
    Ok(Some((input, output, object, params)))
}

const COMPONENTS_HELP: &str = "\
usage: froxel components --input <file.vox> --out <file.vox> [--object <index|name>]
                         [--op list|keep_largest|remove_small|delete|move|split]
                         [--connectivity 6|18|26] [--keep N] [--min-cells N]
                         [--seed X,Y,Z] [--anchor X,Y,Z] [--reach R] [--radius R]
                         [--offset DX,DY,DZ] [--name NAME] [--into OBJECT]

MCP components on the active object (or --object) of a .vox, saved to
--out, with the reply printed as JSON: list the connected parts, keep the
largest, remove the parts under --min-cells (floaters), or delete, move,
or split into a new object the part holding --seed, first cut at its
thinnest neck from --anchor (a cell of the part that stays). Seeds are the
object's local cells.";

/// `froxel components`: MCP `components` on a `.vox`.
pub fn components(args: &[String]) -> i32 {
    let flags = [
        "--op",
        "--connectivity",
        "--keep",
        "--min-cells",
        "--seed",
        "--anchor",
        "--reach",
        "--radius",
        "--offset",
        "--name",
        "--into",
    ];
    match edit_args(args, COMPONENTS_HELP, &flags) {
        Ok(None) => 0,
        Ok(Some((input, output, object, params))) => edit_vox(
            &input,
            &output,
            object.as_deref(),
            "voxel.components",
            &params,
        ),
        Err(err) => {
            eprintln!("{err}");
            2
        }
    }
}

const ORIENT_HELP: &str = "\
usage: froxel orient --args <orient.json> --input <file.vox> --out <file.vox> [--object <index|name>]

MCP orient_to_views on the active object (or --object) of a .vox: the
24 axis-aligned turns (turns upright: the 4 about up), then fine yaw steps,
are scored by the silhouette IoU of the object against each view of a
views sheet, and the object is turned to the best and scaled, then saved;
the reply (IoU per view before and after, rotation, candidates, margin) is
printed as JSON. orient.json holds that tool's arguments: {views: [{view,
path, crop, background, tolerance, flip, weight}, ...], path, background,
tolerance, turns, yawStep, yawRange, fineAxes, resolution, size | height |
scale,
apply}. A relative path is relative to orient.json. froxel import --orient
runs the same search on a mesh before it is voxelized.";

/// `froxel orient`: MCP `orient_to_views` on a `.vox`.
pub fn orient(args: &[String]) -> i32 {
    let parsed = edit_args(args, ORIENT_HELP, &["--args"]).and_then(|found| {
        let Some((input, output, object, flags)) = found else {
            return Ok(None);
        };
        let file = flags["args"]
            .as_str()
            .map(PathBuf::from)
            .ok_or_else(|| format!("--args is required\n\n{ORIENT_HELP}"))?;
        Ok(Some((input, output, object, read_views_json(&file)?)))
    });
    match parsed {
        Ok(None) => 0,
        Ok(Some((input, output, object, params))) => edit_vox(
            &input,
            &output,
            object.as_deref(),
            "voxel.orient_to_views",
            &params,
        ),
        Err(err) => {
            eprintln!("{err}");
            2
        }
    }
}

/// A JSON file of views arguments, its image paths resolved against its
/// folder.
fn read_views_json(file: &Path) -> Result<serde_json::Value, String> {
    let text = std::fs::read_to_string(file)
        .map_err(|err| format!("Could not read {}: {err}", file.display()))?;
    let mut params: serde_json::Value = serde_json::from_str(&text)
        .map_err(|err| format!("{} is not JSON: {err}", file.display()))?;
    views_relative(&mut params, file.parent().unwrap_or(Path::new("")));
    Ok(params)
}

const DETERRACE_HELP: &str = "\
usage: froxel deterrace --input <file.vox> --out <file.vox> [--object <index|name>]
                        [--mode blocks|stagger] [--size 2..4] [--fill F] [--origin X,Y,Z]
                        [--tolerance CELLS] [--run CELLS] [--jitter F] [--seed N]

MCP deterrace on the active object (or --object) of a .vox, saved to
--out, with the reply printed as JSON: --mode blocks (default) re-quantizes
the object into cubes of --size cells (a cube is solid when --fill of its
cells are, default 0.5; cubes that would move the surface more than
--tolerance cells stay); --mode stagger moves the edges of the one-cell
terrace steps on the top surface in teeth --run cells long, with --jitter
of them left in place (seeded by --seed). The reply's steps and longestRun
count one-cell steps and the longest straight run of them, before and
after.";

/// `froxel deterrace`: MCP `deterrace` on a `.vox`.
pub fn deterrace(args: &[String]) -> i32 {
    let flags = [
        "--mode",
        "--size",
        "--fill",
        "--origin",
        "--tolerance",
        "--run",
        "--jitter",
        "--seed",
    ];
    match edit_args(args, DETERRACE_HELP, &flags) {
        Ok(None) => 0,
        Ok(Some((input, output, object, params))) => edit_vox(
            &input,
            &output,
            object.as_deref(),
            "voxel.deterrace",
            &params,
        ),
        Err(err) => {
            eprintln!("{err}");
            2
        }
    }
}

const REDUCE_HELP: &str = "\
usage: froxel reduce-palette --input <file.vox> --out <file.vox> [--object <index|name>]
                             [--colors N | --palette A,B,#rrggbb,...] [--accents 0|1]
                             [--min-share F] [--contrast F] [--max-accents N]
                             [--despeckle PASSES] [--min-same N] [--protect A,B,...]

MCP reduce_palette on the active object (or --object) of a .vox, saved to
--out, with the reply printed as JSON: --colors clusters the object's
colours into N (k-means in Oklab over the surface) and keeps up to
--max-accents small distinct colours (eyes, a mouth) their own slot when
they hold --min-share of the surface and lie --contrast (Oklab) from every
cluster; --palette snaps them to slots or #rrggbb colours instead. Then
--despeckle passes of a majority filter recolour surface cells whose slot
fewer than --min-same of their surface neighbours share. --protect lists
slots both passes leave alone.";

/// `froxel reduce-palette`: MCP `reduce_palette` on a `.vox`.
pub fn reduce_palette(args: &[String]) -> i32 {
    // --palette may hold #rrggbb entries, which edit_args does not parse.
    let mut rest = Vec::new();
    let mut palette = None;
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--palette" && index + 1 < args.len() {
            palette = Some(args[index + 1].clone());
            index += 2;
        } else {
            rest.push(args[index].clone());
            index += 1;
        }
    }
    let flags = [
        "--colors",
        "--accents",
        "--min-share",
        "--contrast",
        "--max-accents",
        "--despeckle",
        "--min-same",
        "--protect",
    ];
    match edit_args(&rest, REDUCE_HELP, &flags) {
        Ok(None) => 0,
        Ok(Some((input, output, object, mut params))) => {
            if let Some(list) = palette {
                params["palette"] = list
                    .split(',')
                    .map(|item| {
                        let item = item.trim();
                        item.parse::<u64>()
                            .map(serde_json::Value::from)
                            .unwrap_or_else(|_| serde_json::Value::from(item))
                    })
                    .collect();
            }
            if let Some(n) = params.get("accents").and_then(|v| v.as_i64()) {
                params["accents"] = serde_json::json!(n != 0);
            }
            if let Some(n) = params.get("protect").and_then(|v| v.as_i64()) {
                params["protect"] = serde_json::json!([n]);
            }
            edit_vox(
                &input,
                &output,
                object.as_deref(),
                "voxel.reduce_palette",
                &params,
            )
        }
        Err(err) => {
            eprintln!("{err}");
            2
        }
    }
}

const DEFORM_HELP: &str = "\
usage: froxel deform --args <deform.json> --input <file.vox> --out <file.vox>
                     [--object <index|name>]

MCP deform on the active object (or --object) of a .vox, saved to --out,
with the reply printed as JSON. deform.json holds its arguments: {op: bend
| taper | twist | displace | scale, x0..z1, axis, origin, length, center,
angle, direction, start, end, amplitude, scale, octaves, seed, bias, point,
factor, radius, falloff, minFeature}. op scale enlarges a part (a head, an
eye, a fin) about point by factor, the cells within radius as one piece,
easing back to none over falloff cells, so a small creature's features get
the cells they need; minFeature N reports the parts thinner than N cells
after the deform (thin in the reply).";

/// `froxel deform`: MCP `deform` on a `.vox`.
pub fn deform(args: &[String]) -> i32 {
    let parsed = edit_args(args, DEFORM_HELP, &["--args"]).and_then(|found| {
        let Some((input, output, object, flags)) = found else {
            return Ok(None);
        };
        let file = flags["args"]
            .as_str()
            .map(PathBuf::from)
            .ok_or_else(|| format!("--args is required\n\n{DEFORM_HELP}"))?;
        let text = std::fs::read_to_string(&file)
            .map_err(|err| format!("Could not read {}: {err}", file.display()))?;
        let params: serde_json::Value = serde_json::from_str(&text)
            .map_err(|err| format!("{} is not JSON: {err}", file.display()))?;
        Ok(Some((input, output, object, params)))
    });
    match parsed {
        Ok(None) => 0,
        Ok(Some((input, output, object, params))) => {
            edit_vox(&input, &output, object.as_deref(), "voxel.deform", &params)
        }
        Err(err) => {
            eprintln!("{err}");
            2
        }
    }
}

const SPLIT_PARTS_HELP: &str = "\
usage: froxel split-parts --args <parts.json> --input <file.vox> --out <file.vox>
                          [--object <index|name>]

MCP split_parts on the active object (or --object) of a .vox, saved to
--out, with the reply printed as JSON. parts.json holds its arguments:
{parts: [{name, point: [x, y, z] | points: [[x, y, z], ...] | bone: NAME |
marker: NAME, planes: [{normal, at}]}], metric (geodesic | euclidean),
connectivity (6 | 26), reach, removeSource, bind, space}. Every cell goes
to the part whose seed is nearest through the solid, and each part becomes
its own object at world [0, 0, 0].";

/// `froxel split-parts`: MCP `split_parts` on a `.vox`.
pub fn split_parts(args: &[String]) -> i32 {
    let parsed = edit_args(args, SPLIT_PARTS_HELP, &["--args"]).and_then(|found| {
        let Some((input, output, object, flags)) = found else {
            return Ok(None);
        };
        let file = flags["args"]
            .as_str()
            .map(PathBuf::from)
            .ok_or_else(|| format!("--args is required\n\n{SPLIT_PARTS_HELP}"))?;
        let text = std::fs::read_to_string(&file)
            .map_err(|err| format!("Could not read {}: {err}", file.display()))?;
        let params: serde_json::Value = serde_json::from_str(&text)
            .map_err(|err| format!("{} is not JSON: {err}", file.display()))?;
        Ok(Some((input, output, object, params)))
    });
    match parsed {
        Ok(None) => 0,
        Ok(Some((input, output, object, params))) => edit_vox(
            &input,
            &output,
            object.as_deref(),
            "object.split_parts",
            &params,
        ),
        Err(err) => {
            eprintln!("{err}");
            2
        }
    }
}
