//! MCP `carve_from_views`, `project_paint`, `fit_to_views`, and
//! `compare_silhouette`,
//! their arguments, and their replies.
//!
//! A view or a concept is an image `path` with a `crop` and a background
//! rule, or `rows` of characters with a `legend`. Rhai scripts have no file
//! access, so they pass rows; [`hull_writes`] and [`paint_writes`] are the
//! code paths MCP and Rhai share, as is [`fit_writes`].

use std::collections::HashMap;
use std::path::Path;

use serde_json::{json, Map, Value};

use super::stone::Writes;
use super::{fill_mode, intersect_writes, opt_bool, opt_f64, opt_str, FillMode};
use crate::state::{Cell, Session};
use crate::tools::{Background, CarveSpec, Colour, Crop, Ink, Mask, Scale, Side, View};

/// MCP handler: the hull written to the active object, one undo step.
pub(super) fn carve_from_views(session: &mut Session, params: &Value) -> Result<Value, String> {
    let palette = session_palette(session);
    let (writes, mut reply) = hull_writes(&session.active().cells, &palette, params, true)?;
    let changed = session.write_cells(&writes)?;
    reply.insert("changed".into(), json!(changed));
    reply.insert("voxels".into(), json!(session.voxel_count()));
    Ok(Value::Object(reply))
}

/// The document's palette colours.
pub(crate) fn session_palette(session: &Session) -> [[u8; 3]; 256] {
    std::array::from_fn(|slot| session.color(slot as u8))
}

/// The writes of `carve_from_views` against `cells` and its reply. With
/// `files` false (Rhai) a view must pass `rows`.
pub(crate) fn hull_writes(
    cells: &HashMap<Cell, u8>,
    palette: &[[u8; 3]; 256],
    params: &Value,
    files: bool,
) -> Result<(Writes, Map<String, Value>), String> {
    let spec = carve_spec(params, files)?;
    let mode = fill_mode(params)?;
    let carved = crate::tools::carve_from_views(&spec, palette)?;
    let writes: Writes = if mode == FillMode::Intersect {
        let inside: std::collections::HashSet<Cell> =
            carved.cells.iter().map(|(cell, _)| *cell).collect();
        intersect_writes(cells, &inside)
    } else {
        carved
            .cells
            .iter()
            .filter(|(cell, _)| match mode {
                FillMode::Replace | FillMode::Intersect => true,
                FillMode::Empty => !cells.contains_key(cell),
                FillMode::Paint => cells.contains_key(cell),
            })
            .map(|(cell, slot)| (cell.0, cell.1, cell.2, i32::from(*slot)))
            .collect()
    };
    let mut reply = Map::new();
    reply.insert("cells".into(), json!(carved.cells.len()));
    reply.insert(
        "bounds".into(),
        match carved.bounds {
            Some((min, max)) => json!({"min": [min.0, min.1, min.2], "max": [max.0, max.1, max.2]}),
            None => Value::Null,
        },
    );
    reply.insert("size".into(), json!(carved.size));
    reply.insert(
        "origin".into(),
        json!([spec.origin.0, spec.origin.1, spec.origin.2]),
    );
    reply.insert(
        "views".into(),
        json!(carved
            .views
            .iter()
            .map(|view| json!({
                "view": view.side.name(),
                "grid": view.grid,
                "covered": view.covered
            }))
            .collect::<Vec<_>>()),
    );
    Ok((writes, reply))
}

