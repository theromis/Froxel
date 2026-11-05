//! Document operations shared by the headless MCP session and the window's
//! control port. Screenshots are not applied here: only a live window can
//! take one.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use serde_json::{json, Map, Value};

mod batch;
mod foliage;
mod import;
mod markers;
mod organic;
mod silhouette;
mod stone;
pub(crate) use batch::{batch_items, batch_tool_names, MAX_BATCH_ITEMS};
pub(crate) use import::{
    import_plan, used_slots, DEFAULT_IMPORT_SIZE, DEFAULT_MAX_NEW_SLOTS, DEFAULT_TOLERANCE,
};
pub(crate) use markers::marker_kind;
pub(crate) use organic::{organic_writes, Organic};
pub(crate) use silhouette::{hull_writes, session_palette};
pub(crate) use stone::{cavity_writes, stone_writes, Stone};

use crate::state::{Cell, GlassMode, Session};

pub const NO_WINDOW: &str = "No window. Start the editor with `froxel --control PORT`, then `froxel mcp --connect 127.0.0.1:PORT`. Add `--launch` to open the window.";

/// Largest side of a `render` or `model_view` PNG.
const MAX_SIDE: i32 = 4096;
/// Largest `render` frame in pixels, so a CPU trace stays bounded.
const MAX_RENDER_PIXELS: i64 = 4096 * 2048;

/// What a still frames: every visible cell, one object, or a world box.
enum FrameTarget {
    All,
    Object(usize),
    Box(glam::Vec3, glam::Vec3),
}

/// Camera fit shared by `render` and `model_view`: the target, the margin,
/// and the distance multiplier (`distance` / `zoom`).
struct Framing {
    target: FrameTarget,
    margin: f32,
    scale: f32,
}

fn framing(session: &Session, params: &Value) -> Result<Framing, String> {
    let distance = opt_f64(params, "distance")?.unwrap_or(1.0);
    if !(distance > 0.0) || distance > 100.0 {
        return Err("distance is out of range".into());
    }
    let zoom = opt_f64(params, "zoom")?.unwrap_or(1.0);
    if !(zoom >= 0.01) || zoom > 100.0 {
        return Err("zoom must be in 0.01..100".into());
    }
    let margin = opt_f64(params, "margin")?.unwrap_or(crate::still::MARGIN as f64);
    if !(0.0..=0.45).contains(&margin) {
        return Err("margin must be in 0..0.45".into());
    }
    let target = match params.get("target") {
        None | Some(Value::Null) => FrameTarget::All,
        Some(Value::String(text)) if text == "all" => FrameTarget::All,
        Some(Value::String(text)) if text == "active" => FrameTarget::Object(session.selected),
        Some(Value::Object(map)) => {
            if let Some(key) = map.keys().find(|key| *key != "min" && *key != "max") {
                return Err(format!("target has no field `{key}`; pass min and max"));
            }
            let min = opt_ivec3(&params["target"], "min")?.ok_or("target needs min")?;
            let max = opt_ivec3(&params["target"], "max")?.ok_or("target needs max")?;
            if (0..3).any(|axis| min[axis] > max[axis]) {
                return Err("target min must not exceed max".into());
            }
            let corner =
                |cell: [i32; 3]| glam::Vec3::new(cell[0] as f32, cell[1] as f32, cell[2] as f32);
            FrameTarget::Box(corner(min), corner(max) + glam::Vec3::ONE)
        }
        Some(found) => {
            let Some(index) = as_i32(found) else {
                return Err("target must be all, active, an object index, or {min, max}".into());
            };
            if index < 0 || index as usize >= session.objects.len() {
                return Err(format!("target object {index} does not exist"));
            }
            FrameTarget::Object(index as usize)
        }
    };
    Ok(Framing {
        target,
        margin: margin as f32,
        scale: (distance / zoom) as f32,
    })
}

/// The points a framing target keeps in the picture, with the session's
/// current pose.
fn target_frame(session: &Session, target: &FrameTarget) -> Result<crate::trace::Frame, String> {
    match target {
        FrameTarget::All => crate::trace::Frame::cells(session, None)
            .ok_or_else(|| "document has no voxels".to_string()),
        FrameTarget::Object(index) => {
            if session.objects[*index].hidden {
                return Err(format!("target object {index} is hidden"));
            }
            crate::trace::Frame::cells(session, Some(*index))
                .ok_or_else(|| format!("target object {index} has no voxels"))
        }
        FrameTarget::Box(min, max) => {
            if crate::trace::Frame::cells(session, None).is_none() {
                return Err("document has no voxels".into());
            }
            Ok(crate::trace::Frame::corners(*min, *max))
        }
    }
}

/// Width and height of a still. One given side keeps it and the other
/// follows the frame's projected shape at every angle of the call, so a
/// long model gets a wide picture. `pixels_per_voxel` sizes both omitted
/// sides from the frame. Neither given is `fallback` square.
fn still_size(
    params: &Value,
    fallback: i32,
    frame: &crate::trace::Frame,
    margin: f32,
    angles: &[(f32, f32)],
    pixels_per_voxel: Option<f32>,
) -> Result<(u32, u32), String> {
    let width = opt_i32_range(params, "width", 0, 1, MAX_SIDE)?;
    let height = opt_i32_range(params, "height", 0, 1, MAX_SIDE)?;
    if width > 0 && height > 0 {
        return Ok((width as u32, height as u32));
    }
    let (mut half_w, mut half_h) = (0.0f32, 0.0f32);
    for (yaw, pitch) in angles {
        let (w, h) = frame.half_extent(yaw.to_radians(), pitch.to_radians());
        half_w = half_w.max(w);
        half_h = half_h.max(h);
    }
    let (half_w, half_h) = (half_w.max(0.5), half_h.max(0.5));
    let fill = 1.0 - 2.0 * margin;
    let side = |pixels: f32, key: &str| -> Result<u32, String> {
        let pixels = pixels.round().max(1.0);
        if pixels > MAX_SIDE as f32 {
            return Err(format!(
                "{key} would be {pixels} pixels, over {MAX_SIDE}; lower pixelsPerVoxel or pass {key}"
            ));
        }
        Ok(pixels as u32)
    };
    match (width, height, pixels_per_voxel) {
        (0, 0, Some(ppv)) => Ok((
            side(2.0 * half_w * ppv / fill, "width")?,
            side(2.0 * half_h * ppv / fill, "height")?,
        )),
        (0, 0, None) => Ok((fallback as u32, fallback as u32)),
        (0, height, _) => Ok((
            side(height as f32 * half_w / half_h, "width")?,
            height as u32,
        )),
        (width, _, _) => Ok((
            width as u32,
            side(width as f32 * half_h / half_w, "height")?,
        )),
    }
}

fn render_scene(session: &mut Session, params: &Value) -> Result<Value, String> {
    let path = req_str(params, "path")?;
    if path.is_empty() {
        return Err("path is empty".into());
    }
    let samples = opt_i32_range(params, "samples", 16, 1, 128)?;
    let bounces = opt_i32_range(params, "bounces", 4, 0, 8)?;
    let seed = opt_i32_range(params, "seed", 1, 0, i32::MAX)?;
    let denoise = opt_bool(params, "denoise")?.unwrap_or(true);
    let atmospheric = opt_bool(params, "atmospheric")?;
    let sky_intensity = opt_f64(params, "sky-intensity")?;
    let sun_intensity = opt_f64(params, "sun-intensity")?;
    let aperture = opt_f64(params, "aperture")?;
    let focus = opt_f64(params, "focus")?;
    let glass = match opt_str(params, "glass-mode")? {
        None => None,
        Some(text) if text == "probabilistic" => Some(GlassMode::Probabilistic),
        Some(text) if text == "absorption" => Some(GlassMode::Absorption),
        Some(_) => {
            return Err("glass-mode must be 'probabilistic' or 'absorption'".into());
        }
    };
    let (yaw, pitch) = view_angles(params)?;
    let framing = framing(session, params)?;
    let orthographic = opt_bool(params, "orthographic")?.unwrap_or(false);
    let show_bones = opt_bool(params, "showBones")?.unwrap_or(false);
    let views = render_views(params)?;
    let pose = match render_pose(params)? {
        Some(pose) => Some(session.checked_pose(pose)?),
        None => None,
    };
    let yaw = yaw.unwrap_or(30.0) as f32;
    let pitch = pitch.unwrap_or(25.0) as f32;
    let mut angles = vec![(yaw, pitch)];
    angles.extend(views.iter().map(|(yaw, pitch, _)| (*yaw, *pitch)));

    let saved_pose = session.pose.clone();
    let saved_bones = session.show_bones;
    if let Some(pose) = pose {
        session.pose = pose;
    }
    let sized = target_frame(session, &framing.target).and_then(|frame| {
        let (width, height) = still_size(params, 256, &frame, framing.margin, &angles, None)?;
        if width as i64 * height as i64 > MAX_RENDER_PIXELS {
            return Err(format!(
                "{width} x {height} is over {MAX_RENDER_PIXELS} pixels; use model_view for a larger flat view"
            ));
        }
        Ok((frame, width, height))
    });
    let (frame, width, height) = match sized {
        Ok(sized) => sized,
        Err(err) => {
            session.pose = saved_pose;
            return Err(err);
        }
    };
    let still = crate::still::Still {
        width,
        height,
        samples: samples as u32,
        bounces,
        seed: seed as u32,
        aces: true,
        denoise,
        atmospheric,
        sky_intensity: sky_intensity.map(|value| value as f32),
        sun_intensity: sun_intensity.map(|value| value as f32),
        aperture: aperture.map(|value| value as f32),
        focus: focus.map(|value| value as f32),
        glass,
        stochastic_gi: None,
        yaw,
        pitch,
        distance: framing.scale,
        orthographic: Some(orthographic),
        frame: Some(frame),
        margin: framing.margin,
        pixels_per_voxel: None,
    };
    let (min, max) = still
        .frame
        .as_ref()
        .map(|frame| frame.bounds())
        .unwrap_or_default();
    let framed = json!({"min": min.to_array(), "max": max.to_array()});
    let saved = crate::still::prepare(session, &still);
    crate::still::fit_all(session, &still, &angles);
    session.show_bones = show_bones;
    let distance = session.distance;
    let written = render_views_to(
        session,
        width,
        height,
        samples as u32,
        seed as u32,
        &path,
        &views,
    );
    session.pose = saved_pose;
    session.show_bones = saved_bones;
    crate::still::finish(session, saved);
    let mut reply = json!({
        "path": path,
        "width": width,
        "height": height,
        "samples": samples,
        "framed": framed,
        "distance": distance
    });
    let files = written?;
    if !views.is_empty() {
        reply["views"] = json!(files);
    }
    Ok(reply)
}

/// The flat model view: palette colours of the first cell hit, no light and
/// no tonemap. Orthographic unless the caller asks for perspective.
fn model_view(session: &mut Session, params: &Value) -> Result<Value, String> {
    let path = req_str(params, "path")?;
    if path.is_empty() {
        return Err("path is empty".into());
    }
    let (yaw, pitch) = view_angles(params)?;
    let framing = framing(session, params)?;
    let orthographic = opt_bool(params, "orthographic")?.unwrap_or(true);
    let pixels_per_voxel = opt_f64(params, "pixelsPerVoxel")?;
    if let Some(ppv) = pixels_per_voxel {
        if !orthographic {
            return Err("pixelsPerVoxel needs orthographic".into());
        }
        if !(ppv >= 0.05) || ppv > 256.0 {
            return Err("pixelsPerVoxel must be in 0.05..256".into());
        }
        if params.get("zoom").is_some() || params.get("distance").is_some() {
            return Err("pass pixelsPerVoxel or zoom and distance, not both".into());
        }
    }
    let pixels_per_voxel = pixels_per_voxel.map(|ppv| ppv as f32);
    let show_bones = opt_bool(params, "showBones")?.unwrap_or(false);
    let background = background(params)?;
    let views = render_views(params)?;
    let pose = match render_pose(params)? {
        Some(pose) => Some(session.checked_pose(pose)?),
        None => None,
    };
    let yaw = yaw.unwrap_or(30.0) as f32;
    let pitch = pitch.unwrap_or(25.0) as f32;
    let mut shots = vec![(yaw, pitch, path.clone())];
    shots.extend(views.iter().cloned());
    let angles: Vec<(f32, f32)> = shots.iter().map(|(yaw, pitch, _)| (*yaw, *pitch)).collect();

    let saved_pose = session.pose.clone();
    if let Some(pose) = pose {
        session.pose = pose;
    }
    let sized = target_frame(session, &framing.target).and_then(|frame| {
        let (width, height) = still_size(
            params,
            512,
            &frame,
            framing.margin,
            &angles,
            pixels_per_voxel,
        )?;
        Ok((frame, width, height))
    });
    let (frame, width, height) = match sized {
        Ok(sized) => sized,
        Err(err) => {
            session.pose = saved_pose;
            return Err(err);
        }
    };
    let still = crate::still::Still {
        width,
        height,
        samples: 1,
        bounces: 0,
        seed: 0,
        aces: false,
        denoise: false,
        atmospheric: None,
        sky_intensity: None,
        sun_intensity: None,
        aperture: None,
        focus: None,
        glass: None,
        stochastic_gi: None,
        yaw,
        pitch,
        distance: framing.scale,
        orthographic: Some(orthographic),
        frame: Some(frame),
        margin: framing.margin,
        pixels_per_voxel,
    };
    let (min, max) = still
        .frame
        .as_ref()
        .map(|frame| frame.bounds())
        .unwrap_or_default();
    let framed = json!({"min": min.to_array(), "max": max.to_array()});
    let saved = crate::still::prepare(session, &still);
    crate::still::fit_all(session, &still, &angles);
    let mut files = Vec::new();
    let mut written = Ok(());
    for (view_yaw, view_pitch, view_path) in &shots {
        session.yaw = view_yaw.to_radians();
        session.pitch = view_pitch.to_radians();
        let mut rgba = crate::trace::render_flat(session, width, height, background);
        if show_bones {
            crate::trace::draw_bone_overlay(&mut rgba, width, height, session);
        }
        written = crate::trace::write_rgba_png(Path::new(view_path), width, height, &rgba);
        if written.is_err() {
            break;
        }
        files.push(json!({"path": view_path, "yaw": view_yaw, "pitch": view_pitch}));
    }
    let pixels_per_voxel = crate::trace::ortho_pixels_per_voxel(session, height);
    let distance = session.distance;
    session.pose = saved_pose;
    crate::still::finish(session, saved);
    written?;
    let mut reply = json!({
        "path": path,
        "width": width,
        "height": height,
        "orthographic": orthographic,
        "framed": framed,
        "distance": distance
    });
    if orthographic {
        reply["pixelsPerVoxel"] = json!(pixels_per_voxel);
    }
    if !views.is_empty() {
        reply["views"] = json!(files[1..]);
    }
    Ok(reply)
}

