//! Variant export behind MCP `export_variants`, `froxel variants`, and the
//! Export panel's Export Variants: one scripted document, a list of
//! parameter sets, and for each set a `.vox` and a `.glb` written with the
//! `export_models` options, plus a manifest of the parameters, markers, and
//! bounds. Each set is baked in [`Session::scratch_run`], so the document
//! and its undo history are as they were afterwards.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::state::Session;

/// Most parameter sets in one call.
pub const MAX_VARIANTS: usize = 64;

/// The manifest's file name when the caller gives none.
pub const MANIFEST: &str = "variants.json";

/// Export every variant in `params` and write the manifest. The reply is
/// the manifest with its path.
pub fn export_variants(session: &mut Session, params: &Value) -> Result<Value, String> {
    let dir = PathBuf::from(
        params
            .get("dir")
            .and_then(Value::as_str)
            .ok_or("dir is required: the folder the files go to")?,
    );
    let list = params
        .get("variants")
        .and_then(Value::as_array)
        .ok_or("variants must be a list of {name?, params}")?;
    if list.is_empty() || list.len() > MAX_VARIANTS {
        return Err(format!(
            "variants must hold 1 to {MAX_VARIANTS} parameter sets"
        ));
    }
    if !session
        .objects
        .iter()
        .any(|object| !object.script.source.is_empty())
    {
        return Err("the document has no script; set_script first".into());
    }
    let options = crate::remote::mesh_options(params)?;
    let write_vox = params.get("vox").and_then(Value::as_bool).unwrap_or(true);
    let timeout = match params.get("timeoutSeconds") {
        None | Some(Value::Null) => crate::script::SCRIPT_TIMEOUT,
        Some(found) => found
            .as_f64()
            .filter(|s| *s > 0.0 && *s <= 3600.0)
            .map(std::time::Duration::from_secs_f64)
            .ok_or("timeoutSeconds must be 0..=3600")?,
    };
    // Check every set before writing a file.
    let mut names = HashSet::new();
    let mut sets: Vec<(String, Map<String, Value>)> = Vec::with_capacity(list.len());
    for (index, item) in list.iter().enumerate() {
        let name = match item.get("name") {
            None | Some(Value::Null) => format!("variant_{}", index + 1),
            Some(Value::String(name)) => check_name(name)?,
            Some(_) => return Err(format!("variant {index} name must be a string")),
        };
        if !names.insert(name.to_lowercase()) {
            return Err(format!("variant name `{name}` is used twice"));
        }
        let values = match item.get("params") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(values)) => values.clone(),
            Some(_) => return Err(format!("variant `{name}` params must be an object")),
        };
        sets.push((name, values));
    }
    std::fs::create_dir_all(&dir)
        .map_err(|err| format!("Could not create {}: {err}", dir.display()))?;
    let mut variants = Vec::with_capacity(sets.len());
    for (name, values) in &sets {
        let entry = session.scratch_run(|session| {
            crate::bridge::apply_values(session, values)?;
            for index in 0..session.objects.len() {
                if !session.objects[index].script.source.is_empty() {
                    session.regenerate_script_within(index, Some(timeout))?;
                }
            }
            bake(session, &dir, name, values, &options, write_vox)
        });
        variants.push(entry.map_err(|err| format!("variant `{name}`: {err}"))?);
    }
    let manifest_path = match params.get("manifest").and_then(Value::as_str) {
        Some(path) => PathBuf::from(path),
        None => dir.join(MANIFEST),
    };
    let manifest = json!({
        "generator": "Froxel",
        "options": crate::remote::mesh_params(&options),
        "variants": variants,
    });
    let text = serde_json::to_string_pretty(&manifest).map_err(|err| err.to_string())?;
    std::fs::write(&manifest_path, text + "\n")
        .map_err(|err| format!("Could not write {}: {err}", manifest_path.display()))?;
    let mut reply = manifest;
    reply["manifest"] = json!(manifest_path.display().to_string());
    Ok(reply)
}

/// Write one baked set and describe it for the manifest.
fn bake(
    session: &Session,
    dir: &Path,
    name: &str,
    values: &Map<String, Value>,
    options: &crate::export::MeshExport,
    write_vox: bool,
) -> Result<Value, String> {
    let glb = dir.join(format!("{name}.glb"));
    let quads = crate::export::export_with(session, &glb, options)?;
    let mut entry = json!({
        "name": name,
        "glb": glb.display().to_string(),
        "quads": quads,
        "requested": values,
        "params": resolved_params(session),
        "markers": session.markers_json(),
        "voxels": session.voxel_count(),
    });
    if write_vox {
        let vox = dir.join(format!("{name}.vox"));
        let bytes = session.export_bytes()?;
        std::fs::write(&vox, bytes)
            .map_err(|err| format!("Could not write {}: {err}", vox.display()))?;
        entry["vox"] = json!(vox.display().to_string());
    }
    if let Some((min, max)) = session.world_bounds() {
        // Mesh units: the cells span min..max + 1.
        let lo = [min.0, min.1, min.2];
        let hi = [max.0 + 1, max.1 + 1, max.2 + 1];
        entry["bounds"] = json!({
            "voxel": {"min": lo, "max": hi},
            "gltf": {"min": [lo[0], lo[2], -hi[1]], "max": [hi[0], hi[2], -lo[1]]},
        });
    }
    Ok(entry)
}