fn carve_spec(params: &Value, files: bool) -> Result<CarveSpec, String> {
    let list = params
        .get("views")
        .and_then(Value::as_array)
        .ok_or("views must be a list of 2 or 3 {view, path | rows, ...}")?;
    let default_cpp = opt_f64(params, "cellsPerPixel")?;
    let mut images = HashMap::new();
    let mut views = Vec::with_capacity(list.len());
    for (i, item) in list.iter().enumerate() {
        if !item.is_object() {
            return Err(format!("view {i} must be an object"));
        }
        let side =
            Side::parse(&opt_str(item, "view")?.ok_or_else(|| format!("view {i} needs view"))?)?;
        let mask = view_mask(item, files, &mut images).map_err(|err| format!("view {i}: {err}"))?;
        let scale = match (item.get("size"), opt_f64(item, "cellsPerPixel")?) {
            (Some(size), None) if !size.is_null() => {
                let pair = size
                    .as_array()
                    .filter(|pair| pair.len() == 2)
                    .and_then(|pair| Some([pair[0].as_u64()?, pair[1].as_u64()?]))
                    .ok_or_else(|| format!("view {i} size must be [across, down] cells"))?;
                Scale::Size(pair.map(|n| n.min(u64::from(u32::MAX)) as u32))
            }
            (Some(size), Some(_)) if !size.is_null() => {
                return Err(format!("view {i}: pass size or cellsPerPixel, not both"))
            }
            (_, Some(c)) => Scale::CellsPerPixel(c as f32),
            _ => match default_cpp {
                Some(c) => Scale::CellsPerPixel(c as f32),
                None => Scale::Fit,
            },
        };
        views.push(View { side, mask, scale });
    }
    let size = box_size(params)?;
    let origin = super::opt_ivec3(params, "origin")?.unwrap_or([0, 0, 0]);
    let colour = colour_arg(params)?;
    let passes = |key: &str| -> Result<Option<u32>, String> {
        match params.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(found) => found
                .as_u64()
                .map(|n| Some(n.min(u64::from(u32::MAX)) as u32))
                .ok_or_else(|| format!("{key} must be a whole number")),
        }
    };
    Ok(CarveSpec {
        views,
        origin: (origin[0], origin[1], origin[2]),
        size,
        colour,
        coverage: opt_f64(params, "coverage")?.unwrap_or(0.5) as f32,
        shell: passes("shell")?.filter(|n| *n > 0),
        smooth: passes("smooth")?.unwrap_or(0),
    })
}

/// `size` [x, y, z] cells, each a whole number or null.
fn box_size(params: &Value) -> Result<[Option<u32>; 3], String> {
    let mut size = [None; 3];
    match params.get("size") {
        None | Some(Value::Null) => {}
        Some(Value::Array(items)) if items.len() == 3 => {
            for (axis, item) in items.iter().enumerate() {
                size[axis] = match item {
                    Value::Null => None,
                    found => Some(
                        found
                            .as_u64()
                            .ok_or("size must be [x, y, z] cells, each a whole number or null")?
                            .min(u64::from(u32::MAX)) as u32,
                    ),
                };
            }
        }
        Some(_) => return Err("size must be [x, y, z] cells".into()),
    }
    Ok(size)
}

/// Palette indices 1..255 under `key`, or `None` when it is left out.
fn slot_list(params: &Value, key: &str) -> Result<Option<Vec<u8>>, String> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(list)) => list
            .iter()
            .map(|v| {
                v.as_u64()
                    .filter(|slot| (1..=255).contains(slot))
                    .map(|slot| slot as u8)
                    .ok_or_else(|| format!("{key} must be palette indices 1..255"))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        Some(_) => Err(format!("{key} must be a list of palette indices")),
    }
}

/// `palette` (one slot) or `slots` (the slots image colours quantise to).
fn colour_arg(params: &Value) -> Result<Colour, String> {
    Ok(match params.get("palette") {
        None | Some(Value::Null) => Colour::Views {
            slots: slot_list(params, "slots")?.unwrap_or_else(|| (1..=255).collect()),
        },
        Some(found) => Colour::Slot(
            found
                .as_u64()
                .filter(|slot| (1..=255).contains(slot))
                .ok_or("palette must be 1..255")? as u8,
        ),
    })
}

/// MCP handler `project_paint`: recolour the active object's visible
/// surface from one image, one undo step.
pub(super) fn project_paint(session: &mut Session, params: &Value) -> Result<Value, String> {
    let palette = session_palette(session);
    let (writes, mut reply) = paint_writes(&session.active().cells, &palette, params, true)?;
    let changed = session.write_cells(&writes)?;
    reply.insert("changed".into(), json!(changed));
    reply.insert("voxels".into(), json!(session.voxel_count()));
    Ok(Value::Object(reply))
}