/// The fast lit preview: the viewport's mesh and light, rasterised on the
/// CPU and framed on the model or one object.
fn preview(session: &mut Session, params: &Value) -> Result<Value, String> {
    let path = req_str(params, "path")?;
    if path.is_empty() {
        return Err("path is empty".into());
    }
    let max = crate::preview::MAX_SIZE as i32;
    let sheet = crate::sheet::wanted(params);
    // A sheet's width and height are each tile's.
    let side = if sheet { crate::sheet::TILE } else { 1024 } as i32;
    let mut preview = crate::preview::Preview {
        width: opt_i32_range(params, "width", side, 1, max)? as u32,
        height: opt_i32_range(params, "height", side, 1, max)? as u32,
        ..Default::default()
    };
    preview_camera(params, &mut preview)?;
    preview.overlay = overlay_arg(params)?;
    preview.export = preview_export(params)?;
    preview.counts_label = opt_bool(params, "countsLabel")?.unwrap_or(false);
    preview.lighting = preview_lighting(params)?;
    if let Some(background) = preview.lighting.as_ref().and_then(|l| l.background) {
        preview.background = background;
    }
    if sheet {
        let object = match params.get("object") {
            None | Some(Value::Null) => None,
            Some(Value::String(name)) => Some(name.clone()),
            Some(Value::Number(number)) => Some(number.to_string()),
            Some(_) => return Err("object must be an index or a name".into()),
        };
        preview.isolate = opt_bool(params, "isolate")?.unwrap_or(false);
        preview.antialias = opt_bool(params, "antialias")?.unwrap_or(true);
        if params
            .get("background")
            .is_some_and(|value| !value.is_null())
        {
            preview.background = background(params)?;
        }
        if preview.isolate && object.is_none() {
            return Err("isolate needs object".into());
        }
        if render_pose(params)?.is_some() {
            return Err("pose is not used with a contact sheet".into());
        }
        crate::preview::check(&crate::preview::Preview {
            object: object.as_ref().map(|_| 0),
            ..preview.clone()
        })?;
        return crate::sheet::contact_sheet(
            session,
            params,
            &preview,
            object.as_deref(),
            Path::new(&path),
        );
    }
    preview.object = preview_object(session, params)?;
    preview.isolate = opt_bool(params, "isolate")?.unwrap_or(false);
    preview.antialias = opt_bool(params, "antialias")?.unwrap_or(true);
    if params
        .get("background")
        .is_some_and(|value| !value.is_null())
    {
        preview.background = background(params)?;
    }
    let pose = match render_pose(params)? {
        Some(pose) => Some(session.checked_pose(pose)?),
        None => None,
    };
    let saved_pose = session.pose.clone();
    if let Some(pose) = pose {
        session.pose = pose;
    }
    let started = std::time::Instant::now();
    let written = crate::preview::write(session, &preview, Path::new(&path));
    session.pose = saved_pose;
    let frame = written?;
    let mut reply = json!({
        "path": path,
        "width": preview.width,
        "height": preview.height,
        "yaw": preview.yaw,
        "placementYaw": preview.placement_yaw,
        "pitch": preview.pitch,
        "orthographic": preview.orthographic,
        "previewTriangles": frame.preview_triangles,
        "milliseconds": started.elapsed().as_millis() as u64
    });
    if let Some(export) = export_counts_json(&preview, &frame) {
        reply["export"] = export;
    }
    if let Some(lighting) = &preview.lighting {
        let sun = lighting.sun_direction.unwrap_or_else(|| session.sun_dir());
        reply["lighting"] = lighting.json(sun);
    }
    if let Some(index) = preview.object {
        reply["object"] = json!({"index": index, "name": session.objects[index].name});
    }
    if let Some(ppv) = frame.pixels_per_voxel {
        reply["pixelsPerVoxel"] = json!(ppv);
    }
    if preview.overlay.any() {
        reply["overlay"] = overlay_json(&frame.overlay);
    }
    Ok(reply)
}

/// MCP `preview` `preset`, `presets`, `view`, `yaw`, `pitch`,
/// `orthographic`, `margin`, and `fov` onto `preview`. Arguments win over
/// the preset. `placementYaw` then turns the camera the other way, into the
/// frame of an asset turned that much where it is placed.
fn preview_camera(params: &Value, preview: &mut crate::preview::Preview) -> Result<(), String> {
    let preset = match opt_str(params, "preset")? {
        Some(name) => Some(crate::sheet::preset(&name, params.get("presets"))?),
        None => None,
    };
    if let Some(preset) = preset {
        preview.yaw = preset.yaw;
        preview.pitch = preset.pitch;
        preview.orthographic = preset.orthographic;
        preview.fov = preset.fov.unwrap_or(preview.fov);
        preview.margin = preset.margin.unwrap_or(preview.margin);
    }
    let (yaw, pitch) = view_angles(params)?;
    preview.yaw = yaw.map_or(preview.yaw, |value| value as f32);
    preview.pitch = pitch.map_or(preview.pitch, |value| value as f32);
    preview.orthographic = opt_bool(params, "orthographic")?.unwrap_or(preview.orthographic);
    if let Some(margin) = opt_f64(params, "margin")? {
        preview.margin = margin as f32;
    }
    if let Some(fov) = opt_f64(params, "fov")? {
        preview.fov = fov as f32;
    }
    if let Some(turn) = opt_f64(params, "placementYaw")? {
        preview.placement_yaw = turn as f32;
        preview.yaw -= preview.placement_yaw;
    }
    Ok(())
}

/// MCP `preview` `object`: an index or a name.
fn preview_object(session: &Session, params: &Value) -> Result<Option<usize>, String> {
    Ok(match params.get("object") {
        None | Some(Value::Null) => None,
        Some(Value::String(name)) => Some(crate::preview::find_object(session, name)?),
        Some(Value::Number(number)) => {
            let index = number.as_u64().ok_or("object must be an index or a name")?;
            Some(crate::preview::find_object(session, &index.to_string())?)
        }
        Some(_) => return Err("object must be an index or a name".into()),
    })
}

/// MCP `preview` `lighting` (a preset name or an object of lighting
/// fields) with `lightingPresets` (an object or a presets file). `None` is
/// the viewport's light.
pub(crate) fn preview_lighting(
    params: &Value,
) -> Result<Option<crate::lighting::Lighting>, String> {
    let given = params.get("lighting").filter(|value| !value.is_null());
    let presets = params
        .get("lightingPresets")
        .filter(|value| !value.is_null());
    let Some(given) = given else {
        if presets.is_some() {
            return Err("lightingPresets needs lighting to name one".into());
        }
        return Ok(None);
    };
    crate::lighting::Presets::load(presets)?.resolve(given)
}

/// MCP `preview` `export`: the export options its counts use, as
/// `export_mesh` takes them (all off when absent), or false for no counts.
pub(crate) fn preview_export(params: &Value) -> Result<Option<crate::export::MeshExport>, String> {
    match params.get("export") {
        None | Some(Value::Null) | Some(Value::Bool(true)) => Ok(Some(mesh_options(&json!({}))?)),
        Some(Value::Bool(false)) => Ok(None),
        Some(options @ Value::Object(map)) => {
            const KEYS: [&str; 8] = [
                "merge",
                "vertexColors",
                "exportHidden",
                "splitObjects",
                "colorAtlas",
                "aoStrength",
                "aoRadius",
                "aoTarget",
            ];
            if let Some(key) = map.keys().find(|key| !KEYS.contains(&key.as_str())) {
                return Err(format!(
                    "export takes the export_mesh options {}, not `{key}`",
                    KEYS.join(", ")
                ));
            }
            Ok(Some(mesh_options(options)?))
        }
        Some(_) => Err("export must be an object of export_mesh options, or false".into()),
    }
}

/// The reply's `export`: the triangles and quads an export with the
/// preview's options writes, and those options, or the export's error.
pub(crate) fn export_counts_json(
    preview: &crate::preview::Preview,
    frame: &crate::preview::Frame,
) -> Option<Value> {
    let options = preview.export.as_ref()?;
    let mut out = mesh_params(options);
    match frame.export.as_ref()? {
        Ok(counts) => {
            out["triangles"] = json!(counts.triangles);
            out["quads"] = json!(counts.quads);
        }
        Err(err) => out["error"] = json!(err),
    }
    Some(out)
}

/// MCP `preview` `overlay`: true or `"all"` for every marker kind, or one of
/// `boxes`, `points`, `connectors`, `markers` (every kind), or a list of
/// them; `overlayLabels` (default true) names each marker.
pub(crate) fn overlay_arg(params: &Value) -> Result<crate::preview::Overlay, String> {
    use crate::preview::Overlay;
    let bad = "overlay must be true, all, markers, boxes, points, connectors, or a list of them";
    let names: Vec<Value> = match params.get("overlay") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => return Ok(Overlay::default()),
        Some(Value::Bool(true)) => vec![json!("all")],
        Some(Value::Array(list)) => list.clone(),
        Some(one @ Value::String(_)) => vec![one.clone()],
        Some(_) => return Err(bad.into()),
    };
    let mut overlay = Overlay::default();
    for name in names {
        match name.as_str().ok_or(bad)? {
            "all" | "markers" => {
                overlay.boxes = true;
                overlay.points = true;
                overlay.connectors = true;
            }
            "boxes" | "collision" => overlay.boxes = true,
            "points" => overlay.points = true,
            "connectors" => overlay.connectors = true,
            _ => return Err(bad.into()),
        }
    }
    overlay.labels = opt_bool(params, "overlayLabels")?.unwrap_or(true);
    Ok(overlay)
}

/// The overlay's markers for a reply.
pub(crate) fn overlay_json(drawn: &[crate::preview::Drawn]) -> Value {
    Value::Array(
        drawn
            .iter()
            .map(|mark| {
                json!({
                    "name": mark.name,
                    "kind": mark.kind,
                    "object": mark.object,
                    "pixel": mark.pixel,
                    "hidden": mark.hidden
                })
            })
            .collect(),
    )
}

/// Move the window camera. frame re-centres on every object first, so a
/// named view always shows the whole model.
fn set_view(session: &mut Session, params: &Value) -> Result<Value, String> {
    let (mut yaw, mut pitch) = view_angles(params)?;
    let preset = match opt_str(params, "preset")? {
        Some(name) => Some(crate::sheet::preset(&name, params.get("presets"))?),
        None => None,
    };
    if let Some(preset) = preset {
        yaw = yaw.or(Some(f64::from(preset.yaw)));
        pitch = pitch.or(Some(f64::from(preset.pitch)));
    }
    let placement = opt_f64(params, "placementYaw")?;
    if let Some(turn) = placement {
        // With no angle given this would turn further on every call.
        yaw = Some(yaw.ok_or("placementYaw needs yaw, view, or preset")? - turn);
    }
    let frame = opt_bool(params, "frame")?.unwrap_or(true);
    let distance = opt_f64(params, "distance")?;
    if let Some(distance) = distance {
        if !frame {
            return Err("distance multiplies the framing distance and needs frame".into());
        }
        if !(distance > 0.0) || distance > 100.0 {
            return Err("distance is out of range".into());
        }
    }
    let orthographic =
        opt_bool(params, "orthographic")?.or(preset.map(|preset| preset.orthographic));
    let bones = opt_bool(params, "bones")?;
    let (old_yaw, old_pitch) = (session.yaw, session.pitch);
    if frame {
        session.frame_camera();
        session.distance *= distance.unwrap_or(1.0) as f32;
    }
    session.yaw = yaw.map_or(old_yaw, |deg| (deg as f32).to_radians());
    session.pitch = pitch.map_or(old_pitch, |deg| (deg as f32).to_radians());
    if let Some(orthographic) = orthographic {
        session.orthographic = orthographic;
    }
    if let Some(bones) = bones {
        session.view_bones = bones;
    }
    Ok(json!({
        "yaw": session.yaw.to_degrees(),
        "pitch": session.pitch.to_degrees(),
        "distance": session.distance,
        "target": session.target.to_array(),
        "orthographic": session.orthographic,
        "bones": session.view_bones
    }))
}

