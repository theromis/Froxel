//! Variant export behind MCP `export_variants`, `froxel run export_variants`, and the
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

/// Export every variant in `params` and merge them into the manifest by
/// name, or write a fresh one with `replace`. The reply names the manifest
/// and the names it `added`, `replaced`, and `kept`, and sums up each set
/// exported now ([`compact_reply`]); `reply: "full"` returns the whole
/// manifest instead.
pub fn export_variants(session: &mut Session, params: &Value) -> Result<Value, String> {
    let dir = PathBuf::from(
        params
            .get("dir")
            .and_then(Value::as_str)
            .ok_or("dir is required: the folder the files go to")?,
    );
    // A list of sets, or the path of a JSON file of them, as preview takes.
    let read;
    let list = match params.get("variants") {
        Some(Value::String(path)) => {
            read = read_sets(Path::new(path))?;
            read.as_array()
                .ok_or("variants must be a list of {name?, params}")?
        }
        found => found
            .and_then(Value::as_array)
            .ok_or("variants must be a list of {name?, params}, or a JSON file of them")?,
    };
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
    let manifest_path = match params.get("manifest").and_then(Value::as_str) {
        Some(path) => PathBuf::from(path),
        None => dir.join(MANIFEST),
    };
    let full = crate::remote::full_reply(params)?;
    let replace = match params.get("replace") {
        None | Some(Value::Null) => false,
        Some(value) => value.as_bool().ok_or("replace must be a boolean")?,
    };
    let previous = if replace {
        None
    } else {
        read_manifest(&manifest_path)?
    };
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
    let options = crate::remote::mesh_params(&options);
    let (manifest, changes) = merge_manifest(previous, variants, &options);
    let text = serde_json::to_string_pretty(&manifest).map_err(|err| err.to_string())?;
    crate::state::create_parent(&manifest_path)?;
    std::fs::write(&manifest_path, text + "\n")
        .map_err(|err| format!("Could not write {}: {err}", manifest_path.display()))?;
    let mut reply = if full {
        manifest
    } else {
        compact_reply(&manifest, &changes)
    };
    reply["manifest"] = json!(manifest_path.display().to_string());
    reply["added"] = json!(changes.added);
    reply["replaced"] = json!(changes.replaced);
    reply["kept"] = json!(changes.kept);
    Ok(reply)
}

/// The default reply: per set exported now its name, files, bounds, voxels,
/// quads, scene shift, and markers counted by kind. Parameters and marker
/// detail stay in the manifest file; kept entries are named only.
pub fn compact_reply(manifest: &Value, changes: &ManifestChanges) -> Value {
    let exported: HashSet<String> = changes
        .added
        .iter()
        .chain(&changes.replaced)
        .map(|name| name.to_lowercase())
        .collect();
    let variants: Vec<Value> = manifest["variants"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| exported.contains(&entry["name"].as_str().unwrap_or("").to_lowercase()))
        .map(|entry| {
            let mut out = Map::new();
            for key in [
                "name",
                "glb",
                "vox",
                "bounds",
                "voxels",
                "quads",
                "sceneShift",
            ] {
                if let Some(value) = entry.get(key) {
                    out.insert(key.into(), value.clone());
                }
            }
            out.insert(
                "markers".into(),
                crate::remote::marker_counts(&entry["markers"]),
            );
            Value::Object(out)
        })
        .collect();
    let mut reply = json!({"reply": "compact", "variants": variants});
    if let Some(options) = manifest.get("options") {
        reply["options"] = options.clone();
    }
    reply
}

/// The variant names an export added, replaced, and kept in its manifest.
#[derive(Debug, Default, PartialEq)]
pub struct ManifestChanges {
    pub added: Vec<String>,
    pub replaced: Vec<String>,
    pub kept: Vec<String>,
}

/// The manifest at `path` when there is one. A file that is not a manifest
/// is an error rather than something to overwrite.
fn read_manifest(path: &Path) -> Result<Option<Value>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("Could not read {}: {err}", path.display())),
    };
    let value: Value = serde_json::from_str(&text).map_err(|err| {
        format!(
            "{} is not JSON ({err}); pass replace: true to write a fresh manifest",
            path.display()
        )
    })?;
    if !value.get("variants").is_some_and(Value::is_array) {
        return Err(format!(
            "{} has no variants list; pass replace: true to write a fresh manifest",
            path.display()
        ));
    }
    Ok(Some(value))
}

/// Merge this call's entries into `previous` by name, ignoring case: an
/// entry exported now replaces the one of its name in place, the others
/// stay in their order, and new names are appended. Every entry records
/// its mesh `options` (a kept entry from an older manifest takes the old
/// top-level ones); the top-level `options` is there only while every entry
/// shares it. Other top-level fields of `previous` are kept.
pub fn merge_manifest(
    previous: Option<Value>,
    exported: Vec<Value>,
    options: &Value,
) -> (Value, ManifestChanges) {
    let mut manifest = match previous {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    };
    let old_options = manifest.remove("options");
    let mut entries: Vec<Value> = match manifest.remove("variants") {
        Some(Value::Array(entries)) => entries,
        _ => Vec::new(),
    };
    let key = |entry: &Value| entry["name"].as_str().unwrap_or("").to_lowercase();
    let mut fresh = vec![false; entries.len()];
    let mut changes = ManifestChanges::default();
    for mut entry in exported {
        entry["options"] = options.clone();
        let name = entry["name"].as_str().unwrap_or("").to_string();
        match entries
            .iter()
            .position(|old| key(old) == name.to_lowercase())
        {
            Some(index) => {
                entries[index] = entry;
                fresh[index] = true;
                changes.replaced.push(name);
            }
            None => {
                entries.push(entry);
                fresh.push(true);
                changes.added.push(name);
            }
        }
    }
    for (entry, fresh) in entries.iter_mut().zip(&fresh) {
        if *fresh {
            continue;
        }
        changes
            .kept
            .push(entry["name"].as_str().unwrap_or("").to_string());
        if entry.get("options").is_none_or(Value::is_null) {
            if let (Some(old), Some(map)) = (&old_options, entry.as_object_mut()) {
                map.insert("options".into(), old.clone());
            }
        }
    }
    manifest.insert("generator".into(), json!("Froxel"));
    if entries.iter().all(|entry| entry["options"] == *options) {
        manifest.insert("options".into(), options.clone());
    }
    manifest.insert("variants".into(), Value::Array(entries));
    (Value::Object(manifest), changes)
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
    if let Some(shift) = session.scene_shift() {
        entry["sceneShift"] = json!(shift);
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

/// "2 added, 1 replaced, 9 kept" from an export reply.
pub fn summary(reply: &Value) -> String {
    let count = |key: &str| reply[key].as_array().map_or(0, Vec::len);
    format!(
        "{} added, {} replaced, {} kept",
        count("added"),
        count("replaced"),
        count("kept")
    )
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