/// The writes of `project_paint` against `cells` and its reply. With
/// `files` false (Rhai) the image must be `rows`.
pub(crate) fn paint_writes(
    cells: &HashMap<Cell, u8>,
    palette: &[[u8; 3]; 256],
    params: &Value,
    files: bool,
) -> Result<(Writes, Map<String, Value>), String> {
    let spec = paint_spec(params, files)?;
    let painted = crate::tools::project_paint(cells, &spec, palette)?;
    let writes: Writes = painted
        .cells
        .iter()
        .map(|(cell, slot)| (cell.0, cell.1, cell.2, i32::from(*slot)))
        .collect();
    let mut reply = Map::new();
    reply.insert("cells".into(), json!(painted.cells.len()));
    reply.insert(
        "bounds".into(),
        match painted.bounds {
            Some((min, max)) => json!({"min": [min.0, min.1, min.2], "max": [max.0, max.1, max.2]}),
            None => Value::Null,
        },
    );
    reply.insert(
        "origin".into(),
        json!([painted.origin.0, painted.origin.1, painted.origin.2]),
    );
    reply.insert("size".into(), json!(painted.size));
    reply.insert("grid".into(), json!(painted.grid));
    reply.insert("covered".into(), json!(painted.covered));
    reply.insert("rays".into(), json!(painted.rays));
    reply.insert("steep".into(), json!(painted.steep));
    Ok((writes, reply))
}

fn paint_spec(params: &Value, files: bool) -> Result<crate::tools::PaintSpec, String> {
    use crate::tools::Toward;
    let toward = match opt_str(params, "view")? {
        Some(name) => {
            if params.get("yaw").is_some_and(|v| !v.is_null())
                || params.get("pitch").is_some_and(|v| !v.is_null())
            {
                return Err("pass view or yaw and pitch, not both".into());
            }
            Toward::Side(Side::parse(&name)?)
        }
        None => {
            let yaw = opt_f64(params, "yaw")?;
            let pitch = opt_f64(params, "pitch")?;
            if yaw.is_none() && pitch.is_none() {
                return Err(format!("needs view ({}) or yaw and pitch", Side::NAMES));
            }
            Toward::Angles {
                yaw: yaw.unwrap_or(0.0) as f32,
                pitch: pitch.unwrap_or(0.0) as f32,
            }
        }
    };
    let mask = view_mask(params, files, &mut HashMap::new())?;
    let scale = match (params.get("imageSize"), opt_f64(params, "cellsPerPixel")?) {
        (Some(found), cpp) if !found.is_null() => {
            if cpp.is_some() {
                return Err("pass imageSize or cellsPerPixel, not both".into());
            }
            let pair = found
                .as_array()
                .filter(|pair| pair.len() == 2)
                .and_then(|pair| Some([pair[0].as_u64()?, pair[1].as_u64()?]))
                .ok_or("imageSize must be [across, down] cells")?;
            Scale::Size(pair.map(|n| n.min(u64::from(u32::MAX)) as u32))
        }
        (_, Some(c)) => Scale::CellsPerPixel(c as f32),
        _ => Scale::Fit,
    };
    let origin = super::opt_ivec3(params, "origin")?.map(|o| (o[0], o[1], o[2]));
    let depth = match params.get("depth") {
        None | Some(Value::Null) => 1,
        Some(found) => found
            .as_u64()
            .ok_or("depth must be a whole number of cells")?
            .min(u64::from(u32::MAX)) as u32,
    };
    Ok(crate::tools::PaintSpec {
        mask,
        toward,
        scale,
        origin,
        size: box_size(params)?,
        colour: colour_arg(params)?,
        coverage: opt_f64(params, "coverage")?.unwrap_or(0.5) as f32,
        depth,
        normal_limit: opt_f64(params, "normalLimit")?.map(|n| n as f32),
        only: slot_list(params, "only")?,
    })
}

/// A view's or a concept's mask: `rows` and `legend`, or `path` with
/// `crop`, `background`, `tolerance`, and `alphaThreshold`. `flip`
/// mirrors it.
pub(crate) fn view_mask(
    item: &Value,
    files: bool,
    images: &mut HashMap<String, crate::sheet::Image>,
) -> Result<Mask, String> {
    let mask = match (item.get("rows"), opt_str(item, "path")?) {
        (Some(rows), None) => {
            let rows = rows
                .as_array()
                .and_then(|rows| {
                    rows.iter()
                        .map(|row| row.as_str().map(str::to_string))
                        .collect::<Option<Vec<_>>>()
                })
                .ok_or("rows must be a list of strings")?;
            crate::tools::mask_from_rows(&rows, &legend(item)?)?
        }
        (None, Some(path)) => {
            if !files {
                return Err(
                    "scripts cannot read files: pass rows and legend, or call carve_from_views over MCP"
                        .into(),
                );
            }
            if !images.contains_key(&path) {
                let image = crate::sheet::read_png(Path::new(&path))?;
                images.insert(path.clone(), image);
            }
            let image = &images[&path];
            crate::tools::mask_from_rgba(
                image.width,
                image.height,
                &image.rgba,
                crop(item)?,
                background_rule(item)?,
            )?
        }
        (Some(_), Some(_)) => return Err("pass path or rows, not both".into()),
        (None, None) => return Err("needs path (a PNG) or rows".into()),
    };
    Ok(if opt_bool(item, "flip")?.unwrap_or(false) {
        mask.flipped()
    } else {
        mask
    })
}

