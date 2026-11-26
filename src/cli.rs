//! `froxel render`, `froxel preview`, `froxel export`, `froxel import`,
//! `froxel carve`, `froxel paint`, `froxel fit`, `froxel compare`, and
//! `froxel voxrt`.

use std::path::{Path, PathBuf};

use crate::state::{rotation_codec_errors, Session};

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

struct VoxJob {
    input: PathBuf,
    dump: bool,
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
    let mut tag = [0u8; 8];
    let _ = getrandom::fill(&mut tag);
    let tag = u64::from_le_bytes(tag);
    let kept = std::env::temp_dir().join(format!("froxel-voxrt-{tag:016x}.vox"));
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

/// `--some-flag value` as the MCP argument `someFlag`: a number, a list
/// `a,b,c` of numbers, `true` or `false`, JSON (an object or a list), or
/// else the string.
fn flag_argument(flag: &str, value: &str) -> Result<(String, serde_json::Value), String> {
    use serde_json::json;
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
    let trimmed = value.trim();
    let parsed = if trimmed.starts_with('{') || trimmed.starts_with('[') {
        serde_json::from_str(trimmed).map_err(|err| format!("{flag} is not JSON: {err}"))?
    } else if trimmed == "true" || trimmed == "false" {
        json!(trimmed == "true")
    } else if value.contains(',') {
        value
            .split(',')
            .map(number)
            .collect::<Option<Vec<_>>>()
            .map(serde_json::Value::from)
            .ok_or_else(|| format!("{flag} must be numbers separated by commas"))?
    } else {
        number(value).unwrap_or(json!(value))
    };
    Ok((key, parsed))
}

const RUN_HELP: &str = "\
usage: froxel run <tool> [--input <file.vox>] [--out <file.vox>] [--object <index|name>]
                  [--args <args.json>] [--json '<arguments>'] [--<argument> <value> ...]
       froxel run --list
       froxel run <tool> --help

Any MCP tool on a .vox with no window: open --input (or start empty), make
--object active, run <tool> with its arguments, print the reply as JSON,
and save to --out when given. Arguments come from --args (a JSON file),
--json (a JSON object), and --<argument> <value> flags, later ones winning:
--min-feature 2 is minFeature 2, a,b,c is a list of numbers, true and false
are booleans, and {...} or [...] is JSON. Every argument is checked
against the tool's schema, as MCP checks it. The window's tools
(screenshots, ui_inspect, ui_click) and reload are not run here.";

/// The old subcommands that `froxel run` replaced, and the tool each is.
pub fn replaced_by(command: &str) -> Option<&'static str> {
    Some(match command {
        "render" => "render",
        "preview" => "preview",
        "export" => "export_mesh",
        "bake-scene" => "bake_scene",
        "carve" => "carve_from_views",
        "paint" => "project_paint (or decal, shell_plates, paint_from_views)",
        "fit" => "fit_to_views",
        "optimize" => "optimize_to_views",
        "compare" => "compare_silhouette",
        "measure" => "measure_sheet",
        "pixelate" => "pixelate_views",
        "import" => "import_mesh",
        "orient" => "orient_to_views",
        "deterrace" => "deterrace",
        "deform" => "deform",
        "blend" => "blend_shapes",
        "relief" => "radial_relief",
        "split-parts" => "split_parts",
        "reduce-palette" => "reduce_palette",
        "components" => "components",
        "variants" => "export_variants",
        _ => return None,
    })
}

/// The argument keys that name a file or a folder.
const PATH_KEYS: &[&str] = &[
    "path",
    "glb",
    "scene",
    "presets",
    "lightingPresets",
    "png",
    "concept",
    "dir",
    "manifest",
    "variants",
    "depth",
    "sheet",
    "mesh",
    "inputs",
];

/// Resolve the relative file names of an `--args` file against its folder:
/// every string (or list of strings) under a key of [`PATH_KEYS`] or one
/// ending in `Path`, at any depth (each view's `path` and `depth`, a
/// reference's `path`).
fn args_relative(value: &mut serde_json::Value, base: &Path) {
    use serde_json::Value;
    let fix = |text: &mut String| {
        if !text.is_empty() && Path::new(text.as_str()).is_relative() {
            *text = base.join(text.as_str()).display().to_string();
        }
    };
    match value {
        Value::Object(map) => {
            for (key, item) in map.iter_mut() {
                let named = PATH_KEYS.contains(&key.as_str()) || key.ends_with("Path");
                match item {
                    Value::String(text) if named => fix(text),
                    Value::Array(list) if named && list.iter().all(Value::is_string) => {
                        for entry in list {
                            if let Value::String(text) = entry {
                                fix(text);
                            }
                        }
                    }
                    _ => args_relative(item, base),
                }
            }
        }
        Value::Array(list) => {
            for item in list {
                args_relative(item, base);
            }
        }
        _ => {}
    }
}

