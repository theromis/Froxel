//! `froxel render`, `froxel preview`, `froxel export`, `froxel import`,
//! `froxel carve`, `froxel compare`, and `froxel voxrt`.

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
                   \"lean\": {\"degrees\": 12, \"toward\": 180}}],
   \"occluders\": [{\"mesh\": \"ground.glb\", \"translation\": [0, 0, 0]},
                 {\"plane\": 0},
                 {\"height\": {\"origin\": [-4, -4], \"cell\": 1,
                             \"rows\": [[0, 0, 0], [0, 1, 0], [0, 0, 0]]}}],
   \"bake\": {\"preset\": \"old_pond\", \"distance\": 2}}

A placement is the frame of the glb froxel export writes, as a game places it:
world = translation + Lean x scale x Ry(yaw) x (x, z, -y) of a cell corner, Y
up, with yaw in degrees about +Y (0 faces -Z). lean {degrees, toward}, on an
instance or a mesh occluder, tilts it by degrees (-90..90) about its base so its
top moves toward the yaw toward (0 is -Z, 90 is -X); without it, upright. Occluders stop light and are not written: mesh is a
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

Fills the visual hull of 2 or 3 orthographic views (front, side, top) into
the active object, as MCP carve_from_views does, and saves the .vox.
views.json holds that tool's arguments: {views: [{view, path, crop,
cellsPerPixel | size, background, tolerance, flip}, ...], size, origin,
cellsPerPixel, palette | slots, coverage, shell, smooth, mode}. A relative
view path is relative to views.json. --input carves into a copy of that
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
    let base = views.parent().unwrap_or(Path::new(""));
    if let Some(list) = params.get_mut("views").and_then(|v| v.as_array_mut()) {
        for view in list {
            if let Some(path) = view.get("path").and_then(|p| p.as_str()) {
                if Path::new(path).is_relative() {
                    view["path"] = serde_json::json!(base.join(path).display().to_string());
                }
            }
        }
    }
    Ok(Some((input, output, params)))
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
froxel preview does.";

fn compare_args(args: &[String]) -> Result<Option<(PathBuf, serde_json::Value, bool)>, String> {
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
                     [--shell] [--vertex-colors] [--palette SLOT | --slots A,B,...]
                     [--max-new-slots N] [--tolerance D] [--mode replace|empty|paint]
                     [--new-object] [--name NAME]

MCP import_mesh from the command line: voxelize the mesh into the first object
of --input (or a new document), or a new object with --new-object, and save
the .vox. --size is cells along the longest axis (default 64); --voxel-size
0.08 keeps a glTF's real size at 0.08 m voxels. The file's Y-up becomes Z-up
unless --z-up. --shell writes only the surface. Colours come from the
base-colour texture, factor, and vertex colours (--vertex-colors: vertex
colours only), reuse palette slots within --tolerance (default 12), and add
at most --max-new-slots (default 32) in slots no cell uses; --slots maps to
those slots only, --palette paints one. Prints the import report as JSON.";

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
            "--mode" => params["mode"] = json!(value()?),
            "--name" => params["name"] = json!(value()?),
            "--z-up" => params["upAxis"] = json!("z"),
            "--shell" => params["fill"] = json!("shell"),
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