fn legend(item: &Value) -> Result<HashMap<char, Ink>, String> {
    let mut out = HashMap::new();
    let Some(found) = item.get("legend").filter(|v| !v.is_null()) else {
        return Ok(out);
    };
    let map = found
        .as_object()
        .ok_or("legend must map a character to a palette index or #rrggbb")?;
    for (key, value) in map {
        let mut chars = key.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return Err(format!("legend key `{key}` must be one character"));
        };
        let ink = match value {
            Value::String(_) => {
                let rgba = super::background(&json!({"background": value}))
                    .map_err(|_| format!("legend `{key}` must be 1..255 or #rrggbb"))?;
                Ink::Rgb([rgba[0], rgba[1], rgba[2]])
            }
            found => Ink::Slot(
                found
                    .as_u64()
                    .filter(|slot| (1..=255).contains(slot))
                    .ok_or_else(|| format!("legend `{key}` must be 1..255 or #rrggbb"))?
                    as u8,
            ),
        };
        out.insert(c, ink);
    }
    Ok(out)
}

fn crop(item: &Value) -> Result<Option<Crop>, String> {
    match item.get("crop") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) if items.len() == 4 => {
            let mut out = [0u32; 4];
            for (slot, item) in out.iter_mut().zip(items) {
                *slot = item
                    .as_u64()
                    .filter(|v| *v <= u64::from(u32::MAX))
                    .ok_or("crop must be [x, y, width, height] in pixels")?
                    as u32;
            }
            Ok(Some(out))
        }
        Some(_) => Err("crop must be [x, y, width, height] in pixels".into()),
    }
}

/// `background` auto (default), alpha, or a key colour, with `tolerance`
/// and `alphaThreshold`.
pub(crate) fn background_rule(item: &Value) -> Result<Background, String> {
    let tolerance = opt_f64(item, "tolerance")?.unwrap_or(40.0);
    if !(0.0..=442.0).contains(&tolerance) {
        return Err("tolerance must be 0..442".into());
    }
    let threshold = opt_f64(item, "alphaThreshold")?.unwrap_or(128.0);
    if !(1.0..=255.0).contains(&threshold) {
        return Err("alphaThreshold must be 1..255".into());
    }
    let tolerance = tolerance as f32;
    Ok(match item.get("background") {
        None | Some(Value::Null) => Background::Auto { tolerance },
        Some(Value::String(text)) if text == "auto" => Background::Auto { tolerance },
        Some(Value::String(text)) if text == "alpha" => Background::Alpha {
            threshold: threshold as u8,
        },
        Some(found) => {
            let rgba = super::background(&json!({"background": found}))
                .map_err(|_| "background must be auto, alpha, #rrggbb, or [r, g, b]".to_string())?;
            Background::Key {
                color: [rgba[0], rgba[1], rgba[2]],
                tolerance,
            }
        }
    })
}