/// Every script parameter after the set was applied, keyed `object/name`.
fn resolved_params(session: &Session) -> Value {
    let mut out = Map::new();
    for object in &session.objects {
        for param in &object.script.params {
            let described = crate::script::param_json(param);
            out.insert(
                format!("{}/{}", object.name, param.name),
                described["value"].clone(),
            );
        }
    }
    Value::Object(out)
}

/// A variant name that is a safe file stem.
fn check_name(name: &str) -> Result<String, String> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if !ok {
        return Err(format!(
            "variant name `{name}` must be 1 to 64 letters, digits, '_', '-', or '.', not starting with '.'"
        ));
    }
    Ok(name.to_string())
}

/// `froxel variants`: the same export with no window.
pub fn cli(args: &[String]) -> i32 {
    match cli_args(args) {
        Ok(None) => 0,
        Ok(Some((input, params))) => {
            let mut session = Session::new();
            let done = session
                .open_vox(&input)
                .and_then(|()| export_variants(&mut session, &params));
            match done {
                Ok(reply) => {
                    for variant in reply["variants"].as_array().into_iter().flatten() {
                        eprintln!(
                            "wrote {} ({} quads)",
                            variant["glb"].as_str().unwrap_or(""),
                            variant["quads"]
                        );
                    }
                    eprintln!("wrote {}", reply["manifest"].as_str().unwrap_or(""));
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

pub const USAGE: &str = "\
usage: froxel variants --input <file.vox> --variants <sets.json> --out <dir>
                       [--manifest <file.json>] [--no-vox]
                       [--merge] [--vertex-colors] [--color-atlas]
                       [--export-hidden] [--split-objects]
                       [--ao STRENGTH] [--ao-radius CELLS]
                       [--ao-target vertex|texture|both]

Bakes the scripted document once per parameter set and writes <name>.vox and
<name>.glb for each into --out, then a manifest (default <out>/variants.json)
with each set's parameters, markers, and bounds. sets.json is a list of
{\"name\": \"low\", \"params\": {\"height\": 12, \"wall/seed\": 3}}, or an object
with that list under \"variants\". The mesh options are those of froxel export.";

fn cli_args(args: &[String]) -> Result<Option<(PathBuf, Value)>, String> {
    let mut input = None;
    let mut sets = None;
    let mut params = json!({});
    let mut index = 0;
    let next = |index: &mut usize, what: &str| -> Result<String, String> {
        *index += 1;
        args.get(*index)
            .cloned()
            .ok_or_else(|| format!("{what} needs a value\n{USAGE}"))
    };
    while index < args.len() {
        match args[index].as_str() {
            "-h" | "--help" => {
                eprintln!("{USAGE}");
                return Ok(None);
            }
            "--input" => input = Some(PathBuf::from(next(&mut index, "--input")?)),
            "--variants" => sets = Some(PathBuf::from(next(&mut index, "--variants")?)),
            "--out" => params["dir"] = json!(next(&mut index, "--out")?),
            "--manifest" => params["manifest"] = json!(next(&mut index, "--manifest")?),
            "--no-vox" => params["vox"] = json!(false),
            "--merge" => params["merge"] = json!(true),
            "--vertex-colors" => params["vertexColors"] = json!(true),
            "--color-atlas" => params["colorAtlas"] = json!(true),
            "--export-hidden" => params["exportHidden"] = json!(true),
            "--split-objects" => params["splitObjects"] = json!(true),
            "--ao" => {
                let value = next(&mut index, "--ao")?;
                params["aoStrength"] =
                    json!(crate::bridge::parse_ao_strength(Some(value.as_str()))?);
            }
            "--ao-radius" => {
                let value = next(&mut index, "--ao-radius")?;
                params["aoRadius"] = json!(crate::bridge::parse_ao_radius(Some(value.as_str()))?);
            }
            "--ao-target" => {
                let value = next(&mut index, "--ao-target")?;
                params["aoTarget"] =
                    json!(crate::bridge::parse_ao_target(Some(value.as_str()))?.name());
            }
            other => return Err(format!("unknown argument `{other}`\n{USAGE}")),
        }
        index += 1;
    }
    let (Some(input), Some(sets)) = (input, sets) else {
        return Err(USAGE.into());
    };
    if params.get("dir").is_none() {
        return Err(USAGE.into());
    }
    params["variants"] = read_sets(&sets)?;
    Ok(Some((input, params)))
}

/// A variants file: a list of sets, or an object with one under `variants`.
pub fn read_sets(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|err| format!("Could not read {}: {err}", path.display()))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|err| format!("{} is not JSON: {err}", path.display()))?;
    match value {
        Value::Array(_) => Ok(value),
        Value::Object(mut map) => map
            .remove("variants")
            .filter(Value::is_array)
            .ok_or_else(|| format!("{} has no variants list", path.display())),
        _ => Err(format!(
            "{} must hold a list of parameter sets",
            path.display()
        )),
    }
}