fn render_views_to(
    session: &mut Session,
    width: u32,
    height: u32,
    samples: u32,
    seed: u32,
    path: &str,
    views: &[(f32, f32, String)],
) -> Result<Vec<Value>, String> {
    let image = crate::trace::render_still(session, width, height, samples, seed);
    crate::trace::write_png(Path::new(path), &image, session)?;
    let mut files = Vec::new();
    let yaw = session.yaw;
    let pitch = session.pitch;
    for (view_yaw, view_pitch, view_path) in views {
        session.yaw = view_yaw.to_radians();
        session.pitch = view_pitch.to_radians();
        let image = crate::trace::render_still(session, width, height, samples, seed);
        crate::trace::write_png(Path::new(view_path), &image, session)?;
        files.push(json!({"path": view_path, "yaw": view_yaw, "pitch": view_pitch}));
    }
    session.yaw = yaw;
    session.pitch = pitch;
    Ok(files)
}

pub fn apply(session: &mut Session, method: &str, params: &Value) -> Result<Value, String> {
    let params = if params.is_null() {
        Value::Object(Map::new())
    } else {
        params.clone()
    };
    match method {
        "document.new" => {
            session.replace_empty();
            Ok(json!({"ok": true}))
        }
        "document.open" => {
            let path = req_str(&params, "path")?;
            session.open_vox(Path::new(&path))?;
            match opt_str(&params, "detail")?.as_deref() {
                None | Some("summary") => Ok(open_summary(session)),
                Some("full") => inspect(session, &params),
                Some(_) => Err("detail must be summary or full".to_string()),
            }
        }
        "document.save" => {
            let path = match opt_str(&params, "path")? {
                Some(path) => path,
                None => session
                    .path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .ok_or_else(|| "document has no path; pass path".to_string())?,
            };
            session.write_vox(Path::new(&path))?;
            let voxels: usize = session
                .objects
                .iter()
                .map(|object| object.cells.values().filter(|slot| **slot != 0).count())
                .sum();
            let objects = session
                .objects
                .iter()
                .filter(|object| object.cells.values().any(|slot| *slot != 0))
                .count();
            Ok(json!({"path": path, "voxels": voxels, "objects": objects}))
        }
        "document.inspect" => inspect(session, &params),
        "voxel.batch" => batch::apply_batch(session, &params),
        "voxel.get" => {
            let (x, y, z) = xyz(&params)?;
            let palette = session.active().cells.get(&(x, y, z)).copied().unwrap_or(0);
            Ok(json!({"x": x, "y": y, "z": z, "palette": palette}))
        }
        "voxel.set" => {
            let (x, y, z) = xyz(&params)?;
            let palette = req_i32(&params, "palette")?;
            let changed = session.write_cells(&[(x, y, z, palette)])?;
            Ok(json!({"x": x, "y": y, "z": z, "palette": palette, "changed": changed}))
        }
        "voxel.set_many" => {
            let writes = cell_writes(&params)?;
            let changed = session.write_cells(&writes)?;
            Ok(json!({"changed": changed, "voxels": session.voxel_count()}))
        }
        "voxel.fill" => sculpt_fill(session, &params, Sculpt::Box),
        "voxel.ellipsoid" => sculpt_fill(session, &params, Sculpt::Ellipsoid),
        "voxel.rounded" => sculpt_fill(session, &params, Sculpt::RoundedBox),
        "voxel.tube" => sculpt_fill(session, &params, Sculpt::Tube),
        "voxel.carve" => carve(session, &params),
        "voxel.smooth" => smooth(session, &params),
        "voxel.break" => break_end(session, &params),
        "voxel.scatter" => scatter(session, &params),
        "voxel.joints" => stone::apply_stone(session, &params, Stone::Joints),
        "voxel.column_relief" => stone::apply_stone(session, &params, Stone::Relief),
        "voxel.weather" => stone::apply_stone(session, &params, Stone::Weather),
        "voxel.moss" => stone::apply_stone(session, &params, Stone::Moss),
        "voxel.algae" => stone::apply_stone(session, &params, Stone::Algae),
        "voxel.vines" => stone::apply_stone(session, &params, Stone::Vines),
        "voxel.sweep" => stone::apply_stone(session, &params, Stone::Sweep),
        "voxel.trim_outline" => stone::apply_stone(session, &params, Stone::Trim),
        "voxel.deform" => organic::apply_organic(session, &params, Organic::Deform),
        "voxel.loft" => organic::apply_organic(session, &params, Organic::Loft),
        "voxel.brush" => organic::apply_organic(session, &params, Organic::Brush),
        "voxel.carve_from_views" => silhouette::carve_from_views(session, &params),
        "render.compare_silhouette" => silhouette::compare_silhouette(session, &params),
        "mesh.import" => import::apply_import(session, &params),
        "voxel.bark" => stone::apply_stone(session, &params, Stone::Bark),
        "voxel.planks" => stone::apply_stone(session, &params, Stone::Planks),
        "voxel.cavity_tint" => stone::apply_cavity(session, &params),
        "voxel.foliage" => stone::apply_stone(session, &params, Stone::Foliage),
        "marker.set" => markers::set_marker(session, &params),
        "marker.remove" => markers::remove_marker(session, &params),
        "marker.list" => Ok(markers::list_markers(session)),
        "voxel.get_box" => {
            let cells: Vec<Value> = box_cells(&session.active().cells, &params)?
                .into_iter()
                .map(|(x, y, z, palette)| json!({"x": x, "y": y, "z": z, "palette": palette}))
                .collect();
            Ok(json!({"cells": cells}))
        }
        "palette.set" => {
            let slots = palette_slots(&params)?;
            let changed = session.write_palette(&slots);
            Ok(json!({"changed": changed}))
        }
        "object.set_active" => {
            let index = req_i32(&params, "index")?;
            if index < 0 || index as usize >= session.objects.len() {
                return Err(format!(
                    "object index {index} is out of range ({} objects)",
                    session.objects.len()
                ));
            }
            session.select_object(index as usize);
            Ok(json!({"activeObject": session.selected}))
        }
        "object.add" => {
            let name = opt_str(&params, "name")?;
            let position = opt_ivec3(&params, "position")?.unwrap_or([0, 0, 0]);
            let size = opt_ivec3(&params, "size")?;
            let make_active = opt_bool(&params, "make_active")?.unwrap_or(true);
            let index = session.add_object(name.as_deref(), position, size, make_active)?;
            Ok(json!({
                "index": index,
                "name": session.objects[index].name,
                "position": position,
                "activeObject": session.selected,
            }))
        }
        "object.set" => {
            let index = object_index(&params, session)?;
            let name = opt_str(&params, "name")?;
            let position = opt_ivec3(&params, "position")?;
            let hidden = opt_bool(&params, "hidden")?;
            session.set_object(index, name.as_deref(), position, hidden)?;
            let object = &session.objects[index];
            Ok(json!({
                "index": index,
                "name": object.name,
                "position": [object.translation.0, object.translation.1, object.translation.2],
                "hidden": object.hidden,
            }))
        }
        "object.remove" => {
            let index = object_index(&params, session)?;
            let count = session.objects.len();
            session.remove_object(index)?;
            let shifted = index + 1 < count;
            let mut body = json!({
                "removed": index,
                "objectCount": session.objects.len(),
                "activeObject": session.selected,
                "indicesShifted": shifted,
            });
            if shifted {
                body["note"] = json!(format!("object indices after {index} decreased by 1"));
            }
            Ok(body)
        }
        "edit.undo" => {
            if !session.can_undo() {
                return Err("nothing to undo".into());
            }
            session.undo();
            Ok(json!({"canUndo": session.can_undo(), "canRedo": session.can_redo()}))
        }
        "edit.redo" => {
            if !session.can_redo() {
                return Err("nothing to redo".into());
            }
            session.redo();
            Ok(json!({"canUndo": session.can_undo(), "canRedo": session.can_redo()}))
        }
        "rig.add" => {
            let name = req_str(&params, "name")?;
            let parent = opt_str(&params, "parent")?;
            let pivot = req_vec3(&params, "pivot")?;
            let rotation = opt_vec4(&params, "rotation")?;
            session.add_bone(&name, parent.as_deref(), pivot, rotation)?;
            Ok(json!({"name": name, "count": session.skeleton.bones.len()}))
        }
        "rig.set" => {
            let name = req_str(&params, "name")?;
            let parent = parent_update(&params)?;
            let pivot = opt_vec3(&params, "pivot")?;
            let rotation = opt_vec4(&params, "rotation")?;
            let rename = opt_str(&params, "rename")?;
            session.set_bone(&name, parent, pivot, rotation, rename.as_deref())?;
            let current = rename.unwrap_or(name);
            Ok(json!({"name": current}))
        }
        "rig.remove" => {
            let name = req_str(&params, "name")?;
            session.remove_bone(&name)?;
            Ok(json!({"removed": name}))
        }
        "rig.bind" => {
            let index = req_i32(&params, "index")?;
            if index < 0 {
                return Err(format!(
                    "object index {index} is out of range ({} objects)",
                    session.objects.len()
                ));
            }
            let bone = bind_name(&params)?;
            session.bind_object(index as usize, bone.as_deref())?;
            Ok(json!({"index": index, "bone": bone}))
        }
        "rig.list" => Ok(bones_json(session)),
        "pose.set" => {
            let pose = render_pose(&params)?.ok_or("pose is required")?;
            session.set_pose(pose)?;
            Ok(json!({"bones": session.pose.len()}))
        }
        "pose.clear" => {
            session.clear_pose();
            Ok(json!({"bones": 0}))
        }
        "clip.add" => {
            let name = req_str(&params, "name")?;
            session.add_clip(&name)?;
            Ok(json!({"name": name}))
        }
        "clip.keys" => {
            let name = req_str(&params, "name")?;
            let bone = req_str(&params, "bone")?;
            let keys = clip_keys(&params)?;
            session.set_clip_keys(&name, &bone, keys)?;
            Ok(json!({"name": name, "bone": bone}))
        }
        "clip.list" => Ok(clips_json(session)),
        "clip.remove" => {
            let name = req_str(&params, "name")?;
            session.remove_clip(&name)?;
            Ok(json!({"name": name}))
        }
        "object.mirror" => {
            let index = req_i32(&params, "index")?;
            if index < 0 {
                return Err(format!(
                    "object index {index} is out of range ({} objects)",
                    session.objects.len()
                ));
            }
            let axis = match req_str(&params, "axis")?.as_str() {
                "x" => 0,
                "y" => 1,
                "z" => 2,
                _ => return Err("axis must be x, y, or z".into()),
            };
            let name = req_str(&params, "name")?;
            let position = opt_ivec3(&params, "position")?;
            let mirror_bones = opt_bool(&params, "mirrorBones")?.unwrap_or(false);
            let (created, bones) =
                session.mirror_object(index as usize, axis, &name, position, mirror_bones)?;
            Ok(json!({"index": created, "name": name, "bones": bones}))
        }
        "render" => render_scene(session, &params),
        "render.flat" => model_view(session, &params),
        "render.preview" => preview(session, &params),
        "view.set" => set_view(session, &params),
        "script.set" => {
            let index = script_index(session, &params)?;
            let source = req_str(&params, "source")?;
            session.set_script(index, &source)?;
            Ok(script_json(session, index))
        }
        "script.get" => {
            let index = script_index(session, &params)?;
            Ok(script_json(session, index))
        }
        "script.params" => {
            let index = script_index(session, &params)?;
            let values = params
                .get("values")
                .and_then(Value::as_object)
                .ok_or("values must be an object")?;
            let mut updates = Vec::new();
            for (name, value) in values {
                let Some(spec) = session.objects[index]
                    .script
                    .params
                    .iter()
                    .find(|param| param.name == *name)
                else {
                    return Err(format!("unknown parameter `{name}`"));
                };
                let parsed = crate::script::parse_param(&spec.value, value)
                    .map_err(|err| format!("`{name}`: {err}"))?;
                updates.push((name.clone(), parsed));
            }
            session.set_script_params(index, &updates)?;
            Ok(serde_json::json!({
                "params": session.objects[index]
                    .script
                    .params
                    .iter()
                    .map(crate::script::param_json)
                    .collect::<Vec<_>>()
            }))
        }
        "script.params.list" => {
            let index = script_index(session, &params)?;
            Ok(serde_json::json!({
                "params": session.objects[index]
                    .script
                    .params
                    .iter()
                    .map(crate::script::param_json)
                    .collect::<Vec<_>>()
            }))
        }
        "script.regenerate" => {
            let index = script_index(session, &params)?;
            let timeout = match params.get("timeoutSeconds") {
                None | Some(Value::Null) => Some(crate::script::SCRIPT_TIMEOUT),
                Some(value) => {
                    let seconds = value
                        .as_f64()
                        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
                        .ok_or("timeoutSeconds must be a number >= 0")?;
                    (seconds > 0.0).then(|| std::time::Duration::from_secs_f64(seconds))
                }
            };
            session.regenerate_script_within(index, timeout)?;
            Ok(serde_json::json!({
                "object": index,
                "voxels": session.objects[index].cells.len(),
                "objects": session.objects.len(),
            }))
        }
        "mesh.export" => export_mesh(session, &params),
        "mesh.export_batch" => export_batch(session, &params),
        "mesh.export_variants" => crate::variants::export_variants(session, &params),
        "ui.screenshot" | "ui.inspect" | "ui.click" => Err(NO_WINDOW.into()),
        "control.methods" => Ok(Value::Array(
            METHODS
                .iter()
                .map(|(method, params, description)| {
                    json!({"method": method, "params": params, "description": description})
                })
                .collect(),
        )),
        "ping" => Ok(json!("pong")),
        other => Err(format!(
            "unknown method `{other}`; `control.methods` lists them"
        )),
    }
}