/// MCP `compare_silhouette`: write the overlay PNG and reply with the
/// numbers.
pub(super) fn compare_silhouette(session: &mut Session, params: &Value) -> Result<Value, String> {
    let path = super::req_str(params, "path")?;
    if path.is_empty() {
        return Err("path is empty".into());
    }
    let concept_path = opt_str(params, "concept")?.ok_or("concept must be a PNG path")?;
    let mut item = params.clone();
    item["path"] = json!(concept_path);
    if let Some(map) = item.as_object_mut() {
        map.remove("rows");
    }
    let concept =
        view_mask(&item, true, &mut HashMap::new()).map_err(|err| format!("concept: {err}"))?;
    let max = crate::preview::MAX_SIZE as i32;
    let aspect = concept.width as f32 / concept.height as f32;
    let width = super::opt_i32_range(params, "width", 0, 16, max)?;
    let height = super::opt_i32_range(params, "height", 0, 16, max)?;
    let (width, height) = match (width, height) {
        (0, 0) => (((512.0 * aspect).round() as i32).clamp(16, max), 512),
        (0, h) => (((h as f32 * aspect).round() as i32).clamp(16, max), h),
        (w, 0) => (w, ((w as f32 / aspect).round() as i32).clamp(16, max)),
        (w, h) => (w, h),
    };
    let mut preview = crate::preview::Preview {
        width: width as u32,
        height: height as u32,
        ..Default::default()
    };
    super::preview_camera(params, &mut preview)?;
    preview.object = super::preview_object(session, params)?;
    preview.isolate = opt_bool(params, "isolate")?.unwrap_or(false);
    preview.antialias = opt_bool(params, "antialias")?.unwrap_or(true);
    preview.lighting = super::preview_lighting(params)?;
    let background = preview
        .lighting
        .as_ref()
        .and_then(|l| l.background)
        .unwrap_or(crate::preview::VIEWPORT_BACKGROUND);
    let align = align_arg(params)?;
    let pose = match super::render_pose(params)? {
        Some(pose) => Some(session.checked_pose(pose)?),
        None => None,
    };
    let saved_pose = session.pose.clone();
    if let Some(pose) = pose {
        session.pose = pose;
    }
    let compared = crate::silhouette::compare(session, &preview, &concept, align, background);
    session.pose = saved_pose;
    let compared = compared?;
    let file = Path::new(&path);
    crate::state::create_parent(file)?;
    crate::trace::write_rgba_png(file, preview.width, preview.height, &compared.rgba)?;
    let reply = compare_reply(&path, &preview, align, &compared);
    // A window shows the latest comparison over its viewport.
    session.show_silhouette_overlay(
        preview.width,
        preview.height,
        compared.rgba,
        &reply,
        file.to_path_buf(),
    );
    Ok(reply)
}

fn align_arg(params: &Value) -> Result<crate::silhouette::Align, String> {
    use crate::silhouette::Align;
    match params.get("align") {
        None | Some(Value::Null) => Ok(Align::Fit),
        Some(Value::String(text)) => match text.as_str() {
            "fit" => Ok(Align::Fit),
            "height" => Ok(Align::Height),
            "width" => Ok(Align::Width),
            other => Err(format!(
                "align must be fit, height, width, or {{scale, offset}}, not `{other}`"
            )),
        },
        Some(found @ Value::Object(_)) => {
            let scale = opt_f64(found, "scale")?.ok_or("align needs scale")?;
            let offset = super::opt_vec2(found, "offset")?.unwrap_or([0.0, 0.0]);
            Ok(Align::Given {
                scale: scale as f32,
                offset,
            })
        }
        Some(_) => Err("align must be fit, height, width, or {scale, offset}".into()),
    }
}

fn round(value: f32, places: i32) -> f64 {
    let k = 10f64.powi(places);
    (f64::from(value) * k).round() / k
}

pub(crate) fn compare_reply(
    path: &str,
    preview: &crate::preview::Preview,
    align: crate::silhouette::Align,
    compared: &crate::silhouette::Compared,
) -> Value {
    let aspect = |b: Option<[i32; 4]>| {
        b.map(|[x0, y0, x1, y1]| round((x1 - x0 + 1) as f32 / (y1 - y0 + 1) as f32, 3))
    };
    let mut reply = json!({
        "path": path,
        "width": preview.width,
        "height": preview.height,
        "yaw": preview.yaw,
        "placementYaw": preview.placement_yaw,
        "pitch": preview.pitch,
        "orthographic": preview.orthographic,
        "iou": round(compared.iou, 4),
        "missing": round(compared.missing, 4),
        "excess": round(compared.excess, 4),
        "pixels": {
            "concept": compared.concept_pixels,
            "model": compared.model_pixels,
            "overlap": compared.overlap_pixels
        },
        "align": {
            "mode": align.name(),
            "scale": round(compared.scale, 4),
            "offset": [round(compared.offset[0], 2), round(compared.offset[1], 2)]
        },
        "conceptBounds": compared.concept_bounds,
        "modelBounds": compared.model_bounds,
        "aspect": {
            "concept": aspect(compared.concept_bounds),
            "model": aspect(compared.model_bounds)
        }
    });
    if let Some(ppv) = compared.pixels_per_voxel {
        reply["pixelsPerVoxel"] = json!(ppv);
    }
    reply
}