/// `froxel run`: any registry tool on a `.vox`, headless.
pub fn run_tool(args: &[String]) -> i32 {
    use serde_json::{json, Value};
    let Some(name) = args.first().map(String::as_str) else {
        eprintln!("{RUN_HELP}");
        return 2;
    };
    if name == "-h" || name == "--help" {
        eprintln!("{RUN_HELP}");
        return 0;
    }
    if name == "--list" {
        for op in crate::ops::OPS {
            let description = crate::ops::schema::tool(op.tool)
                .and_then(|tool| tool["description"].as_str())
                .unwrap_or("");
            let first = description.split(". ").next().unwrap_or(description);
            println!("{:<22} {}", op.tool, first.trim_end_matches('.'));
        }
        return 0;
    }
    let Some(op) = crate::ops::by_tool(name) else {
        eprintln!("`{name}` is not a tool; froxel run --list names them");
        return 2;
    };
    if op.method.starts_with("ui.") || op.method == "server.reload" {
        eprintln!("`{name}` needs a running server or window; use froxel mcp");
        return 2;
    }
    let tool = crate::ops::schema::tool(op.tool).expect("every op has a schema");
    if args[1..].iter().any(|arg| arg == "-h" || arg == "--help") {
        println!(
            "{}\n\n{}\n",
            op.tool,
            tool["description"].as_str().unwrap_or("")
        );
        let required: Vec<&str> = tool["inputSchema"]["required"]
            .as_array()
            .map(|list| list.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if let Some(properties) = tool["inputSchema"]["properties"].as_object() {
            for (key, property) in properties {
                let kind = match &property["type"] {
                    Value::String(kind) => kind.clone(),
                    Value::Array(kinds) => kinds
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" | "),
                    _ => "any".into(),
                };
                let need = if required.contains(&key.as_str()) {
                    " (required)"
                } else {
                    ""
                };
                println!("  {key} [{kind}]{need}");
                // The panel's label and hint are the short help.
                if let (Some(label), Some(hint)) = (
                    property["title"].as_str(),
                    property[crate::ops::schema::UI]["hint"].as_str(),
                ) {
                    println!("      {label}: {hint}");
                }
                println!("      {}", property["description"].as_str().unwrap_or(""));
            }
        }
        return 0;
    }
    let parsed =
        (|| -> Result<(Option<PathBuf>, Option<PathBuf>, Option<String>, Value), String> {
            let (mut input, mut output, mut object) = (None, None, None);
            let mut params = json!({});
            let mut index = 1;
            while index < args.len() {
                let flag = args[index].as_str();
                index += 1;
                let value = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| format!("{flag} needs a value"))?;
                index += 1;
                match flag {
                    "--input" => input = Some(PathBuf::from(value)),
                    "--out" | "--output" => output = Some(PathBuf::from(value)),
                    "--object" => object = Some(value),
                    "--args" | "--json" => {
                        let text = if flag == "--args" {
                            std::fs::read_to_string(&value)
                                .map_err(|err| format!("Could not read {value}: {err}"))?
                        } else {
                            value.clone()
                        };
                        let mut found: Value = serde_json::from_str(&text)
                            .map_err(|err| format!("{flag} is not JSON: {err}"))?;
                        if flag == "--args" {
                            // A file's paths are relative to the file, so it
                            // can sit beside its images and run from anywhere.
                            let base = Path::new(&value).parent().unwrap_or(Path::new(""));
                            args_relative(&mut found, base);
                        }
                        let Value::Object(found) = found else {
                            return Err(format!("{flag} must be a JSON object"));
                        };
                        for (key, value) in found {
                            params[key] = value;
                        }
                    }
                    _ if flag.starts_with("--") => {
                        let (key, value) = flag_argument(flag, &value)?;
                        params[key] = value;
                    }
                    other => return Err(format!("unexpected argument `{other}`\n\n{RUN_HELP}")),
                }
            }
            Ok((input, output, object, params))
        })();
    let (input, output, object, params) = match parsed {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("{err}");
            return 2;
        }
    };
    let mut session = Session::new();
    let done = match &input {
        Some(input) => session.open_vox(input),
        None => {
            session.replace_empty();
            Ok(())
        }
    }
    .and_then(|()| {
        if let Some(object) = &object {
            session.selected = crate::preview::find_object(&session, object)?;
        }
        crate::remote::apply(&mut session, op.method, &params)
    })
    .and_then(|reply| {
        if let Some(output) = &output {
            if let Some(parent) = output
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent)
                    .map_err(|err| format!("Could not create {}: {err}", parent.display()))?;
            }
            session.write_vox(output)?;
        }
        Ok(reply)
    });
    match done {
        Ok(reply) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&reply).unwrap_or_default()
            );
            if let Some(output) = &output {
                eprintln!("wrote {}", output.display());
            }
            0
        }
        Err(err) => {
            eprintln!("{err}");
            1
        }
    }
}

#[cfg(test)]
#[path = "../tests/cli.rs"]
mod tests;