fn export_mesh(session: &Session, params: &Value) -> Result<Value, String> {
    let path = req_str(params, "path")?;
    let options = mesh_options(params)?;
    let written = write_mesh(session, &path, &options)?;
    Ok(json!({
        "path": path,
        "quads": written["quads"],
        "triangles": written["triangles"],
        "merge": options.merge,
        "vertexColors": options.vertex_colors,
        "colorAtlas": options.color_atlas,
        "aoStrength": options.ao_strength,
        "aoRadius": options.ao_radius,
        "aoTarget": options.ao_target.name(),
        "structure": written["structure"],
        "markers": session.markers_json(),
    }))
}

fn export_batch(session: &Session, params: &Value) -> Result<Value, String> {
    let models = params
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| "models must be an array of 1 to 16 exports".to_string())?;
    if models.is_empty() || models.len() > 16 {
        return Err("models must contain 1 to 16 exports".into());
    }
    // Duplicate output names are a malformed batch, not independent per-model failures.
    // Check the complete batch before an export can replace an existing file.
    let mut outputs = HashSet::new();
    let mut existing: Vec<PathBuf> = Vec::new();
    for model in models {
        if let Some(path) = model
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| path.ends_with(".glb"))
        {
            let key = export_destination_key(Path::new(path))?;
            let mut aliases_existing = false;
            if Path::new(path).is_file() {
                for previous in &existing {
                    aliases_existing |= same_file::is_same_file(path, previous)
                        .map_err(|error| format!("Cannot compare output files: {error}"))?;
                }
                existing.push(PathBuf::from(path));
            }
            if !outputs.insert(key) || aliases_existing {
                return Err(format!("Duplicate output destination: {path}. Use distinct names, including case-only aliases."));
            }
        }
    }
    let mut written = Vec::with_capacity(models.len());
    let mut failed = 0;
    for (index, model) in models.iter().enumerate() {
        match export_model(session, model) {
            Ok(value) => written.push(value),
            Err(error) => {
                failed += 1;
                written.push(json!({"ok": false, "index": index, "error": error}));
            }
        }
    }
    Ok(json!({
        "ok": failed == 0,
        "models": written,
        "failed": failed,
    }))
}

fn export_destination_key(path: &Path) -> Result<String, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(path)
    };
    let ancestor = absolute
        .ancestors()
        .find(|part| std::fs::symlink_metadata(part).is_ok())
        .ok_or("No existing output ancestor.")?;
    let mut resolved = ancestor.canonicalize().map_err(|error| error.to_string())?;
    resolved.push(
        absolute
            .strip_prefix(ancestor)
            .map_err(|error| error.to_string())?,
    );
    let mut normalized = PathBuf::new();
    for component in resolved.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    // Portable batches must not rely on a case-sensitive output filesystem.
    Ok(normalized
        .to_string_lossy()
        .replace('\\', "/")
        .to_lowercase())
}

fn export_model(session: &Session, model: &Value) -> Result<Value, String> {
    if !model.is_object() {
        return Err("each model must be an object".into());
    }
    let path = req_str(model, "path")?;
    let options = mesh_options(model)?;
    let source = opt_str(model, "source")?;
    let (document, source) = match source {
        Some(source) => {
            if !source.ends_with(".vox") {
                return Err(format!("source must be a .vox file ({source})"));
            }
            let mut loaded = Session::new();
            loaded.open_vox(Path::new(&source))?;
            (loaded, Some(source))
        }
        None => {
            // The open document. Cloning the session would copy the undo stacks,
            // so the export reads it in place. open_vox above uses its own session.
            return finish_export(session, &path, &options, None);
        }
    };
    finish_export(&document, &path, &options, source)
}

fn finish_export(
    session: &Session,
    path: &str,
    options: &crate::export::MeshExport,
    source: Option<String>,
) -> Result<Value, String> {
    if !path.ends_with(".glb") {
        return Err(format!("path must end in .glb ({path})"));
    }
    let written = write_mesh(session, path, options)?;
    let mut body = json!({
        "ok": true,
        "path": path,
        "quads": written["quads"],
        "triangles": written["triangles"],
        "bytes": written["bytes"],
        "structure": written["structure"],
        "markers": session.markers_json(),
    });
    if let Some(source) = source {
        body["source"] = json!(source);
    }
    Ok(body)
}

fn write_mesh(
    session: &Session,
    path: &str,
    options: &crate::export::MeshExport,
) -> Result<Value, String> {
    let file = Path::new(path);
    crate::state::create_parent(file)?;
    let quads = crate::export::export_with(session, file, options)?;
    let bytes = std::fs::read(file).map_err(|err| format!("Could not read {path}: {err}"))?;
    let structure = crate::export::gltf_structure(&bytes)?;
    let triangles = crate::export::glb_triangles(&bytes)?;
    Ok(
        json!({"quads": quads, "triangles": triangles, "bytes": bytes.len(), "structure": structure}),
    )
}

/// The export fields of [`mesh_options`] for `options`, so the editor's
/// Export panel can call the batch export with its settings.
pub fn mesh_params(options: &crate::export::MeshExport) -> Value {
    json!({
        "merge": options.merge,
        "vertexColors": options.vertex_colors,
        "exportHidden": options.export_hidden,
        "splitObjects": options.split_objects,
        "colorAtlas": options.color_atlas,
        "aoStrength": options.ao_strength,
        "aoRadius": options.ao_radius,
        "aoTarget": options.ao_target.name(),
    })
}

pub(crate) fn mesh_options(params: &Value) -> Result<crate::export::MeshExport, String> {
    Ok(crate::export::MeshExport {
        merge: opt_bool(params, "merge")?.unwrap_or(false),
        vertex_colors: opt_bool(params, "vertexColors")?.unwrap_or(false),
        export_hidden: opt_bool(params, "exportHidden")?.unwrap_or(false),
        split_objects: opt_bool(params, "splitObjects")?.unwrap_or(false),
        color_atlas: opt_bool(params, "colorAtlas")?.unwrap_or(false),
        ao_strength: match params.get("aoStrength") {
            None | Some(Value::Null) => 0.0,
            Some(value) => value
                .as_f64()
                .filter(|value| (0.0..=1.0).contains(value))
                .ok_or("aoStrength must be a number from 0 to 1")?
                as f32,
        },
        ao_radius: match params.get("aoRadius") {
            None | Some(Value::Null) => crate::bridge::DEFAULT_AO_RADIUS,
            Some(value) => value
                .as_u64()
                .filter(|value| *value <= crate::export::occlusion::MAX_RADIUS as u64)
                .ok_or("aoRadius must be a whole number from 0 to 8")?
                as u32,
        },
        ao_target: match params.get("aoTarget") {
            None | Some(Value::Null) => crate::export::AoTarget::Auto,
            Some(value) => value
                .as_str()
                .and_then(crate::export::AoTarget::parse)
                .ok_or_else(|| format!("aoTarget must be {}", crate::export::AoTarget::NAMES))?,
        },
    })
}