/// MCP handler `fit_to_views`: refine the active object so its views match
/// a views sheet, one undo step. Other visible objects (unless `isolate`)
/// cover and hide but do not change; `reference` (an object index or name)
/// gives the depth target.
pub(super) fn fit_to_views(session: &mut Session, params: &Value) -> Result<Value, String> {
    let palette = session_palette(session);
    let active = session.selected;
    let shift = |from: usize| {
        let (a, b) = (
            session.objects[from].translation,
            session.objects[active].translation,
        );
        (a.0 - b.0, a.1 - b.1, a.2 - b.2)
    };
    let reference = match params.get("reference") {
        None | Some(Value::Null) => None,
        Some(Value::String(name)) => Some(crate::preview::find_object(session, name)?),
        Some(Value::Number(n)) => Some(crate::preview::find_object(
            session,
            &n.as_u64()
                .ok_or("reference must be an object index or name")?
                .to_string(),
        )?),
        Some(_) => return Err("reference must be an object index or name".into()),
    };
    if reference == Some(active) {
        return Err("reference must be another object than the active one".into());
    }
    let reference_cells: Option<Vec<Cell>> = reference.map(|index| {
        let d = shift(index);
        session.objects[index]
            .cells
            .keys()
            .map(|c| (c.0 + d.0, c.1 + d.1, c.2 + d.2))
            .collect()
    });
    let mut context = Vec::new();
    if !opt_bool(params, "isolate")?.unwrap_or(false) {
        for (index, object) in session.objects.iter().enumerate() {
            if index == active || Some(index) == reference || object.hidden {
                continue;
            }
            let d = shift(index);
            context.extend(
                object
                    .cells
                    .iter()
                    .map(|(c, slot)| ((c.0 + d.0, c.1 + d.1, c.2 + d.2), *slot)),
            );
        }
    }
    let (writes, mut reply) = fit_writes(
        &session.active().cells,
        &context,
        reference_cells.as_deref(),
        &palette,
        params,
        true,
    )?;
    let changed = session.write_cells(&writes)?;
    reply.insert("changed".into(), json!(changed));
    reply.insert("voxels".into(), json!(session.voxel_count()));
    Ok(Value::Object(reply))
}

/// The writes of `fit_to_views` against `cells` and its reply. With
/// `files` false (Rhai) a view must pass `rows`.
pub(crate) fn fit_writes(
    cells: &HashMap<Cell, u8>,
    context: &[(Cell, u8)],
    reference: Option<&[Cell]>,
    palette: &[[u8; 3]; 256],
    params: &Value,
    files: bool,
) -> Result<(Writes, Map<String, Value>), String> {
    let spec = fit_spec(params, files)?;
    let fitted = crate::tools::fit_to_views(cells, context, reference, &spec, palette)?;
    let writes: Writes = fitted
        .writes
        .iter()
        .map(|(cell, slot)| (cell.0, cell.1, cell.2, i32::from(*slot)))
        .collect();
    let score = |s: &crate::tools::ViewScore| {
        let mut out = json!({
            "iou": round(s.iou, 4),
            "missing": round(s.missing, 4),
            "excess": round(s.excess, 4),
            "colorError": round(s.color_error, 2),
            "pixels": {"target": s.target, "model": s.model, "overlap": s.overlap}
        });
        if let Some(depth) = s.depth_error {
            out["depthError"] = json!(round(depth, 3));
        }
        out
    };
    let mut reply = Map::new();
    reply.insert(
        "views".into(),
        json!(fitted
            .views
            .iter()
            .map(|view| json!({
                "view": view.side.name(),
                "grid": view.grid,
                "align": {
                    "scale": round(view.scale, 4),
                    "offset": [round(view.offset[0], 2), round(view.offset[1], 2)]
                },
                "before": score(&view.before),
                "after": score(&view.after)
            }))
            .collect::<Vec<_>>()),
    );
    let k = 1000.0;
    reply.insert(
        "energy".into(),
        json!({
            "before": (fitted.energy[0] * k).round() / k,
            "after": (fitted.energy[1] * k).round() / k
        }),
    );
    reply.insert("added".into(), json!(fitted.added));
    reply.insert("removed".into(), json!(fitted.removed));
    reply.insert("recolored".into(), json!(fitted.recolored));
    reply.insert("sweeps".into(), json!(fitted.sweeps));
    reply.insert("moves".into(), json!(fitted.moves));
    reply.insert("rounded".into(), json!(fitted.rounded));
    reply.insert(
        "palette".into(),
        json!(fitted
            .palette
            .iter()
            .map(|(slot, n)| json!({"slot": slot, "cells": n}))
            .collect::<Vec<_>>()),
    );
    reply.insert(
        "origin".into(),
        json!([fitted.origin.0, fitted.origin.1, fitted.origin.2]),
    );
    reply.insert("size".into(), json!(fitted.size));
    Ok((writes, reply))
}

