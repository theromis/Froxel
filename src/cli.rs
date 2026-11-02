//! `froxel render`, `froxel preview`, `froxel export`, and `froxel voxrt`.

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

/// `froxel preview`: the fast lit raster of MCP `preview`.
pub fn preview(args: &[String]) -> i32 {
    match preview_args(args) {
        Ok(None) => 0,
        Ok(Some((input, output, object, preview))) => {
            let started = std::time::Instant::now();
            let mut session = Session::new();
            let written = session.open_vox(&input).and_then(|()| {
                let mut preview = preview;
                if let Some(key) = &object {
                    preview.object = Some(crate::preview::find_object(&session, key)?);
                }
                crate::preview::write(&session, &preview, &output)
            });
            match written {
                Ok(frame) => {
                    eprintln!(
                        "wrote {} ({} triangles, {} ms)",
                        output.display(),
                        frame.triangles,
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

const PREVIEW_USAGE: &str = "\
usage: froxel preview --input <file.vox> --out <file.png>
                      [--width N] [--height N] [--yaw DEG] [--pitch DEG]
                      [--view front|back|left|right|top|bottom]
                      [--ortho] [--fov DEG] [--margin F]
                      [--object NAME|INDEX] [--isolate]
                      [--background #rrggbb[aa]] [--no-antialias]

The editor viewport's mesh and light (sun plus ambient), rasterised on the
CPU: no path tracer, no window, no GPU. The camera orbits at yaw and pitch
and fits the model, or --object, inside the image with --margin (fraction
of the image per side, default 0.06). --isolate draws only --object.
Defaults: 1024x1024 (1..4096), yaw 30, pitch 25, perspective with the
viewport's 50 degree vertical --fov (10..120), the viewport grey
background, and 2x2 antialiasing up to 2048x2048. --output is an alias of
--out.";

#[allow(clippy::type_complexity)]
fn preview_args(
    args: &[String],
) -> Result<Option<(PathBuf, PathBuf, Option<String>, crate::preview::Preview)>, String> {
    let mut input = None;
    let mut output = None;
    let mut object = None;
    let mut preview = crate::preview::Preview::default();
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
            "--input" => input = Some(PathBuf::from(value(&mut index)?)),
            "--out" | "--output" => output = Some(PathBuf::from(value(&mut index)?)),
            "--width" => preview.width = parse_u32(&value(&mut index)?, arg, 1, max)?,
            "--height" => preview.height = parse_u32(&value(&mut index)?, arg, 1, max)?,
            "--yaw" => {
                preview.yaw = parse_f32(&value(&mut index)?, arg)?;
                angles = true;
            }
            "--pitch" => {
                preview.pitch = parse_f32(&value(&mut index)?, arg)?;
                angles = true;
            }
            "--view" => {
                (preview.yaw, preview.pitch) = crate::remote::named_view(&value(&mut index)?)?;
                view = true;
            }
            "--ortho" | "--orthographic" => preview.orthographic = true,
            "--fov" => preview.fov = parse_f32(&value(&mut index)?, arg)?,
            "--margin" => preview.margin = parse_f32(&value(&mut index)?, arg)?,
            "--object" => object = Some(value(&mut index)?),
            "--isolate" => preview.isolate = true,
            "--background" => {
                preview.background = crate::remote::background(
                    &serde_json::json!({"background": value(&mut index)?}),
                )?;
            }
            "--no-antialias" => preview.antialias = false,
            other => return Err(format!("unknown argument `{other}`\n{PREVIEW_USAGE}")),
        }
        index += 1;
    }
    if angles && view {
        return Err("pass --view or --yaw and --pitch, not both".into());
    }
    if preview.isolate && object.is_none() {
        return Err("--isolate needs --object".into());
    }
    let (Some(input), Some(output)) = (input, output) else {
        return Err(PREVIEW_USAGE.into());
    };
    if !output.extension().is_some_and(|ext| ext == "png") {
        return Err("--out must end in .png".into());
    }
    crate::preview::check(&crate::preview::Preview {
        object: object.as_ref().map(|_| 0),
        ..preview.clone()
    })?;
    Ok(Some((input, output, object, preview)))
}

const EXPORT_USAGE: &str = "\
usage: froxel export --input <file.vox> --output <file.glb>
                     [--merge] [--vertex-colors] [--color-atlas]
                     [--export-hidden] [--split-objects]
                     [--ao STRENGTH] [--ao-radius CELLS]
                     [--ao-target vertex|texture|both]

The same options as the MCP tool export_mesh, all off by default.
--ao bakes ambient occlusion: 0..1, the light a fully occluded corner loses.
--ao-radius is its hemisphere reach in cells (0..8, default 3; 0 or 1 is the
corner term alone). --ao-target vertex writes it into COLOR_0 per corner
(faces that shade differently stay unmerged); texture darkens each cell's
colour atlas texel and merges as without occlusion (needs --color-atlas);
both writes the two. The default is texture with --color-atlas, else vertex.";

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