const METHODS: &[(&str, &str, &str)] = &[
    ("document.new", "", "Empty document, default palette"),
    (
        "document.open",
        "path, detail?, fullPalette?",
        "Open a .vox, replacing the session. detail full replies like document.inspect",
    ),
    ("document.save", "path?", "Write every non-empty object. path overwrites the open file when omitted"),
    (
        "document.inspect",
        "fullPalette?",
        "Path, counts, bounds, objects, used palette slots, motion",
    ),
    ("voxel.batch", "operations", "Apply a list of {tool, arguments} editing tools in order as one undo step. Stops at the first error, names the item, and leaves the document as it was. Tools that write files are refused"),
    ("voxel.get", "x, y, z", "Palette index of one local cell. 0 is empty"),
    ("voxel.set", "x, y, z, palette", "Write one cell. One undo step"),
    ("voxel.set_many", "cells", "Write up to 200000 cells as one undo step"),
    ("voxel.fill", "x0, y0, z0, x1, y1, z1, palette, mode?", "Fill an inclusive box, at most 2000000 cells"),
    ("voxel.ellipsoid", "x0, y0, z0, x1, y1, z1, palette, mode?", "Fill an ellipsoid inside an inclusive box, at most 2000000 cells checked"),
    ("voxel.rounded", "x0, y0, z0, x1, y1, z1, palette, radius, mode?", "Fill a box with rounded edges. radius 0 is the full box"),
    ("voxel.tube", "points, radius?, curve?, palette | palettes, pattern?, period?, sectors?, seed?, mode?", "Fill a tapered tube through 2..=1024 points, at most 2000000 cells"),
    ("voxel.carve", "shape, x0..z1 | points, radius?, curve?", "Remove the cells inside a box, ellipsoid, rounded_box, or tube (sweep with palette 0 removes a cross-section along a path; trim_outline removes everything outside a surface)"),
    ("voxel.break", "point, normal, depth?, splinters? | density?, length?, thickness?, taper?, rimBias?, bark?, sapwood?, tips?, seed?", "Jagged fracture across a plane with splinters"),
    ("voxel.scatter", "x0, y0, z0, x1, y1, z1, palettes, count?, kinds?, size?, seed?, floor?, clump?, exclude?", "Drop small pieces that settle into a heap"),
    ("voxel.joints", "x0, y0, z0, x1, y1, z1, pattern, spacing?, course?, width?, depth?, dirt?, only?, exclude?, density?, seed?", "Cut joint gaps: columns, running bond courses, or cracks"),
    ("voxel.column_relief", "x0, y0, z0, x1, y1, z1, facing, palette, width?, out?, crown?, top?, lower?, depth?, exclude?, seed?", "Columns of random width standing out of a cliff face, ending at uneven heights"),
    ("voxel.weather", "x0, y0, z0, x1, y1, z1, chip?, chipSize?, pits?, pitDepth?, broken?, noise?, noiseDensity?, noiseScale?, only?, exclude?, seed?", "Chip edges and corners, dent pits, and add colour noise on exposed cells"),
    ("voxel.moss", "x0, y0, z0, x1, y1, z1, palettes, style?, amount?, noise?, faces?, facing?, band?, only?, exclude?, layer?, curtains?, length?, thickness?, size?, gap?, exposure?, drapes?, seed?", "Moss caps on tops and curtains down faces with a ragged end, or (style cushions) separate cushions where the surface faces up, with drapes from overhangs"),
    ("voxel.algae", "x0, y0, z0, x1, y1, z1, palettes, band?, amount?, noise?, faces?, facing?, only?, exclude?, layer?, seed?", "Algae band at the waterline, dense low and thinning upward in clumps"),
    ("voxel.vines", "x0, y0, z0, x1, y1, z1, palettes, amount?, length?, blooms?, bloomChance?, noise?, faces?, facing?, band?, only?, exclude?, seed?", "Vine strands hanging from top edges, with blooms"),
    ("voxel.bark", "x0, y0, z0, x1, y1, z1, axis | points, curve?, palettes, dirt?, width?, length?, depth?, cracks?, only?, exclude?, seed?", "Bark plates along an axis or a tube's curve, on round wood or across a flat plate, split by fissures cut 1 to 2 cells deep with dirt at the bottom"),
    ("voxel.planks", "x0, y0, x1, y1, z, axis, palettes, dirt?, dark?, width?, length?, gap?, thickness?, ragged?, nails?, knots?, only?, exclude?, seed?", "Separate boards over a rectangle at a height: rows of varied width, end joints staggered between neighbouring rows, gaps open down to a dirt cell, a tone per board, optional ragged ends, nails, and knots"),
    ("voxel.cavity_tint", "x0, y0, z0, x1, y1, z1, radius?, dark?, lighten?, light?, pairs?, darken?, brighten?, only?, exclude?", "Darken crevices and concave cells (and lighten ridges) by neighbour occupancy, to given or created shades"),
    ("voxel.foliage", "palettes, ellipsoids? | boxes? | from?, x0..z1?, core?, size?, gap?, flatten?, ragged?, light?, dark?, sun?, masses?, groove?, protect?, exclude?, seed?", "A clustered tree crown: separate leaf clumps over a volume, lit tops, dark undersides, and a dark core in the gaps"),
    ("voxel.sweep", "points, profile | width + height + radius?, curve?, palette | palettes, pattern?, period?, seed?, mode?", "Fill a 2D profile along a polyline or smooth curve, with twist and scale per point; palette 0 erases it"),
    ("voxel.trim_outline", "outline, inset?, curve?, x0..z1?, only?, skin?, thickness?", "Remove every solid cell outside a closed plan outline moved inward by an inset per height, optionally with a new skin along the cut"),
    ("voxel.deform", "op, x0..z1?, axis?, origin?, length?, center?, angle?, direction?, start?, end?, amplitude?, scale?, octaves?, seed?, bias?", "Bend, taper, or twist the object or a box along an axis, or displace its surface with noise; slots kept"),
    ("voxel.loft", "points, radius? | rx? + ry?, roll?, flatTop?, noise?, frequency?, curve?, caps?, palette | palettes, pattern?, period?, sectors?, seed?, mode?", "Fill a solid along a curve whose elliptical, flattened, noisy section changes per point"),
    ("voxel.brush", "op, points, radius?, strength?, falloff?, only?, palette?, vector?, spacing?", "Sculpt brush dabs along points: inflate, deflate, smooth, flatten, or grab"),
    ("voxel.carve_from_views", "views, size?, origin?, cellsPerPixel?, palette? | slots?, coverage?, shell?, smooth?, mode?", "Fill the visual hull of 2 or 3 orthographic views (front, side, top): each a PNG path or rows, with crop, cellsPerPixel or size, and a background rule"),
    ("render.compare_silhouette", "path, concept, crop?, background?, tolerance?, alphaThreshold?, flip?, align?, preset?, view?, yaw?, pitch?, orthographic?, fov?, margin?, object?, isolate?, width?, height?, lighting?, pose?", "Compare the model's preview silhouette with a concept mask: IoU, missing and excess shares, and an overlay PNG"),
    ("mesh.import", "path, size? | scale? | voxelSize?, upAxis?, rotation?, position?, anchor?, fill?, colors?, palette? | slots?, maxNewSlots?, tolerance?, mode?, newObject?, name?", "Voxelize a .glb, .gltf, or .obj into the active object or a new one, colours quantized into the palette. One undo step"),
    ("marker.set", "name, kind?, point | x0..z1 | min + max, direction?, object?", "Name a point, collision box, or connector on an object; replaces one of the same name"),
    ("marker.remove", "name | all", "Remove one marker, or all"),
    ("marker.list", "", "Markers with local, world, and glTF coordinates"),
    ("voxel.smooth", "x0, y0, z0, x1, y1, z1, iterations?, fillAt?, removeAt?", "Majority filter in a box that keeps colours"),
    ("voxel.get_box", "x0, y0, z0, x1, y1, z1", "Non-empty cells inside an inclusive box, at most 100000"),
    ("palette.set", "index, r, g, b, a?, motion?, motion_amount? | entries", "Replace one slot, or many, as one undo step"),
    ("object.set_active", "index", "Edit a different object. Voxel coordinates stay local to it"),
    ("object.add", "name?, position?, size?, make_active?", "Empty object at a world position. Active unless make_active is false"),
    ("object.set", "index, name?, position?, hidden?", "Rename, move, or hide. position is world voxels; local cells stay put"),
    ("object.remove", "index", "Remove an object and its voxels. Later indices shift. The last object stays"),
    ("object.mirror", "index, axis, name, position?, mirrorBones?", "Copy an object mirrored across x, y, or z. mirrorBones copies the bound subtree and swaps _l/_r"),
    ("edit.undo", "", "Undo the last edit"),
    ("edit.redo", "", "Redo the last undone edit"),
    (
        "render",
        "path, width?, height?, samples?, bounces?, seed?, denoise?, yaw?, pitch?, distance?, zoom?, margin?, target?",
        "Path-trace the scene to a PNG. The camera fits the framed box (target: all visible cells by default) inside the frame with margin 0.05 on each side; zoom 2 is twice as large. width and height default to 256 (1..4096, at most 4096 x 2048 pixels); give one and the other follows the model. samples defaults to 16 (1..128). bounces defaults to 4 (0..8). denoise defaults to true. yaw 30 and pitch 25 degrees frame the scene when omitted. yaw 0 sits the camera on +X looking toward -X. orthographic, views, pose, and showBones are optional.",
    ),
    (
        "render.flat",
        "path, width?, height?, view?, yaw?, pitch?, distance?, zoom?, margin?, target?, pixelsPerVoxel?, orthographic?, background?, views?, pose?, showBones?",
        "Flat model view PNG: each pixel is the palette colour of the first cell hit, with no light and no tonemap. The camera fits the framed box like render. width and height default to 512 (1..4096); give one and the other follows the model, or pixelsPerVoxel to size both. orthographic defaults to true. view is front, back, left, right, top, or bottom. background defaults to transparent.",
    ),
    (
        "render.preview",
        "path, width?, height?, view?, yaw?, pitch?, orthographic?, fov?, margin?, object?, isolate?, background?, antialias?, pose?",
        "Fast lit preview PNG: the viewport's mesh and light rasterised on the CPU, framed on the model or one object with margin (default 0.06 per side). width and height default to 1024 (1..4096). Perspective (fov 50 degrees, like the viewport) unless orthographic. object is an index or a name; isolate draws only it. background defaults to the viewport grey.",
    ),
    (
        "view.set",
        "view?, yaw?, pitch?, frame?, distance?, orthographic?, bones?",
        "Move the window camera. frame (default true) centres every object; distance multiplies the framing distance. bones false hides the viewport joints.",
    ),
    (
        "mesh.export",
        "path, merge?, vertexColors?, aoStrength?, aoRadius?",
        "Write a glTF 2.0 .glb of every object. merge defaults to false. vertexColors defaults to false (palette texture). exportHidden and splitObjects default to false. aoStrength (0..1, default 0) bakes ambient occlusion into COLOR_0; aoRadius (0..8 cells, default 3) is its hemisphere reach; aoTarget is vertex, texture (colour atlas texels, needs colorAtlas) or both. A skeleton writes a rigid skin. Clips become glTF animations.",
    ),
    (
        "mesh.export_batch",
        "models",
        "Write 1 to 16 .glb files. Each model is {path, source?, merge?, vertexColors?, exportHidden?, splitObjects?, colorAtlas?, aoStrength?, aoRadius?, aoTarget?}. source opens that .vox in a separate session, so the open document stays. The reply includes each file's node, mesh, skin and animation structure.",
    ),
    (
        "script.set",
        "source, object?",
        "Store a script on an object and read its parameters. One undo step. A script that fails to run is kept, and the voxels stay as they were. The error includes the script line.",
    ),
    (
        "script.get",
        "object?",
        "Script source, auto flag, and parameters. object defaults to the active object.",
    ),
    (
        "script.params",
        "values, object?",
        "Set parameter values by name. One undo step. Does not run the script.",
    ),
    (
        "script.params.list",
        "object?",
        "Parameters declared by the last successful run.",
    ),
    (
        "script.regenerate",
        "object?",
        "Run the script and replace the objects it writes. One undo step. A limit or runtime error leaves the document as it was and includes the script line.",
    ),
    ("rig.add", "name, pivot, parent?, rotation?", "Add a bone. rotation is [x, y, z, w]"),
    ("rig.set", "name, parent?, pivot?, rotation?, rename?", "Change a bone. parent null makes a root"),
    ("rig.remove", "name", "Delete a bone. Objects bound to it become unbound"),
    ("rig.bind", "index, bone", "Bind an object. bone null unbinds"),
    ("rig.list", "", "Bones with parent, pivot, rotation, and bound objects"),
    ("pose.set", "pose", "Preview pose. Map of bone name to quaternion [x, y, z, w] or {rotation, scale}, on top of the bind pose. One undo step"),
    ("pose.clear", "", "Clear the preview pose. One undo step"),
    ("clip.add", "name", "Add an empty animation clip. One undo step"),
    ("clip.keys", "name, bone, keys", "Replace one bone's keys. time, rotation, translation, scale. One undo step"),
    ("clip.list", "", "Named clips and their bone keys"),
    ("clip.remove", "name", "Remove a clip. One undo step"),
    (
        "ui.screenshot",
        "region?, x?, y?, width?, height?, path?",
        "PNG of the window, the viewport, or a rectangle in window points. Requires the open editor.",
    ),
    (
        "ui.inspect",
        "",
        "Controls from the previous frame, plus tool, mode, and status. Requires the open editor.",
    ),
    (
        "ui.click",
        "x, y",
        "Click a registered control or stamp the viewport. Requires the open editor.",
    ),
    ("control.methods", "", "Methods this server answers"),
    ("ping", "", "Replies \"pong\""),
];

/// Object names listed by the `document.open` summary before it gives a count.
const SUMMARY_OBJECT_NAMES: usize = 20;

/// The short `document.open` reply: counts and bounds, and the first object
/// names, without per-object rows or palette entries.
fn open_summary(session: &Session) -> Value {
    let voxels: usize = session
        .objects
        .iter()
        .map(|object| object.cells.len())
        .sum();
    let names: Vec<&str> = session
        .objects
        .iter()
        .take(SUMMARY_OBJECT_NAMES)
        .map(|object| object.name.as_str())
        .collect();
    let palette = palette_rows(session, false);
    let motion = motion_rows(session);
    json!({
        "path": session.path.as_ref().map(|p| p.display().to_string()),
        "activeObject": session.selected,
        "objectCount": session.objects.len(),
        "voxels": voxels,
        "bounds": bounds_json(session.world_bounds().into_iter().flat_map(|(min, max)| [min, max])),
        "canUndo": session.can_undo(),
        "canRedo": session.can_redo(),
        "bones": session.skeleton.bones.len(),
        "paletteSlots": palette.as_object().map_or(0, |rows| rows.len()),
        "motionSlots": motion.as_object().map_or(0, |rows| rows.len()),
        "objectNames": names,
        "moreObjects": session.objects.len().saturating_sub(SUMMARY_OBJECT_NAMES),
    })
}

fn inspect(session: &Session, params: &Value) -> Result<Value, String> {
    let full_palette = opt_bool(params, "fullPalette")?.unwrap_or(false);
    let objects: Vec<Value> = session
        .objects
        .iter()
        .enumerate()
        .map(|(index, object)| {
            json!({
                "index": index,
                "name": object.name,
                "hidden": object.hidden,
                "active": index == session.selected,
                "model": index,
                "voxels": object.cells.len(),
                "bounds": bounds_json(object.cells.keys().copied()),
                "position": [object.translation.0, object.translation.1, object.translation.2],
                "translation": [object.translation.0, object.translation.1, object.translation.2],
                "bone": object.bone,
            })
        })
        .collect();
    let voxels: usize = session
        .objects
        .iter()
        .map(|object| object.cells.len())
        .sum();
    Ok(json!({
        "path": session.path.as_ref().map(|p| p.display().to_string()),
        "activeObject": session.selected,
        "objectCount": session.objects.len(),
        "voxels": voxels,
        "bounds": bounds_json(session.world_bounds().into_iter().flat_map(|(min, max)| [min, max])),
        "activeBounds": bounds_json(session.active().cells.keys().copied()),
        "unsynced": false,
        "canUndo": session.can_undo(),
        "canRedo": session.can_redo(),
        "objects": objects,
        "bones": bone_rows(session),
        "palette": palette_rows(session, full_palette),
        "motion": motion_rows(session),
        "markers": session.markers_json(),
    }))
}

/// Palette colours keyed by index: the slots some cell uses, or every slot.
fn palette_rows(session: &Session, full: bool) -> Value {
    let mut used = [full; 256];
    for object in &session.objects {
        for slot in object.cells.values() {
            used[*slot as usize] = true;
        }
    }
    used[0] = full;
    let rows: serde_json::Map<String, Value> = (0..256)
        .filter(|&index| used[index])
        .map(|index| (index.to_string(), json!(hex_color(session.palette[index]))))
        .collect();
    Value::Object(rows)
}

/// Slots whose motion is not the default (`none` at amount 1), keyed by index.
fn motion_rows(session: &Session) -> Value {
    let rows: serde_json::Map<String, Value> = session
        .materials
        .iter()
        .enumerate()
        .filter(|(_, material)| {
            material.motion != crate::state::Motion::None || material.motion_amount != 1.0
        })
        .map(|(index, material)| {
            (
                index.to_string(),
                json!({"kind": material.motion.as_str(), "amount": short_f32(material.motion_amount)}),
            )
        })
        .collect();
    Value::Object(rows)
}

fn bounds_json(cells: impl IntoIterator<Item = crate::state::Cell>) -> Value {
    let mut iter = cells.into_iter();
    let Some(first) = iter.next() else {
        return Value::Null;
    };
    let mut min = first;
    let mut max = first;
    for cell in iter {
        min.0 = min.0.min(cell.0);
        min.1 = min.1.min(cell.1);
        min.2 = min.2.min(cell.2);
        max.0 = max.0.max(cell.0);
        max.1 = max.1.max(cell.1);
        max.2 = max.2.max(cell.2);
    }
    json!({"min": [min.0, min.1, min.2], "max": [max.0, max.1, max.2]})
}

/// The f64 that prints like the f32, so 0.3 is not 0.30000001192092896.
fn short_f32(value: f32) -> f64 {
    value.to_string().parse().unwrap_or(value as f64)
}

fn hex_color(rgba: [u8; 4]) -> String {
    format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        rgba[0], rgba[1], rgba[2], rgba[3]
    )
}

fn xyz(params: &Value) -> Result<(i32, i32, i32), String> {
    Ok((
        req_i32(params, "x")?,
        req_i32(params, "y")?,
        req_i32(params, "z")?,
    ))
}

fn render_views(params: &Value) -> Result<Vec<(f32, f32, String)>, String> {
    match params.get("views") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => {
            if items.is_empty() {
                return Err("views is empty".into());
            }
            let mut views = Vec::new();
            for item in items {
                if let Some(key) = item.as_object().and_then(|map| {
                    map.keys()
                        .find(|key| !matches!(key.as_str(), "path" | "view" | "yaw" | "pitch"))
                }) {
                    return Err(format!(
                        "a views item has no field `{key}`; it takes path, view, yaw, pitch"
                    ));
                }
                let path = req_str(item, "path")?;
                if path.is_empty() {
                    return Err("view path is empty".into());
                }
                let (yaw, pitch) = match opt_str(item, "view")? {
                    Some(name) => named_view(&name)?,
                    None => (
                        item.get("yaw")
                            .and_then(Value::as_f64)
                            .ok_or("view yaw is required")? as f32,
                        item.get("pitch")
                            .and_then(Value::as_f64)
                            .ok_or("view pitch is required")? as f32,
                    ),
                };
                if !(-90.0..=90.0).contains(&pitch) {
                    return Err("pitch is out of range".into());
                }
                views.push((yaw, pitch, path));
            }
            Ok(views)
        }
        Some(_) => Err("views must be a list".into()),
    }
}