fn fit_spec(params: &Value, files: bool) -> Result<crate::tools::FitSpec, String> {
    use crate::tools::{FitSpec, FitView};
    let list = params
        .get("views")
        .and_then(Value::as_array)
        .ok_or("views must be a list of 1 to 6 {view, path | rows, ...}")?;
    let default_align = align_arg(params)?;
    let mut images = HashMap::new();
    let mut views = Vec::with_capacity(list.len());
    for (i, item) in list.iter().enumerate() {
        if !item.is_object() {
            return Err(format!("view {i} must be an object"));
        }
        // Sheet-wide keys at the top level fill a view's missing ones.
        let mut item = item.clone();
        for key in ["path", "background", "tolerance", "alphaThreshold"] {
            let given = params.get(key).filter(|v| !v.is_null());
            if let (Some(value), Some(map)) = (given, item.as_object_mut()) {
                if !map.contains_key(key) && !(key == "path" && map.contains_key("rows")) {
                    map.insert(key.into(), value.clone());
                }
            }
        }
        let item = &item;
        let side =
            Side::parse(&opt_str(item, "view")?.ok_or_else(|| format!("view {i} needs view"))?)?;
        let mask = view_mask(item, files, &mut images).map_err(|err| format!("view {i}: {err}"))?;
        let align = match item.get("align") {
            None | Some(Value::Null) => default_align,
            Some(_) => align_arg(item).map_err(|err| format!("view {i}: {err}"))?,
        };
        let color = opt_bool(item, "color")?.unwrap_or(true);
        views.push(FitView {
            side,
            mask,
            align,
            color,
        });
    }
    let whole = |key: &str| -> Result<Option<u64>, String> {
        match params.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(found) => found
                .as_u64()
                .map(Some)
                .ok_or_else(|| format!("{key} must be a whole number")),
        }
    };
    let defaults = FitSpec::default();
    let weight = |key: &str, default: f32| -> Result<f32, String> {
        Ok(opt_f64(params, key)?.map_or(default, |w| w as f32))
    };
    Ok(FitSpec {
        views,
        slots: slot_list(params, "slots")?,
        coverage: opt_f64(params, "coverage")?.unwrap_or(0.5) as f32,
        iterations: whole("iterations")?
            .map_or(defaults.iterations, |n| n.min(u64::from(u32::MAX)) as u32),
        seed: whole("seed")?.unwrap_or(defaults.seed),
        margin: whole("margin")?.map(|n| n.min(u64::from(u32::MAX)) as u32),
        mask_weight: weight("maskWeight", defaults.mask_weight)?,
        color_weight: weight("colorWeight", defaults.color_weight)?,
        depth_weight: weight("depthWeight", defaults.depth_weight)?,
        smooth_weight: weight("smoothWeight", defaults.smooth_weight)?,
        shape: opt_bool(params, "shape")?.unwrap_or(true),
        recolor: opt_bool(params, "recolor")?.unwrap_or(true),
        normal_limit: opt_f64(params, "normalLimit")?.map_or(defaults.normal_limit, |n| n as f32),
        keep: slot_list(params, "keep")?.unwrap_or_default(),
        shared_scale: opt_bool(params, "sharedScale")?.unwrap_or(true),
        round_weight: weight("roundWeight", defaults.round_weight)?,
        roundness: weight("roundness", defaults.roundness)?,
        round_axes: match opt_str(params, "roundAxes")? {
            None => defaults.round_axes,
            Some(text) => {
                let mut axes = [false; 3];
                for c in text.chars() {
                    match c {
                        'x' => axes[0] = true,
                        'y' => axes[1] = true,
                        'z' => axes[2] = true,
                        _ => return Err("roundAxes must be letters of xyz, such as \"z\"".into()),
                    }
                }
                axes
            }
        },
    })
}