/// Yaw and pitch in degrees of a named view of a model that faces -Y with
/// +Z up. right and left are the model's own sides.
pub fn named_view(name: &str) -> Result<(f32, f32), String> {
    match name {
        "front" => Ok((-90.0, 0.0)),
        "back" => Ok((90.0, 0.0)),
        "right" => Ok((180.0, 0.0)),
        "left" => Ok((0.0, 0.0)),
        "top" => Ok((-90.0, 90.0)),
        "bottom" => Ok((-90.0, -90.0)),
        other => Err(format!(
            "view must be front, back, left, right, top, or bottom ({other})"
        )),
    }
}

/// Top-level `view`, or `yaw` and `pitch` in degrees. Omitted angles are None.
fn view_angles(params: &Value) -> Result<(Option<f64>, Option<f64>), String> {
    let mut yaw = opt_f64(params, "yaw")?;
    let mut pitch = opt_f64(params, "pitch")?;
    if let Some(name) = opt_str(params, "view")? {
        if yaw.is_some() || pitch.is_some() {
            return Err("pass view or yaw and pitch, not both".into());
        }
        let (named_yaw, named_pitch) = named_view(&name)?;
        yaw = Some(named_yaw as f64);
        pitch = Some(named_pitch as f64);
    }
    if let Some(pitch) = pitch {
        if !(-90.0..=90.0).contains(&pitch) {
            return Err("pitch is out of range".into());
        }
    }
    Ok((yaw, pitch))
}

/// `#rrggbb`, `#rrggbbaa`, `[r, g, b]`, or `[r, g, b, a]`. Defaults to
/// transparent.
pub(crate) fn background(params: &Value) -> Result<[u8; 4], String> {
    let bad = || "background must be #rrggbb, #rrggbbaa, or [r, g, b, a?] in 0..255".to_string();
    match params.get("background") {
        None | Some(Value::Null) => Ok([0, 0, 0, 0]),
        Some(Value::String(text)) => {
            let hex = text.strip_prefix('#').unwrap_or(text);
            if !(hex.len() == 6 || hex.len() == 8) || !hex.is_ascii() {
                return Err(bad());
            }
            let mut rgba = [0, 0, 0, 255];
            for (i, channel) in rgba.iter_mut().enumerate().take(hex.len() / 2) {
                *channel = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(|_| bad())?;
            }
            Ok(rgba)
        }
        Some(Value::Array(items)) if items.len() == 3 || items.len() == 4 => {
            let mut rgba = [0, 0, 0, 255];
            for (channel, item) in rgba.iter_mut().zip(items) {
                *channel = item
                    .as_u64()
                    .filter(|value| *value <= 255)
                    .ok_or_else(bad)? as u8;
            }
            Ok(rgba)
        }
        Some(_) => Err(bad()),
    }
}

/// A pose map: each bone is a quaternion `[x, y, z, w]`, or an object with
/// optional `rotation` and `scale` `[x, y, z]`.
fn render_pose(params: &Value) -> Result<Option<crate::rig::Pose>, String> {
    match params.get("pose") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(map)) => {
            let mut pose = crate::rig::Pose::new();
            for (name, value) in map {
                let extra = match value {
                    Value::Object(_) => crate::rig::BonePose {
                        rotation: opt_vec4(value, "rotation")?
                            .unwrap_or(crate::rig::IDENTITY_ROTATION),
                        scale: opt_vec3(value, "scale")?.unwrap_or([1.0, 1.0, 1.0]),
                    },
                    _ => {
                        let rotation = as_vec(value, 4).ok_or_else(|| {
                            format!("pose `{name}` must be [x, y, z, w] or {{rotation, scale}}")
                        })?;
                        crate::rig::BonePose::rotation([
                            rotation[0],
                            rotation[1],
                            rotation[2],
                            rotation[3],
                        ])
                    }
                };
                pose.insert(name.clone(), extra);
            }
            Ok(Some(pose))
        }
        Some(_) => Err("pose must be an object of bone poses".into()),
    }
}

/// A shape of MCP `fill_box`, `fill_ellipsoid`, `fill_rounded_box`,
/// `fill_tube`, and `carve`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Sculpt {
    Box,
    Ellipsoid,
    RoundedBox,
    Tube,
}

fn sculpt_fill(session: &mut Session, params: &Value, sculpt: Sculpt) -> Result<Value, String> {
    let writes = fill_writes(&session.active().cells, params, sculpt)?;
    let changed = session.write_cells(&writes)?;
    Ok(json!({"changed": changed, "voxels": session.voxel_count()}))
}

/// The writes of a fill tool against `cells`, from its arguments: the
/// shape's cells with `palette` (or a tube's `palettes` and `pattern`), kept
/// as `mode` says. Rhai scripts call this with their options map.
pub(crate) fn fill_writes(
    cells: &HashMap<Cell, u8>,
    params: &Value,
    sculpt: Sculpt,
) -> Result<Vec<(i32, i32, i32, i32)>, String> {
    let mode = fill_mode(params)?;
    let mut writes: Vec<(i32, i32, i32, i32)> = if sculpt == Sculpt::Tube {
        tube_writes(params)?
    } else {
        let palette = req_i32(params, "palette")?;
        sculpt_cells(params, sculpt)?
            .into_iter()
            .map(|(x, y, z)| (x, y, z, palette))
            .collect()
    };
    if let Some((_, _, _, bad)) = writes.iter().find(|w| !(0..=255).contains(&w.3)) {
        return Err(format!("palette must be 0..255, not {bad}"));
    }
    if mode != FillMode::Replace {
        writes.retain(|&(x, y, z, _)| cells.contains_key(&(x, y, z)) == (mode == FillMode::Paint));
    }
    Ok(writes)
}

/// The cells of a shape from its arguments, solid or not.
fn sculpt_cells(params: &Value, sculpt: Sculpt) -> Result<Vec<Cell>, String> {
    match sculpt {
        Sculpt::Box => {
            let (x0, y0, z0, x1, y1, z1) = box_corners(params)?;
            let count =
                (i64::from(x1 - x0) + 1) * (i64::from(y1 - y0) + 1) * (i64::from(z1 - z0) + 1);
            if count > 2_000_000 {
                return Err("box is larger than 2000000 cells".into());
            }
            Ok((z0..=z1)
                .flat_map(|z| (y0..=y1).flat_map(move |y| (x0..=x1).map(move |x| (x, y, z))))
                .collect())
        }
        Sculpt::Ellipsoid => shape_cells(params, false),
        Sculpt::RoundedBox => shape_cells(params, true),
        Sculpt::Tube => Ok(tube_cells(params)?
            .into_iter()
            .map(|cell| cell.cell)
            .collect()),
    }
}

/// Cells whose centre lies inside the ellipsoid or rounded box of the
/// inclusive box in `params`.
fn shape_cells(params: &Value, rounded: bool) -> Result<Vec<Cell>, String> {
    let (x0, y0, z0, x1, y1, z1) = box_corners(params)?;
    let nx = i64::from(x1 - x0) + 1;
    let ny = i64::from(y1 - y0) + 1;
    let nz = i64::from(z1 - z0) + 1;
    let count = nx * ny * nz;
    if count > 2_000_000 {
        return Err("box is larger than 2000000 cells".into());
    }
    let radius = if rounded {
        let radius = opt_f64(params, "radius")?.ok_or("radius is required")?;
        if !radius.is_finite() || radius < 0.0 {
            return Err("radius must be finite and at least 0".into());
        }
        crate::tools::rounded_radius(radius, [nx as f64, ny as f64, nz as f64])
    } else {
        0.0
    };
    let cx = (f64::from(x0) + f64::from(x1) + 1.0) / 2.0;
    let cy = (f64::from(y0) + f64::from(y1) + 1.0) / 2.0;
    let cz = (f64::from(z0) + f64::from(z1) + 1.0) / 2.0;
    let rx = nx as f64 / 2.0;
    let ry = ny as f64 / 2.0;
    let rz = nz as f64 / 2.0;
    let mut cells = Vec::new();
    for z in z0..=z1 {
        for y in y0..=y1 {
            for x in x0..=x1 {
                let keep = if rounded {
                    rounded_inside(x, y, z, x0, y0, z0, x1, y1, z1, radius)
                } else {
                    let nx = (f64::from(x) + 0.5 - cx) / rx;
                    let ny = (f64::from(y) + 0.5 - cy) / ry;
                    let nz = (f64::from(z) + 0.5 - cz) / rz;
                    nx * nx + ny * ny + nz * nz <= 1.0 + 1.0e-9
                };
                if keep {
                    cells.push((x, y, z));
                }
            }
        }
    }
    Ok(cells)
}

/// `mode` of a fill: `replace` writes every cell of the shape, `empty` only
/// the empty ones, `paint` only the solid ones.
#[derive(Clone, Copy, PartialEq)]
enum FillMode {
    Replace,
    Empty,
    Paint,
}

fn fill_mode(params: &Value) -> Result<FillMode, String> {
    match opt_str(params, "mode")?.as_deref() {
        None | Some("replace") => Ok(FillMode::Replace),
        Some("empty") => Ok(FillMode::Empty),
        Some("paint") => Ok(FillMode::Paint),
        Some(other) => Err(format!(
            "mode must be replace, empty, or paint, not `{other}`"
        )),
    }
}

/// The tube in `params`: `points` of `[x, y, z]`, `[x, y, z, radius]`, or
/// `{x, y, z, radius?}`, the default `radius`, and `curve`.
fn tube_cells(params: &Value) -> Result<Vec<crate::tools::TubeCell>, String> {
    let list = params
        .get("points")
        .and_then(Value::as_array)
        .ok_or("points must be a list of [x, y, z] or [x, y, z, radius]")?;
    let default = opt_f64(params, "radius")?;
    let mut points = Vec::with_capacity(list.len());
    for (index, point) in list.iter().enumerate() {
        let number = |value: Option<&Value>, what: &str| -> Result<Option<f64>, String> {
            match value {
                None | Some(Value::Null) => Ok(None),
                Some(found) => found
                    .as_f64()
                    .map(Some)
                    .ok_or_else(|| format!("point {index} {what} must be a number")),
            }
        };
        let (at, radius) = if let Some(arr) = point.as_array() {
            if arr.len() != 3 && arr.len() != 4 {
                return Err(format!(
                    "point {index} must be [x, y, z] or [x, y, z, radius]"
                ));
            }
            let mut at = [0.0; 3];
            for (axis, name) in ["x", "y", "z"].iter().enumerate() {
                at[axis] = number(arr.get(axis), name)?.unwrap_or(0.0);
            }
            (at, number(arr.get(3), "radius")?)
        } else if point.is_object() {
            let mut at = [0.0; 3];
            for (axis, name) in ["x", "y", "z"].iter().enumerate() {
                at[axis] = number(point.get(*name), name)?
                    .ok_or_else(|| format!("point {index} is missing {name}"))?;
            }
            (at, number(point.get("radius"), "radius")?)
        } else {
            return Err(format!(
                "point {index} must be [x, y, z], [x, y, z, radius], or {{x, y, z, radius}}"
            ));
        };
        let radius = radius
            .or(default)
            .ok_or_else(|| format!("point {index} has no radius; give it one or pass radius"))?;
        points.push(crate::tools::TubePoint { at, radius });
    }
    let curve = match opt_str(params, "curve")?.as_deref() {
        None | Some("polyline") => false,
        Some("smooth") => true,
        Some(other) => return Err(format!("curve must be polyline or smooth, not `{other}`")),
    };
    crate::tools::tube_shaped(&points, curve, tube_shape(params)?)
}

/// `aspect`, `roll` (degrees), and `profile` of `fill_tube` and a carved tube.
fn tube_shape(params: &Value) -> Result<crate::tools::TubeShape, String> {
    let aspect = opt_f64(params, "aspect")?.unwrap_or(1.0);
    if !(0.05..=1.0).contains(&aspect) {
        return Err(format!("aspect must be 0.05..=1, not {aspect}"));
    }
    let blade = match opt_str(params, "profile")?.as_deref() {
        None | Some("round") => false,
        Some("blade") => true,
        Some(other) => return Err(format!("profile must be round or blade, not `{other}`")),
    };
    Ok(crate::tools::TubeShape {
        aspect,
        roll: opt_f64(params, "roll")?.unwrap_or(0.0).to_radians(),
        blade,
    })
}

/// A tube's cells with `palette`, or `palettes` laid out by `pattern`.
fn tube_writes(params: &Value) -> Result<Vec<(i32, i32, i32, i32)>, String> {
    let palettes = match (params.get("palette"), params.get("palettes")) {
        (Some(_), Some(_)) => return Err("pass palette or palettes, not both".into()),
        (Some(_), None) => vec![req_i32(params, "palette")?],
        (None, Some(list)) => {
            let list = list
                .as_array()
                .ok_or("palettes must be a list of indices")?;
            if list.is_empty() || list.len() > 64 {
                return Err("palettes must hold 1..=64 indices".into());
            }
            list.iter()
                .map(|v| as_i32(v).ok_or_else(|| "palettes must be integers".to_string()))
                .collect::<Result<_, _>>()?
        }
        (None, None) => return Err("palette or palettes is required".into()),
    };
    let pattern = opt_str(params, "pattern")?.unwrap_or_else(|| "rings".into());
    let period = opt_f64(params, "period")?.unwrap_or(2.0);
    if period <= 0.0 {
        return Err("period must be more than 0".into());
    }
    let sectors = match params.get("sectors") {
        None => palettes.len() as i64,
        Some(found) => i64::from(as_i32(found).ok_or("sectors must be an integer")?),
    };
    if !(1..=256).contains(&sectors) {
        return Err("sectors must be 1..=256".into());
    }
    let seed = match params.get("seed") {
        None => 0,
        Some(found) => found
            .as_u64()
            .ok_or("seed must be a whole number of at least 0")?,
    };
    if !matches!(pattern.as_str(), "rings" | "stripes" | "random") {
        return Err(format!(
            "pattern must be rings, stripes, or random, not `{pattern}`"
        ));
    }
    let pattern = match pattern.as_str() {
        "rings" => crate::tools::TubePattern::Rings { period },
        "stripes" => crate::tools::TubePattern::Stripes {
            sectors: sectors as u32,
        },
        _ => crate::tools::TubePattern::Random { seed },
    };
    Ok(tube_cells(params)?
        .into_iter()
        .map(|cell| {
            let (x, y, z) = cell.cell;
            (x, y, z, palettes[pattern.pick(&cell, palettes.len())])
        })
        .collect())
}

fn carve(session: &mut Session, params: &Value) -> Result<Value, String> {
    let writes = carve_writes(&session.active().cells, params)?;
    let removed = session.write_cells(&writes)?;
    Ok(json!({"removed": removed, "voxels": session.voxel_count()}))
}

/// The shape named by `shape` in MCP `carve` arguments.
pub(crate) fn carve_shape(params: &Value) -> Result<Sculpt, String> {
    let shape = opt_str(params, "shape")?
        .ok_or("shape is required: box, ellipsoid, rounded_box, or tube")?;
    match shape.as_str() {
        "box" => Ok(Sculpt::Box),
        "ellipsoid" => Ok(Sculpt::Ellipsoid),
        "rounded_box" => Ok(Sculpt::RoundedBox),
        "tube" => Ok(Sculpt::Tube),
        "sweep" => Err(
            "carve has no sweep shape; to erase a cross-section along a path, call sweep with palette 0"
                .into(),
        ),
        other => Err(format!(
            "shape must be box, ellipsoid, rounded_box, or tube, not `{other}`"
        )),
    }
}

/// The erasing writes of MCP `carve` against `cells`: the solid cells of
/// the shape. Rhai scripts call this with their options map.
pub(crate) fn carve_writes(
    cells: &HashMap<Cell, u8>,
    params: &Value,
) -> Result<Vec<(i32, i32, i32, i32)>, String> {
    Ok(sculpt_cells(params, carve_shape(params)?)?
        .into_iter()
        .filter(|cell| cells.contains_key(cell))
        .map(|(x, y, z)| (x, y, z, 0))
        .collect())
}

/// One palette index, or a list of them, under `key`.
fn slot_list(params: &Value, key: &str) -> Result<Vec<u8>, String> {
    let bad = || format!("{key} must be a palette index 1..=255 or a list of them");
    let values: Vec<Value> = match params.get(key) {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(list)) => list.clone(),
        Some(one) => vec![one.clone()],
    };
    if values.len() > 64 {
        return Err(format!("{key} takes at most 64 indices"));
    }
    values
        .iter()
        .map(|v| {
            as_i32(v)
                .filter(|slot| (1..=255).contains(slot))
                .map(|slot| slot as u8)
                .ok_or_else(bad)
        })
        .collect()
}

/// MCP `exclude`: one inclusive box `[x0, y0, z0, x1, y1, z1]` or a list of
/// them, at most [`crate::tools::MAX_EXCLUDE`]. Missing is none.
pub(crate) fn exclude_boxes(params: &Value) -> Result<crate::tools::Exclude, String> {
    let bad = "exclude must be a box [x0, y0, z0, x1, y1, z1] or a list of them";
    let boxes: Vec<&Value> = match params.get("exclude") {
        None | Some(Value::Null) => return Ok(crate::tools::Exclude::default()),
        Some(Value::Array(list)) if list.iter().all(Value::is_array) => list.iter().collect(),
        Some(one @ Value::Array(_)) => vec![one],
        Some(_) => return Err(bad.into()),
    };
    if boxes.len() > crate::tools::MAX_EXCLUDE {
        return Err(format!(
            "exclude takes at most {} boxes",
            crate::tools::MAX_EXCLUDE
        ));
    }
    let mut regions = Vec::with_capacity(boxes.len());
    for found in boxes {
        let corners: Vec<i32> = found
            .as_array()
            .filter(|list| list.len() == 6)
            .and_then(|list| list.iter().map(as_i32).collect())
            .ok_or(bad)?;
        regions.push(crate::tools::Region::spanning(
            (corners[0], corners[1], corners[2]),
            (corners[3], corners[4], corners[5]),
        ));
    }
    Ok(crate::tools::Exclude(regions))
}

/// A `[min, max]` pair, or one number for both.
fn range_pair(params: &Value, key: &str, default: (f64, f64)) -> Result<(f64, f64), String> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Array(pair)) if pair.len() == 2 => {
            let lo = pair[0]
                .as_f64()
                .ok_or_else(|| format!("{key} must be numbers"))?;
            let hi = pair[1]
                .as_f64()
                .ok_or_else(|| format!("{key} must be numbers"))?;
            Ok((lo, hi))
        }
        Some(one) => one
            .as_f64()
            .map(|v| (v, v))
            .ok_or_else(|| format!("{key} must be [min, max] or a number")),
    }
}

fn break_end(session: &mut Session, params: &Value) -> Result<Value, String> {
    let broken = session.break_active(&break_spec(params)?)?;
    Ok(json!({
        "changed": broken.writes.len(),
        "removed": broken.removed,
        "recoloured": broken.recoloured,
        "splinters": broken.splinters,
        "voxels": session.voxel_count()
    }))
}

/// MCP `break_end` arguments as a spec. Rhai scripts call this too.
pub(crate) fn break_spec(params: &Value) -> Result<crate::tools::BreakSpec, String> {
    let vec = |key: &str| -> Result<[f64; 3], String> {
        let v = opt_vec3(params, key)?.ok_or_else(|| format!("{key} [x, y, z] is required"))?;
        Ok([f64::from(v[0]), f64::from(v[1]), f64::from(v[2])])
    };
    let splinters = match (params.get("splinters"), params.get("density")) {
        (Some(_), Some(_)) => return Err("pass splinters or density, not both".into()),
        (Some(count), None) => crate::tools::Splinters::Count(
            count
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or("splinters must be a whole number")?,
        ),
        (None, Some(density)) => {
            crate::tools::Splinters::Density(density.as_f64().ok_or("density must be a number")?)
        }
        (None, None) => crate::tools::Splinters::Density(0.15),
    };
    let single = |key: &str| -> Result<Option<u8>, String> {
        let list = slot_list(params, key)?;
        match list.len() {
            0 => Ok(None),
            1 => Ok(Some(list[0])),
            _ => Err(format!("{key} is one palette index")),
        }
    };
    let spec = crate::tools::BreakSpec {
        point: vec("point")?,
        normal: vec("normal")?,
        depth: opt_f64(params, "depth")?.unwrap_or(3.0),
        splinters,
        length: range_pair(params, "length", (4.0, 12.0))?,
        thickness: opt_f64(params, "thickness")?.unwrap_or(1.0),
        taper: opt_f64(params, "taper")?.unwrap_or(1.0),
        rim_bias: opt_f64(params, "rimBias")?.unwrap_or(0.5),
        bark: single("bark")?,
        sapwood: slot_list(params, "sapwood")?,
        tips: single("tips")?,
        seed: match params.get("seed") {
            None => 0,
            Some(found) => found
                .as_u64()
                .ok_or("seed must be a whole number of at least 0")?,
        },
    };
    Ok(spec)
}

fn scatter(session: &mut Session, params: &Value) -> Result<Value, String> {
    let done = session.scatter_active(&scatter_spec(params)?)?;
    Ok(json!({
        "changed": done.writes.len(),
        "pieces": done.placed,
        "skipped": done.skipped,
        "voxels": session.voxel_count()
    }))
}

/// MCP `scatter` arguments as a spec. Rhai scripts call this too.
pub(crate) fn scatter_spec(params: &Value) -> Result<crate::tools::ScatterSpec, String> {
    let (x0, y0, z0, x1, y1, z1) = box_corners(params)?;
    let kinds = match params.get("kinds") {
        None | Some(Value::Null) => crate::tools::PieceKind::ALL.to_vec(),
        Some(Value::Array(list)) => list
            .iter()
            .map(|kind| {
                kind.as_str()
                    .and_then(crate::tools::PieceKind::parse)
                    .ok_or_else(|| {
                        "kinds must be a list of shard, chip, pebble, or chunk".to_string()
                    })
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err("kinds must be a list of shard, chip, pebble, or chunk".into()),
    };
    let (lo, hi) = range_pair(params, "size", (1.0, 3.0))?;
    if lo.fract() != 0.0 || hi.fract() != 0.0 {
        return Err("size must be whole numbers".into());
    }
    let palettes = slot_list(params, "palettes")?;
    if palettes.is_empty() {
        return Err("palettes is required: a palette index or a list of them".into());
    }
    let spec = crate::tools::ScatterSpec {
        min: (x0, y0, z0),
        max: (x1, y1, z1),
        count: match params.get("count") {
            None => 20,
            Some(found) => found
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or("count must be a whole number")?,
        },
        kinds,
        size: (lo as i32, hi as i32),
        palettes,
        seed: match params.get("seed") {
            None => 0,
            Some(found) => found
                .as_u64()
                .ok_or("seed must be a whole number of at least 0")?,
        },
        floor: match params.get("floor") {
            None | Some(Value::Null) => None,
            Some(found) => Some(as_i32(found).ok_or("floor must be an integer")?),
        },
        clump: opt_f64(params, "clump")?.unwrap_or(0.5),
        exclude: exclude_boxes(params)?,
    };
    Ok(spec)
}

fn smooth(session: &mut Session, params: &Value) -> Result<Value, String> {
    let (min, max, iterations, fill_at, remove_at) = smooth_args(params)?;
    let report = session.smooth_region(min, max, iterations, fill_at, remove_at)?;
    let changed = report.writes.len();
    Ok(json!({
        "changed": changed,
        "filled": report.filled,
        "removed": report.removed,
        "iterations": report.iterations,
        "voxels": session.voxel_count()
    }))
}

/// MCP `smooth` arguments: the box, iterations, `fillAt`, and `removeAt`.
/// Rhai scripts call this too.
pub(crate) fn smooth_args(params: &Value) -> Result<(Cell, Cell, u32, i32, i32), String> {
    let (x0, y0, z0, x1, y1, z1) = box_corners(params)?;
    let whole = |key: &str, default: i32, range: std::ops::RangeInclusive<i32>| {
        let value = match params.get(key) {
            None => default,
            Some(found) => as_i32(found).ok_or_else(|| format!("{key} must be an integer"))?,
        };
        if !range.contains(&value) {
            return Err(format!(
                "{key} must be {}..={}, not {value}",
                range.start(),
                range.end()
            ));
        }
        Ok::<i32, String>(value)
    };
    let iterations = whole(
        "iterations",
        1,
        1..=crate::tools::MAX_SMOOTH_ITERATIONS as i32,
    )?;
    let fill_at = whole("fillAt", 17, 1..=27)?;
    let remove_at = whole("removeAt", 9, -1..=25)?;
    Ok((
        (x0, y0, z0),
        (x1, y1, z1),
        iterations as u32,
        fill_at,
        remove_at,
    ))
}

fn rounded_inside(
    x: i32,
    y: i32,
    z: i32,
    x0: i32,
    y0: i32,
    z0: i32,
    x1: i32,
    y1: i32,
    z1: i32,
    radius: f64,
) -> bool {
    let point = [f64::from(x) + 0.5, f64::from(y) + 0.5, f64::from(z) + 0.5];
    let min = [f64::from(x0), f64::from(y0), f64::from(z0)];
    let max = [
        f64::from(x1) + 1.0,
        f64::from(y1) + 1.0,
        f64::from(z1) + 1.0,
    ];
    crate::tools::rounded_inside(point, min, max, radius)
}

fn clip_keys(params: &Value) -> Result<Vec<crate::rig::ClipKey>, String> {
    let keys = params
        .get("keys")
        .and_then(Value::as_array)
        .ok_or("keys must be a list")?;
    let mut out = Vec::new();
    for key in keys {
        let time = key
            .get("time")
            .and_then(Value::as_f64)
            .ok_or("key time is required")? as f32;
        let rotation = opt_vec4(key, "rotation")?;
        let translation = opt_vec3(key, "translation")?;
        let scale = opt_vec3(key, "scale")?.unwrap_or([1.0, 1.0, 1.0]);
        out.push(crate::rig::ClipKey {
            time,
            rotation,
            translation,
            scale,
        });
    }
    Ok(out)
}

pub(crate) fn clips_json(session: &Session) -> Value {
    json!({
        "clips": session.clips.iter().map(|clip| json!({
            "name": clip.name,
            "bones": clip.tracks.iter().map(|track| json!({
                "bone": track.bone,
                "keys": track.keys.iter().map(|key| json!({
                    "time": key.time,
                    "rotation": key.rotation,
                    "translation": key.translation,
                    "scale": key.scale
                })).collect::<Vec<_>>()
            })).collect::<Vec<_>>()
        })).collect::<Vec<_>>()
    })
}

fn box_corners(params: &Value) -> Result<(i32, i32, i32, i32, i32, i32), String> {
    let x0 = req_i32(params, "x0")?;
    let y0 = req_i32(params, "y0")?;
    let z0 = req_i32(params, "z0")?;
    let x1 = req_i32(params, "x1")?;
    let y1 = req_i32(params, "y1")?;
    let z1 = req_i32(params, "z1")?;
    Ok((
        x0.min(x1),
        y0.min(y1),
        z0.min(z1),
        x0.max(x1),
        y0.max(y1),
        z0.max(z1),
    ))
}

/// The writes of MCP `set_voxels` from its `cells`. Rhai scripts call this.
pub(crate) fn cell_writes(params: &Value) -> Result<Vec<(i32, i32, i32, i32)>, String> {
    let cells = params
        .get("cells")
        .and_then(Value::as_array)
        .ok_or("missing array cells")?;
    if cells.len() > 200_000 {
        return Err("too many cells (limit 200000)".into());
    }
    cells.iter().map(read_cell).collect()
}

/// The solid cells of MCP `get_voxels` in the box of `params`, sorted by
/// x, then y, then z. Rhai scripts call this.
pub(crate) fn box_cells(
    cells: &HashMap<Cell, u8>,
    params: &Value,
) -> Result<Vec<(i32, i32, i32, u8)>, String> {
    let (x0, y0, z0, x1, y1, z1) = box_corners(params)?;
    let mut found: Vec<_> = cells
        .iter()
        .filter(|(&(x, y, z), _)| {
            (x0..=x1).contains(&x) && (y0..=y1).contains(&y) && (z0..=z1).contains(&z)
        })
        .map(|(&(x, y, z), &palette)| (x, y, z, palette))
        .collect();
    if found.len() > 100_000 {
        return Err("too many cells (limit 100000)".into());
    }
    found.sort_unstable();
    Ok(found)
}

fn read_cell(value: &Value) -> Result<(i32, i32, i32, i32), String> {
    if let Some(arr) = value.as_array() {
        if arr.len() != 4 {
            return Err("cell array must be [x, y, z, palette]".into());
        }
        return Ok((
            as_i32(&arr[0]).ok_or("cell x must be an integer")?,
            as_i32(&arr[1]).ok_or("cell y must be an integer")?,
            as_i32(&arr[2]).ok_or("cell z must be an integer")?,
            as_i32(&arr[3]).ok_or("cell palette must be an integer")?,
        ));
    }
    Ok((
        req_i32(value, "x")?,
        req_i32(value, "y")?,
        req_i32(value, "z")?,
        req_i32(value, "palette")?,
    ))
}

fn script_index(session: &Session, params: &Value) -> Result<usize, String> {
    match params.get("object") {
        None | Some(Value::Null) => Ok(session
            .selected
            .min(session.objects.len().saturating_sub(1))),
        Some(value) => {
            let index = value.as_i64().ok_or("object must be an integer")?;
            if index < 0 || index as usize >= session.objects.len() {
                return Err(format!(
                    "object index {index} is out of range ({} objects)",
                    session.objects.len()
                ));
            }
            Ok(index as usize)
        }
    }
}

fn script_json(session: &Session, index: usize) -> Value {
    let script = &session.objects[index].script;
    json!({
        "object": index,
        "source": script.source,
        "auto": script.auto,
        "params": script.params.iter().map(crate::script::param_json).collect::<Vec<_>>(),
    })
}

fn palette_slots(params: &Value) -> Result<Vec<crate::state::PaletteChange>, String> {
    let one = params.get("index").is_some();
    let many = params.get("entries").is_some();
    if one == many {
        return Err("pass index, r, g, b or entries, not both".into());
    }
    if one {
        return Ok(vec![one_slot(params)?]);
    }
    let entries = params
        .get("entries")
        .and_then(Value::as_array)
        .ok_or("entries must be an array")?;
    if entries.len() > 256 {
        return Err("too many palette entries (limit 256)".into());
    }
    let mut slots = Vec::with_capacity(entries.len());
    for entry in entries {
        slots.push(one_slot(entry)?);
    }
    Ok(slots)
}

fn one_slot(value: &Value) -> Result<crate::state::PaletteChange, String> {
    let index = req_i32(value, "index")?;
    if !(0..=255).contains(&index) {
        return Err("palette index must be 0..255".into());
    }
    let r = byte(value, "r")?;
    let g = byte(value, "g")?;
    let b = byte(value, "b")?;
    let a = match value.get("a") {
        None => 255,
        Some(_) => byte(value, "a")?,
    };
    let motion = match value.get("motion") {
        None => None,
        Some(found) => {
            let text = found.as_str().ok_or("motion must be none, sway, or bob")?;
            Some(crate::state::Motion::parse(text).ok_or("motion must be none, sway, or bob")?)
        }
    };
    let amount = match opt_f64(value, "motion_amount")? {
        None => None,
        Some(number) => {
            if !(0.0..=4.0).contains(&number) {
                return Err("motion_amount is out of range".into());
            }
            Some(number as f32)
        }
    };
    Ok(crate::state::PaletteChange {
        index: index as u8,
        rgba: [r, g, b, a],
        motion,
        amount,
    })
}

fn byte(value: &Value, key: &str) -> Result<u8, String> {
    let n = req_i32(value, key)?;
    u8::try_from(n).map_err(|_| format!("{key} must be 0..255"))
}

pub fn req_str(value: &Value, key: &str) -> Result<String, String> {
    opt_str(value, key)?.ok_or_else(|| format!("missing string {key}"))
}

fn opt_i32_range(value: &Value, key: &str, fallback: i32, lo: i32, hi: i32) -> Result<i32, String> {
    match value.get(key) {
        None => Ok(fallback),
        Some(found) => {
            let Some(number) = as_i32(found) else {
                return Err(format!("{key} is out of range"));
            };
            if number < lo || number > hi {
                return Err(format!("{key} is out of range"));
            }
            Ok(number)
        }
    }
}

fn opt_f64(value: &Value, key: &str) -> Result<Option<f64>, String> {
    match value.get(key) {
        None => Ok(None),
        Some(found) => {
            let Some(number) = found.as_f64() else {
                return Err(format!("{key} must be a number"));
            };
            if !number.is_finite() {
                return Err(format!("{key} must be a number"));
            }
            Ok(Some(number))
        }
    }
}

fn object_index(params: &Value, session: &Session) -> Result<usize, String> {
    let index = req_i32(params, "index")?;
    if index < 0 || index as usize >= session.objects.len() {
        return Err(format!(
            "object index {index} is out of range ({} objects)",
            session.objects.len()
        ));
    }
    Ok(index as usize)
}

fn opt_ivec3(value: &Value, key: &str) -> Result<Option<[i32; 3]>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(found) => Ok(Some(ivec3(found, key)?)),
    }
}

fn ivec3(value: &Value, key: &str) -> Result<[i32; 3], String> {
    let items = value
        .as_array()
        .ok_or_else(|| format!("{key} must be [x, y, z]"))?;
    if items.len() != 3 {
        return Err(format!("{key} must be [x, y, z]"));
    }
    let mut out = [0; 3];
    for (axis, item) in items.iter().enumerate() {
        let number = if let Some(number) = item.as_i64() {
            number
        } else if let Some(number) = item.as_f64() {
            if !number.is_finite() || number.fract() != 0.0 {
                return Err(format!("{key} must be integer voxel coordinates"));
            }
            number as i64
        } else {
            return Err(format!("{key} must be integer voxel coordinates"));
        };
        out[axis] = i32::try_from(number).map_err(|_| format!("{key} is out of range"))?;
    }
    Ok(out)
}

fn opt_bool(value: &Value, key: &str) -> Result<Option<bool>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(flag)) => Ok(Some(*flag)),
        Some(_) => Err(format!("{key} must be a boolean")),
    }
}

pub(crate) fn bones_json(session: &Session) -> Value {
    json!({"bones": bone_rows(session)})
}

fn bone_rows(session: &Session) -> Vec<Value> {
    session
        .skeleton
        .bones
        .iter()
        .map(|bone| {
            let objects: Vec<usize> = session
                .objects
                .iter()
                .enumerate()
                .filter(|(_, object)| object.bone.as_deref() == Some(bone.name.as_str()))
                .map(|(index, _)| index)
                .collect();
            json!({
                "name": bone.name,
                "parent": bone.parent,
                "pivot": bone.pivot,
                "rotation": bone.rotation,
                "objects": objects,
            })
        })
        .collect()
}

fn parent_update(value: &Value) -> Result<Option<Option<String>>, String> {
    match value.get("parent") {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(text)) => Ok(Some(Some(text.clone()))),
        Some(_) => Err("parent must be a bone name or null".into()),
    }
}

fn bind_name(value: &Value) -> Result<Option<String>, String> {
    match value.get("bone") {
        None => Err("missing bone".into()),
        Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err("bone must be a name or null".into()),
    }
}

fn req_vec3(value: &Value, key: &str) -> Result<[f32; 3], String> {
    let found = value.get(key).ok_or_else(|| format!("missing {key}"))?;
    as_vec(found, 3)
        .ok_or_else(|| format!("{key} must be [x, y, z]"))
        .map(|v| [v[0], v[1], v[2]])
}

fn opt_vec3(value: &Value, key: &str) -> Result<Option<[f32; 3]>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(found) => as_vec(found, 3)
            .map(|v| Some([v[0], v[1], v[2]]))
            .ok_or_else(|| format!("{key} must be [x, y, z]")),
    }
}

fn opt_vec2(value: &Value, key: &str) -> Result<Option<[f32; 2]>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(found) => as_vec(found, 2)
            .map(|v| Some([v[0], v[1]]))
            .ok_or_else(|| format!("{key} must be [x, y]")),
    }
}

fn opt_vec4(value: &Value, key: &str) -> Result<Option<[f32; 4]>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(found) => as_vec(found, 4)
            .map(|v| Some([v[0], v[1], v[2], v[3]]))
            .ok_or_else(|| format!("{key} must be [x, y, z, w]")),
    }
}

fn as_vec(value: &Value, len: usize) -> Option<Vec<f32>> {
    let array = value.as_array()?;
    if array.len() != len {
        return None;
    }
    array
        .iter()
        .map(|item| {
            let number = item.as_f64()?;
            let number = number as f32;
            number.is_finite().then_some(number)
        })
        .collect()
}

pub fn opt_str(value: &Value, key: &str) -> Result<Option<String>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(format!("{key} must be a string")),
    }
}

pub fn req_i32(value: &Value, key: &str) -> Result<i32, String> {
    let found = value
        .get(key)
        .ok_or_else(|| format!("missing integer {key}"))?;
    as_i32(found).ok_or_else(|| format!("{key} must be an integer"))
}

fn as_i32(value: &Value) -> Option<i32> {
    let number = value.as_i64().or_else(|| {
        let f = value.as_f64()?;
        (f.fract() == 0.0).then_some(f as i64)
    })?;
    i32::try_from(number).ok()
}

/// Crop `pixels` (row-major RGBA, `width` wide) to an inclusive-exclusive
/// pixel rectangle and encode a PNG. `None` keeps the whole image.
pub fn png_region(
    pixels: &[[u8; 4]],
    width: usize,
    height: usize,
    region: Option<(i32, i32, i32, i32)>,
) -> Result<(Vec<u8>, u32, u32), String> {
    if pixels.len() != width.saturating_mul(height) || width == 0 || height == 0 {
        return Err("screenshot is empty".into());
    }
    let (x, y, w, h) = region.unwrap_or((0, 0, width as i32, height as i32));
    if w <= 0 || h <= 0 {
        return Err("screenshot region is empty".into());
    }
    if x < 0 || y < 0 || x as usize + w as usize > width || y as usize + h as usize > height {
        return Err(format!(
            "region {x},{y} {w}x{h} is outside the {width}x{height} window"
        ));
    }
    let mut rgba = Vec::with_capacity((w as usize) * (h as usize) * 4);
    for row in y..y + h {
        let start = (row as usize) * width + x as usize;
        for pixel in &pixels[start..start + w as usize] {
            rgba.extend_from_slice(pixel);
        }
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, w as u32, h as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| format!("png: {e}"))?;
    writer
        .write_image_data(&rgba)
        .map_err(|e| format!("png: {e}"))?;
    drop(writer);
    Ok((out, w as u32, h as u32))
}

pub fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n =
            (u32::from(bytes[i]) << 16) | (u32::from(bytes[i + 1]) << 8) | u32::from(bytes[i + 2]);
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push(TABLE[(n & 63) as usize] as char);
        i += 3;
    }
    if i < bytes.len() {
        let left = bytes.len() - i;
        let b0 = u32::from(bytes[i]);
        let b1 = if left == 2 {
            u32::from(bytes[i + 1])
        } else {
            0
        };
        let n = (b0 << 16) | (b1 << 8);
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        if left == 2 {
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
            out.push('=');
        } else {
            out.push('=');
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
#[path = "../tests/remote.rs"]
mod tests;
