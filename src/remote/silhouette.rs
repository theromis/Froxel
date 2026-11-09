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
                "covered": view.covered,
                "depth": view.depth.as_ref().map(|(cal, carved)| {
                    let mut out = calibration_reply(cal);
                    out["carved"] = json!(carved);
                    out
                })
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
        let item = &sheet_keys(params, item);
        let side =
            Side::parse(&opt_str(item, "view")?.ok_or_else(|| format!("view {i} needs view"))?)?;
        let (mask, depth) = view_mask_depth(item, params, files, &mut images)
            .map_err(|err| format!("view {i}: {err}"))?;
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
        views.push(View {
            side,
            mask,
            scale,
            depth,
        });
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
        depth_tolerance: opt_f64(params, "depthTolerance")?.unwrap_or(0.0) as f32,
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
    let mirror = super::edit::mirror_arg(cells, params)?;
    let (writes, painted, mirrored) = crate::tools::mirrored(cells, mirror, |cells| {
        let painted = crate::tools::project_paint(cells, &spec, palette)?;
        let writes: Writes = painted
            .cells
            .iter()
            .map(|(cell, slot)| (cell.0, cell.1, cell.2, i32::from(*slot)))
            .collect();
        Ok((writes, painted))
    })?;
    let mut reply = Map::new();
    if mirror.is_some() {
        reply.insert("mirrored".into(), json!(mirrored));
    }
    reply.insert("cells".into(), json!(writes.len()));
    reply.insert(
        "bounds".into(),
        if mirror.is_some() {
            super::bounds_json(writes.iter().map(|w| (w.0, w.1, w.2)))
        } else {
            match painted.bounds {
                Some((min, max)) => {
                    json!({"min": [min.0, min.1, min.2], "max": [max.0, max.1, max.2]})
                }
                None => Value::Null,
            }
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

/// MCP handler `decal`: lay one picture onto the active object's surface
/// around a point, one undo step.
pub(super) fn decal(session: &mut Session, params: &Value) -> Result<Value, String> {
    let palette = session_palette(session);
    let (writes, mut reply) = decal_writes(&session.active().cells, &palette, params, true)?;
    let changed = session.write_cells(&writes)?;
    reply.insert("changed".into(), json!(changed));
    reply.insert("voxels".into(), json!(session.voxel_count()));
    Ok(Value::Object(reply))
}

/// The writes of `decal` against `cells` and its reply. With `files` false
/// (Rhai) the picture must be `rows`.
pub(crate) fn decal_writes(
    cells: &HashMap<Cell, u8>,
    palette: &[[u8; 3]; 256],
    params: &Value,
    files: bool,
) -> Result<(Writes, Map<String, Value>), String> {
    let spec = decal_spec(params, files)?;
    let mirror = super::edit::mirror_arg(cells, params)?;
    let (writes, done, mirrored) = crate::tools::mirrored(cells, mirror, |cells| {
        let done = crate::tools::decal(cells, &spec, palette)?;
        let writes: Writes = done
            .cells
            .iter()
            .map(|(cell, slot)| (cell.0, cell.1, cell.2, i32::from(*slot)))
            .collect();
        Ok((writes, done))
    })?;
    let mut reply = Map::new();
    if mirror.is_some() {
        reply.insert("mirrored".into(), json!(mirrored));
    }
    reply.insert("cells".into(), json!(writes.len()));
    reply.insert(
        "bounds".into(),
        super::bounds_json(writes.iter().map(|w| (w.0, w.1, w.2))),
    );
    reply.insert(
        "centre".into(),
        json!([done.centre.0, done.centre.1, done.centre.2]),
    );
    reply.insert(
        "normal".into(),
        json!(done.normal.map(|c| (c * 1000.0).round() / 1000.0)),
    );
    reply.insert("grid".into(), json!(done.grid));
    reply.insert("covered".into(), json!(done.covered));
    reply.insert("missed".into(), json!(done.missed));
    reply.insert("reached".into(), json!(done.reached));
    Ok((writes, reply))
}

/// `[x, y, z]` of numbers.
fn vec3_arg(value: &Value, what: &str) -> Result<[f64; 3], String> {
    value
        .as_array()
        .filter(|items| items.len() == 3)
        .and_then(|items| Some([items[0].as_f64()?, items[1].as_f64()?, items[2].as_f64()?]))
        .ok_or_else(|| format!("{what} must be [x, y, z]"))
}

fn decal_spec(params: &Value, files: bool) -> Result<crate::tools::DecalSpec, String> {
    use crate::tools::Target;
    let target = match (params.get("point"), params.get("ray")) {
        (Some(point), None) if !point.is_null() => Target::Point(vec3_arg(point, "point")?),
        (None, Some(ray)) if !ray.is_null() => Target::Ray {
            origin: vec3_arg(ray.get("origin").unwrap_or(&Value::Null), "ray origin")?,
            direction: vec3_arg(
                ray.get("direction").unwrap_or(&Value::Null),
                "ray direction",
            )?,
        },
        (Some(_), Some(_)) => return Err("pass point or ray, not both".into()),
        _ => return Err("needs point [x, y, z] or ray {origin, direction}".into()),
    };
    let (width, height) = match params.get("size") {
        Some(Value::Array(pair)) if pair.len() == 2 => (
            pair[0]
                .as_f64()
                .ok_or("size must be cells across or [across, down]")?,
            Some(
                pair[1]
                    .as_f64()
                    .ok_or("size must be cells across or [across, down]")?,
            ),
        ),
        Some(found) if found.is_number() => (found.as_f64().unwrap_or(0.0), None),
        None | Some(Value::Null) => {
            return Err("size is required: cells across, or [across, down]".into())
        }
        Some(_) => return Err("size must be cells across or [across, down]".into()),
    };
    let up = match params.get("up") {
        None | Some(Value::Null) => [0.0, 0.0, 1.0],
        Some(found) => vec3_arg(found, "up")?,
    };
    let whole = |key: &str, default: u32| -> Result<u32, String> {
        match params.get(key) {
            None | Some(Value::Null) => Ok(default),
            Some(found) => found
                .as_u64()
                .map(|n| n.min(u64::from(u32::MAX)) as u32)
                .ok_or_else(|| format!("{key} must be a whole number of cells")),
        }
    };
    Ok(crate::tools::DecalSpec {
        mask: view_mask(params, files, &mut HashMap::new())?,
        target,
        width,
        height,
        up,
        colour: colour_arg(params)?,
        coverage: opt_f64(params, "coverage")?.unwrap_or(0.5) as f32,
        depth: whole("depth", 1)?,
        reach: opt_f64(params, "reach")?.unwrap_or(8.0),
        normal_limit: opt_f64(params, "normalLimit")?.unwrap_or(90.0),
        only: slot_list(params, "only")?,
    })
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
/// `crop`, `background`, `tolerance`, and `alphaThreshold`, cleaned by the
/// mask edits (`excludeColors`, `flood`, `mask`, `erase`, `keepLargest`).
/// `flip` mirrors it.
pub(crate) fn view_mask(
    item: &Value,
    files: bool,
    images: &mut HashMap<String, crate::sheet::Image>,
) -> Result<Mask, String> {
    view_mask_report(item, files, images).map(|(mask, _)| mask)
}

/// [`view_mask`] and what its mask edits removed (`None` without edits).
pub(crate) fn view_mask_report(
    item: &Value,
    files: bool,
    images: &mut HashMap<String, crate::sheet::Image>,
) -> Result<(Mask, Option<crate::tools::MaskEditReport>), String> {
    let (mut mask, source) = match (item.get("rows"), opt_str(item, "path")?) {
        (Some(rows), None) => {
            let rows = rows
                .as_array()
                .and_then(|rows| {
                    rows.iter()
                        .map(|row| row.as_str().map(str::to_string))
                        .collect::<Option<Vec<_>>>()
                })
                .ok_or("rows must be a list of strings")?;
            (crate::tools::mask_from_rows(&rows, &legend(item)?)?, None)
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
            let crop = crop(item)?;
            let mask = crate::tools::mask_from_rgba(
                image.width,
                image.height,
                &image.rgba,
                crop,
                background_rule(item)?,
            )?;
            (
                mask,
                Some((path, crop.unwrap_or([0, 0, image.width, image.height]))),
            )
        }
        (Some(_), Some(_)) => return Err("pass path or rows, not both".into()),
        (None, None) => return Err("needs path (a PNG) or rows".into()),
    };
    let edits = mask_edits(item, files, images, source.as_ref())?;
    let report = if edits.is_empty() {
        None
    } else {
        let source = source.as_ref().map(|(path, crop)| {
            let image = &images[path];
            crate::tools::MaskSource {
                rgba: &image.rgba,
                width: image.width,
                crop: *crop,
            }
        });
        Some(crate::tools::edit_mask(&mut mask, source.as_ref(), &edits)?)
    };
    let mask = if opt_bool(item, "flip")?.unwrap_or(false) {
        mask.flipped()
    } else {
        mask
    };
    Ok((mask, report))
}

/// The mask edits of a view or a concept: `excludeColors` (colours) with
/// `excludeTolerance` (default 40), `flood` (true, a colour step, or
/// `{tolerance}`; default step 24), `mask` (a path or `{path, crop,
/// background, tolerance, alphaThreshold, invert}`), `erase` (rectangles
/// `[x, y, width, height]` in image pixels, as `crop` is), and
/// `keepLargest`.
fn mask_edits(
    item: &Value,
    files: bool,
    images: &mut HashMap<String, crate::sheet::Image>,
    source: Option<&(String, crate::tools::Crop)>,
) -> Result<crate::tools::MaskEdits, String> {
    use crate::tools::{MaskEdits, MAX_ERASE, MAX_EXCLUDE_COLORS as MAX_EXCLUDE};
    let mut edits = MaskEdits {
        exclude_tolerance: 40.0,
        ..Default::default()
    };
    match item.get("excludeColors") {
        None | Some(Value::Null) => {}
        Some(Value::Array(colours)) => {
            if colours.len() > MAX_EXCLUDE {
                return Err(format!("excludeColors takes at most {MAX_EXCLUDE} colours"));
            }
            for colour in colours {
                let rgba = super::background(&json!({"background": colour})).map_err(|_| {
                    "excludeColors must be #rrggbb or [r, g, b] colours".to_string()
                })?;
                edits.exclude.push([rgba[0], rgba[1], rgba[2]]);
            }
        }
        Some(_) => return Err("excludeColors must be a list of #rrggbb or [r, g, b]".into()),
    }
    if let Some(tolerance) = opt_f64(item, "excludeTolerance")? {
        if !(0.0..=442.0).contains(&tolerance) {
            return Err("excludeTolerance must be 0..442".into());
        }
        edits.exclude_tolerance = tolerance as f32;
    }
    edits.flood = match item.get("flood") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => None,
        Some(Value::Bool(true)) => Some(24.0),
        Some(Value::Number(n)) => Some(n.as_f64().unwrap_or(24.0)),
        Some(found @ Value::Object(_)) => Some(opt_f64(found, "tolerance")?.unwrap_or(24.0)),
        Some(_) => return Err("flood must be true, a tolerance, or {tolerance}".into()),
    }
    .map(|step| {
        if (0.0..=442.0).contains(&step) {
            Ok(step as f32)
        } else {
            Err("flood tolerance must be 0..442".to_string())
        }
    })
    .transpose()?;
    if edits.flood.is_some() && source.is_none() {
        return Err("flood needs an image (path), not rows".into());
    }
    let mask_item = match item.get("mask") {
        None | Some(Value::Null) => None,
        Some(Value::String(path)) => Some(json!({"path": path})),
        Some(found @ Value::Object(_)) => Some(found.clone()),
        Some(_) => {
            return Err("mask must be a PNG path or {path, crop, background, invert}".into())
        }
    };
    if let Some(mut mask_item) = mask_item {
        if !files {
            return Err("scripts cannot read files: a mask needs MCP".into());
        }
        let path = opt_str(&mask_item, "path")?.ok_or("mask needs path (a PNG)")?;
        edits.invert_mask = opt_bool(&mask_item, "invert")?.unwrap_or(false);
        if !images.contains_key(&path) {
            let image =
                crate::sheet::read_png(Path::new(&path)).map_err(|err| format!("mask: {err}"))?;
            images.insert(path.clone(), image);
        }
        // A mask as large as the view's image shares its crop; any other
        // mask covers the crop whole.
        if mask_item.get("crop").is_none() {
            if let Some((source_path, crop)) = source {
                let (mw, mh) = (images[&path].width, images[&path].height);
                let (sw, sh) = (images[source_path].width, images[source_path].height);
                if (mw, mh) == (sw, sh) {
                    mask_item["crop"] = json!(crop);
                }
            }
        }
        let image = &images[&path];
        let cut = crate::tools::mask_from_rgba(
            image.width,
            image.height,
            &image.rgba,
            crop(&mask_item)?,
            background_rule(&mask_item)?,
        )
        .map_err(|err| format!("mask: {err}"))?;
        edits.mask = Some(cut);
    }
    match item.get("erase") {
        None | Some(Value::Null) => {}
        Some(Value::Array(rects)) => {
            if rects.len() > MAX_ERASE {
                return Err(format!("erase takes at most {MAX_ERASE} rectangles"));
            }
            let corner = source.map_or([0, 0], |(_, crop)| [crop[0], crop[1]]);
            for rect in rects {
                let numbers: Option<Vec<i64>> =
                    rect.as_array().filter(|r| r.len() == 4).and_then(|r| {
                        r.iter()
                            .map(|v| v.as_f64().map(|n| n.round() as i64))
                            .collect()
                    });
                let [x, y, w, h] = numbers
                    .and_then(|n| <[i64; 4]>::try_from(n).ok())
                    .ok_or("erase must be a list of [x, y, width, height] in pixels")?;
                if w < 0 || h < 0 {
                    return Err("an erase rectangle's width and height must be at least 0".into());
                }
                edits
                    .erase
                    .push([x - i64::from(corner[0]), y - i64::from(corner[1]), w, h]);
            }
        }
        Some(_) => return Err("erase must be a list of [x, y, width, height] in pixels".into()),
    }
    edits.keep_largest = opt_bool(item, "keepLargest")?.unwrap_or(false);
    Ok(edits)
}

/// The `maskEdits` of a reply: inked pixels before, removed by each edit,
/// and after.
pub(crate) fn mask_report_json(report: &crate::tools::MaskEditReport) -> Value {
    json!({
        "before": report.before,
        "excluded": report.excluded,
        "flooded": report.flooded,
        "masked": report.masked,
        "erased": report.erased,
        "dropped": report.dropped,
        "after": report.after,
    })
}

/// A view with the sheet-wide keys at the top level (`path`,
/// `background`, `tolerance`, `alphaThreshold`) filling its missing ones.
fn sheet_keys(params: &Value, item: &Value) -> Value {
    let mut item = item.clone();
    for key in [
        "path",
        "background",
        "tolerance",
        "alphaThreshold",
        "excludeColors",
        "excludeTolerance",
        "flood",
        "keepLargest",
    ] {
        let given = params.get(key).filter(|v| !v.is_null());
        if let (Some(value), Some(map)) = (given, item.as_object_mut()) {
            // Rows have no path to inherit and no background colours to flood.
            let rows = map.contains_key("rows");
            if !map.contains_key(key) && !(rows && (key == "path" || key == "flood")) {
                map.insert(key.into(), value.clone());
            }
        }
    }
    item
}

/// A view's mask and depth map. A view with `depth` and no `path` or
/// `rows` takes its silhouette from the depth map.
pub(crate) fn view_mask_depth(
    item: &Value,
    params: &Value,
    files: bool,
    images: &mut HashMap<String, crate::sheet::Image>,
) -> Result<(Mask, Option<crate::tools::Depth>), String> {
    let colour = item.get("rows").is_some_and(|v| !v.is_null())
        || item.get("path").is_some_and(|v| !v.is_null());
    let depth =
        view_depth(item, params, files, images, colour).map_err(|err| format!("depth: {err}"))?;
    let mask = match (&depth, colour) {
        (Some(depth), false) => depth.map.mask(),
        _ => view_mask(item, files, images)?,
    };
    Ok((mask, depth))
}

/// A view's `depth`: `true` (the call's `depthPath`), a PNG path, or
/// `{path | values, crop, background, tolerance, alphaThreshold, invert,
/// gamma, near, far, calibrate, back}`; `false` or left out without
/// `depthPath` is none. `crop` defaults to the view's. With a colour image
/// the map's pixels all hold values (the silhouette is the colour
/// image's) unless `background` is given; without one the map keys its
/// background as a view does.
fn view_depth(
    item: &Value,
    params: &Value,
    files: bool,
    images: &mut HashMap<String, crate::sheet::Image>,
    colour: bool,
) -> Result<Option<crate::tools::Depth>, String> {
    use crate::tools::{Calibrate, Depth, Gamma};
    let sheet = opt_str(params, "depthPath")?;
    let mut d = match item.get("depth") {
        None | Some(Value::Null) if sheet.is_none() => return Ok(None),
        None | Some(Value::Null) | Some(Value::Bool(true)) => json!({}),
        Some(Value::Bool(false)) => return Ok(None),
        Some(Value::String(path)) => json!({ "path": path }),
        Some(found @ Value::Object(_)) => found.clone(),
        Some(_) => {
            return Err("must be true, false, a PNG path, or {path | values, ...}".into());
        }
    };
    let has = |d: &Value, key: &str| d.get(key).is_some_and(|v| !v.is_null());
    if !has(&d, "path") && !has(&d, "values") {
        match &sheet {
            Some(path) => d["path"] = json!(path),
            None => return Err("needs path or values, or depthPath on the call".into()),
        }
    }
    let mut inherited = vec!["crop"];
    if !colour {
        inherited.extend(["background", "tolerance", "alphaThreshold"]);
    }
    for key in inherited {
        if !has(&d, key) && has(item, key) {
            d[key] = item[key].clone();
        }
    }
    let map = match (
        d.get("values").filter(|v| !v.is_null()),
        opt_str(&d, "path")?,
    ) {
        (Some(values), None) => {
            let rows = values
                .as_array()
                .and_then(|rows| {
                    rows.iter()
                        .map(|row| {
                            row.as_array()?
                                .iter()
                                .map(|v| match v {
                                    Value::Null => Some(None),
                                    found => found.as_f64().map(|n| Some(n as f32)),
                                })
                                .collect::<Option<Vec<_>>>()
                        })
                        .collect::<Option<Vec<_>>>()
                })
                .ok_or("values must be rows of numbers 0..1 (1 nearest) or null")?;
            crate::tools::depth_from_values(&rows)?
        }
        (None, Some(path)) => {
            if !files {
                return Err(
                    "scripts cannot read files: pass depth values, or call over MCP".into(),
                );
            }
            if !images.contains_key(&path) {
                let image = crate::sheet::read_png(Path::new(&path))?;
                images.insert(path.clone(), image);
            }
            let image = &images[&path];
            let background = match d.get("background") {
                Some(Value::String(text)) if text == "none" => None,
                None | Some(Value::Null) if colour => None,
                _ => Some(background_rule(&d)?),
            };
            crate::tools::depth_from_rgba(
                image.width,
                image.height,
                &image.rgba,
                crop(&d)?,
                background,
            )?
        }
        (Some(_), Some(_)) => return Err("pass path or values, not both".into()),
        (None, None) => unreachable!("set above"),
    };
    let map = if opt_bool(item, "flip")?.unwrap_or(false) {
        map.flipped()
    } else {
        map
    };
    let mut depth = Depth::new(map);
    depth.invert = opt_bool(&d, "invert")?.unwrap_or(false);
    depth.gamma = match d.get("gamma") {
        None | Some(Value::Null) => Gamma::Fixed(1.0),
        Some(Value::String(text)) if text == "auto" => Gamma::Auto,
        Some(found) => Gamma::Fixed(found.as_f64().ok_or("gamma must be a number or auto")? as f32),
    };
    depth.back = opt_bool(&d, "back")?.unwrap_or(false);
    depth.regions = opt_bool(&d, "regions")?.unwrap_or(false);
    if let Some(edge) = opt_f64(&d, "edge")? {
        depth.edge = edge as f32;
    }
    depth.near_far = match (opt_f64(&d, "near")?, opt_f64(&d, "far")?) {
        (Some(near), Some(far)) => Some([near as f32, far as f32]),
        (None, None) => None,
        _ => return Err("pass near and far together".into()),
    };
    if let Some(name) = opt_str(&d, "calibrate")? {
        depth.calibrate = Calibrate::parse(&name)?;
    }
    match d.get("detail") {
        None | Some(Value::Null) => {}
        Some(found) => {
            // The detail map reads as a depth map of its own, with the
            // view's crop and colour rule.
            let mut detail = match found {
                Value::String(path) => json!({ "path": path }),
                Value::Object(_) => found.clone(),
                _ => return Err("detail must be a PNG path or {path | values, ...}".into()),
            };
            if let Some(map) = detail.as_object_mut() {
                for key in ["detail", "near", "far", "calibrate", "back", "regions"] {
                    if map.remove(key).is_some() {
                        return Err(format!("detail takes no {key}: give it to the depth map"));
                    }
                }
            }
            let mut view = item.clone();
            view["depth"] = detail;
            let inner = view_depth(&view, &json!({}), files, images, colour)
                .map_err(|err| format!("detail: {err}"))?
                .expect("an object depth");
            depth.detail = Some(Box::new(inner));
        }
    }
    depth.check()?;
    Ok(Some(depth))
}

fn calibration_reply(cal: &crate::tools::Calibration) -> Value {
    let mut out = json!({
        "calibrate": cal.mode,
        "near": round(cal.near, 2),
        "far": round(cal.far, 2),
        "gamma": round(cal.gamma, 2),
        "rim": round(cal.rim, 2)
    });
    if let Some(residual) = cal.residual {
        out["pairs"] = json!(cal.pairs);
        out["residual"] = json!(round(residual, 3));
    }
    if cal.regions > 0 {
        out["regions"] = json!(cal.regions);
    }
    if let Some(residual) = cal.region_residual {
        out["regionResidual"] = json!(round(residual, 3));
    }
    out
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
    let (concept, edited) = view_mask_report(&item, true, &mut HashMap::new())
        .map_err(|err| format!("concept: {err}"))?;
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
    let mut reply = compare_reply(&path, &preview, align, &compared);
    if let Some(edited) = edited {
        reply["maskEdits"] = mask_report_json(&edited);
    }
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

/// MCP `measure_sheet`: read a views sheet and reply with its views,
/// pitches, checks, and parts. Writes no file.
pub(super) fn measure_sheet(params: &Value) -> Result<Value, String> {
    use crate::tools::{PartsSpec, SheetSpec};
    let path = super::req_str(params, "path")?;
    let mut images = HashMap::new();
    let mut item = params.clone();
    if let Some(map) = item.as_object_mut() {
        map.remove("rows");
        map.remove("flip");
        map.remove("mask");
    }
    let (mask, edited) = view_mask_report(&item, true, &mut images)?;
    let image = &images[&path];
    let [cx, cy, cw, ch] = crop(params)?.unwrap_or([0, 0, image.width, image.height]);
    let mut luma = Vec::with_capacity((cw * ch) as usize);
    for y in cy..cy + ch {
        let row = ((y * image.width + cx) * 4) as usize;
        luma.extend(crate::tools::luma_of(
            &image.rgba[row..row + (cw * 4) as usize],
        ));
    }
    let mut spec = SheetSpec::default();
    if let Some(gap) = opt_f64(params, "gap")? {
        if !(0.0..=1024.0).contains(&gap) {
            return Err("gap must be 0..1024 pixels".into());
        }
        spec.gap = gap.round() as u32;
    }
    if let Some(share) = opt_f64(params, "minShare")? {
        if !(0.0..=1.0).contains(&share) {
            return Err("minShare must be 0..1".into());
        }
        spec.min_share = share as f32;
    }
    if let Some(tolerance) = opt_f64(params, "checkTolerance")? {
        if !(0.0..=100.0).contains(&tolerance) {
            return Err("checkTolerance must be 0..100 percent".into());
        }
        spec.tolerance = tolerance as f32;
    }
    match params.get("layout") {
        None | Some(Value::Null) => {}
        Some(Value::Array(names)) if names.len() == 4 => {
            for (slot, name) in spec.layout.iter_mut().zip(names) {
                let name = name.as_str().ok_or("layout must be 4 view names")?;
                crate::tools::Side::parse(name).map_err(|err| format!("layout: {err}"))?;
                *slot = name.to_string();
            }
        }
        Some(_) => return Err("layout must be 4 view names in reading order".into()),
    }
    if let Some(range) = super::opt_vec2(params, "pitchRange")? {
        let (lo, hi) = (range[0].round(), range[1].round());
        if !(2.0..=512.0).contains(&lo) || hi < lo * 2.0 || hi > 2048.0 {
            return Err("pitchRange must be [low, high] pixels, 2 <= low, high >= 2 * low".into());
        }
        spec.pitch_range = [lo as u32, hi as u32];
    }
    spec.parts = match params.get("parts") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => None,
        Some(Value::Bool(true)) => Some(PartsSpec::default()),
        Some(found @ Value::Object(_)) => {
            let mut parts = PartsSpec::default();
            if let Some(view) = found.get("view") {
                parts.view = match view {
                    Value::String(text) => text.clone(),
                    Value::Number(n) => n.to_string(),
                    _ => return Err("parts view must be a name or an index".into()),
                };
            }
            if let Some(radius) = opt_f64(found, "radius")? {
                if !(1.0..=4096.0).contains(&radius) {
                    return Err("parts radius must be 1..4096 pixels".into());
                }
                parts.radius = Some(radius as f32);
            }
            if let Some(share) = opt_f64(found, "minShare")? {
                if !(0.0..=1.0).contains(&share) {
                    return Err("parts minShare must be 0..1".into());
                }
                parts.min_share = share as f32;
            }
            Some(parts)
        }
        Some(_) => return Err("parts must be true or {view, radius, minShare}".into()),
    };
    let sheet = crate::tools::measure_sheet(&mask, &luma, &spec)?;
    // A view's size in cubes: across by its across pitch, down by its down
    // pitch (each falling back to the view's pitch).
    let cubes = |v: &crate::tools::SheetView| -> Option<[f64; 2]> {
        let pitch = v.pitch()?;
        let px = v.pitch_x.map_or(pitch, |p| p.pitch);
        let py = v.pitch_y.map_or(pitch, |p| p.pitch);
        Some([
            round(v.crop[2] as f32 / px, 1),
            round(v.crop[3] as f32 / py, 1),
        ])
    };
    // Square cubes: the two pitches within 10% of each other.
    let square = |v: &crate::tools::SheetView| -> Option<bool> {
        match (v.pitch_x, v.pitch_y) {
            (Some(a), Some(b)) => Some((a.pitch - b.pitch).abs() <= 0.1 * a.pitch.max(b.pitch)),
            _ => None,
        }
    };
    let mut notes = Vec::new();
    let stretched: Vec<String> = sheet
        .views
        .iter()
        .enumerate()
        .filter(|(_, v)| square(v) == Some(false))
        .map(|(i, v)| v.name.clone().unwrap_or(i.to_string()))
        .collect();
    if !stretched.is_empty() {
        notes.push(format!(
            "cubes are not square in {} (pitchX and pitchY differ by more than 10%): the sheet may be stretched, so compare sizes in cubes along the same axis",
            stretched.join(", ")
        ));
    }
    if !sheet.layout {
        notes.push(format!(
            "{} views are not a 2 x 2 layout: no names and no checks",
            sheet.views.len()
        ));
    }
    let shift = |c: [u32; 4]| [c[0] + cx, c[1] + cy, c[2], c[3]];
    let views: Vec<Value> = sheet
        .views
        .iter()
        .map(|v| {
            let pitch = v.pitch();
            json!({
                "view": v.name,
                "crop": shift(v.crop),
                "pixels": v.pixels,
                "pitch": pitch.map(|p| round(p, 2)),
                "pitchX": v.pitch_x.map(|p| round(p.pitch, 2)),
                "pitchY": v.pitch_y.map(|p| round(p.pitch, 2)),
                "strength": [v.pitch_x.map(|p| round(p.strength, 3)), v.pitch_y.map(|p| round(p.strength, 3))],
                "cubes": cubes(v),
                "square": square(v),
            })
        })
        .collect();
    let checks: Vec<Value> = sheet
        .checks
        .iter()
        .map(|c| {
            let side = |(view, axis, pixels): &(String, &str, f32), cubes: Option<f32>| {
                json!({"view": view, "axis": axis, "pixels": pixels, "cubes": cubes.map(|v| round(v, 1))})
            };
            json!({
                "check": c.name,
                "a": side(&c.a, c.cubes.map(|v| v.0)),
                "b": side(&c.b, c.cubes.map(|v| v.1)),
                "percent": round(c.percent, 1),
                "percentCubes": c.percent_cubes.map(|v| round(v, 1)),
                "ok": c.percent <= spec.tolerance
                    && c.percent_cubes.is_none_or(|v| v <= spec.tolerance),
            })
        })
        .collect();
    let mut reply = json!({
        "path": path,
        "width": image.width,
        "height": image.height,
        "layout": sheet.layout,
        "views": views,
        "checks": checks,
        "ok": sheet.layout && sheet.checks.iter().all(|c| c.percent <= spec.tolerance
            && c.percent_cubes.is_none_or(|v| v <= spec.tolerance)),
        "ignored": sheet.ignored.iter().map(|c| shift(*c)).collect::<Vec<_>>(),
        "notes": notes,
    });
    if sheet.layout {
        reply["fitViews"] = json!(sheet
            .views
            .iter()
            .map(|v| json!({"view": v.name, "crop": shift(v.crop)}))
            .collect::<Vec<_>>());
    }
    if let Some(parts) = &sheet.parts {
        reply["parts"] = json!({
            "view": parts.view,
            "count": parts.parts.len(),
            "radius": round(parts.radius, 1),
            "bodyPixels": parts.body_pixels,
            "parts": parts.parts.iter().map(|p| json!({
                "bounds": shift(p.bounds),
                "pixels": p.pixels,
                "direction": p.direction,
            })).collect::<Vec<_>>(),
        });
    }
    if let Some(edited) = edited {
        reply["maskEdits"] = mask_report_json(&edited);
    }
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
                "after": score(&view.after),
                "depth": view.depth.as_ref().map(calibration_reply)
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
        let item = &sheet_keys(params, item);
        let side =
            Side::parse(&opt_str(item, "view")?.ok_or_else(|| format!("view {i} needs view"))?)?;
        let (mask, depth) = view_mask_depth(item, params, files, &mut images)
            .map_err(|err| format!("view {i}: {err}"))?;
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
            depth,
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
        depth_tolerance: opt_f64(params, "depthTolerance")?.unwrap_or(0.0) as f32,
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

/// The spec of MCP `orient_to_views` (and `import_mesh` `orient`): `views`
/// as `fit_to_views` takes them (plus `weight`), `turns`, `yawStep`,
/// `yawRange`, and `resolution`.
pub(crate) fn orient_spec(params: &Value, files: bool) -> Result<crate::tools::OrientSpec, String> {
    use crate::tools::{OrientSpec, OrientView, Turns};
    let measured;
    let list = match params.get("views") {
        None | Some(Value::Null) if files && params.get("path").is_some_and(|p| p.is_string()) => {
            measured = sheet_layout(params)?;
            &measured
        }
        found => found
            .and_then(Value::as_array)
            .filter(|list| (1..=6).contains(&list.len()))
            .ok_or("views must be a list of 1 to 6 {view, path | rows, ...}, or pass only the sheet's path")?,
    };
    let mut images = HashMap::new();
    let mut views = Vec::with_capacity(list.len());
    for (i, item) in list.iter().enumerate() {
        if !item.is_object() {
            return Err(format!("view {i} must be an object"));
        }
        let item = &sheet_keys(params, item);
        let side =
            Side::parse(&opt_str(item, "view")?.ok_or_else(|| format!("view {i} needs view"))?)?;
        let mask = view_mask(item, files, &mut images).map_err(|err| format!("view {i}: {err}"))?;
        let weight = opt_f64(item, "weight")?.unwrap_or(1.0);
        if !(0.0..=100.0).contains(&weight) {
            return Err(format!("view {i}: weight must be 0..=100"));
        }
        views.push(OrientView {
            side,
            mask,
            weight: weight as f32,
        });
    }
    let turns = match opt_str(params, "turns")?.as_deref() {
        None | Some("all") => Turns::All,
        Some("upright") => Turns::Upright,
        Some(other) => return Err(format!("turns must be all or upright, not `{other}`")),
    };
    let resolution = match opt_f64(params, "resolution")? {
        None => crate::tools::DEFAULT_ORIENT_RESOLUTION,
        Some(n) if n.fract() == 0.0 && (8.0..=512.0).contains(&n) => n as u32,
        Some(_) => return Err("resolution must be a whole number 8..=512".into()),
    };
    let fine_axes = match opt_str(params, "fineAxes")? {
        None => [false, false, true],
        Some(text) => {
            let mut axes = [false; 3];
            for c in text.chars() {
                match c {
                    'x' => axes[0] = true,
                    'y' => axes[1] = true,
                    'z' => axes[2] = true,
                    _ => return Err(format!("fineAxes takes x, y, and z, not `{text}`")),
                }
            }
            axes
        }
    };
    Ok(OrientSpec {
        views,
        turns,
        fine_axes,
        yaw_step: opt_f64(params, "yawStep")?.unwrap_or(0.0),
        yaw_range: opt_f64(params, "yawRange")?.unwrap_or(45.0),
        resolution,
    })
}

/// The reply's account of an orientation: the score and per-view IoU as
/// the object was and as turned, the turn, and the best axis-aligned
/// candidates.
pub(crate) fn orient_reply(
    spec: &crate::tools::OrientSpec,
    found: &crate::tools::Oriented,
) -> Map<String, Value> {
    let scored = |c: &crate::tools::Candidate| {
        json!({
            "score": round(c.score, 4),
            "views": spec.views.iter().zip(&c.ious).map(|(view, iou)| {
                json!({"view": view.side.name(), "iou": round(*iou, 4)})
            }).collect::<Vec<_>>(),
        })
    };
    let turn = |c: &crate::tools::Candidate| {
        json!({
            "euler": crate::tools::euler_xyz(&c.turn),
            "axes": crate::tools::axis_names(&c.turn),
            "yaw": c.fine[2],
            "fine": c.fine,
            "score": round(c.score, 4),
        })
    };
    let mut reply = Map::new();
    reply.insert("before".into(), scored(&found.before));
    reply.insert("after".into(), scored(&found.best));
    let mut rotation = turn(&found.best);
    rotation["matrix"] = json!(found
        .best
        .turn
        .map(|row| row.map(|v| (v * 1e6).round() / 1e6)));
    reply.insert("rotation".into(), rotation);
    reply.insert(
        "candidates".into(),
        json!(found.ranked.iter().take(6).map(turn).collect::<Vec<_>>()),
    );
    if let Some(second) = found.ranked.get(1) {
        reply.insert(
            "margin".into(),
            json!(round(found.ranked[0].score - second.score, 4)),
        );
    }
    reply
}

/// The writes of `orient_to_views` on `cells` and its reply: the best turn,
/// and with `apply` (default) the object resampled to it, scaled to `size`
/// cells on its longest axis, `height` cells tall, or by `scale`.
pub(crate) fn orient_writes(
    cells: &HashMap<Cell, u8>,
    params: &Value,
    files: bool,
) -> Result<(Writes, Map<String, Value>), String> {
    let spec = orient_spec(params, files)?;
    let found = crate::tools::orient(cells, &spec)?;
    let mut reply = orient_reply(&spec, &found);
    let turn = found.best.turn;
    let points = crate::tools::surface_points(cells);
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for p in &points {
        let q = crate::tools::turned(&turn, *p);
        for a in 0..3 {
            lo[a] = lo[a].min(q[a]);
            hi[a] = hi[a].max(q[a]);
        }
    }
    let extent: [f64; 3] = std::array::from_fn(|a| hi[a] - lo[a] + 1.0);
    let scale = match (
        opt_f64(params, "size")?,
        opt_f64(params, "height")?,
        opt_f64(params, "scale")?,
    ) {
        (None, None, None) => 1.0,
        (Some(size), None, None) => {
            if !(1.0..=256.0).contains(&size) {
                return Err("size must be 1..=256 cells".into());
            }
            size / extent.iter().copied().fold(0.0, f64::max)
        }
        (None, Some(height), None) => {
            if !(1.0..=256.0).contains(&height) {
                return Err("height must be 1..=256 cells".into());
            }
            height / extent[2]
        }
        (None, None, Some(scale)) => scale,
        _ => return Err("pass one of size, height, and scale".into()),
    };
    if !(scale > 0.0 && scale <= 16.0) {
        return Err("the scale must be above 0 and at most 16".into());
    }
    reply.insert("scale".into(), json!(round(scale as f32, 4)));
    if !opt_bool(params, "apply")?.unwrap_or(true) {
        reply.insert("applied".into(), json!(false));
        return Ok((Vec::new(), reply));
    }
    let turned = crate::tools::resample(cells, &turn, scale)?;
    let mut writes: Writes = cells
        .keys()
        .filter(|c| !turned.contains_key(c))
        .map(|c| (c.0, c.1, c.2, 0))
        .collect();
    writes.extend(
        turned
            .iter()
            .map(|(c, slot)| (c.0, c.1, c.2, i32::from(*slot))),
    );
    reply.insert("applied".into(), json!(true));
    reply.insert("cells".into(), json!(turned.len()));
    reply.insert("bounds".into(), super::bounds_json(turned.keys().copied()));
    Ok((writes, reply))
}

/// MCP `orient_to_views`: turn (and scale) the active object to best match
/// a views sheet, one undo step.
pub(super) fn orient_to_views(session: &mut Session, params: &Value) -> Result<Value, String> {
    let (writes, mut reply) = orient_writes(&session.active().cells, params, true)?;
    let changed = session.write_cells(&writes)?;
    reply.insert("changed".into(), json!(changed));
    reply.insert("voxels".into(), json!(session.voxel_count()));
    Ok(Value::Object(reply))
}

/// The spec of `paint_from_views`. Views come from `views` (each `{view,
/// path | rows, crop, legend, flip, ...}` with the sheet-wide keys at the
/// top level), or, over MCP with a top-level `path` and no `views`, from
/// the layout `measure_sheet` finds in that sheet. `default_slots` are the
/// slots colours quantise into without `slots` or `newSlots`.
pub(crate) fn views_paint_spec(
    params: &Value,
    files: bool,
    default_slots: Vec<u8>,
) -> Result<crate::tools::PaintViewsSpec, String> {
    use crate::tools::{Blend, PaintViewsSpec, Quantize, SheetImage, ViewsFit};
    let measured;
    let list = match params.get("views") {
        Some(Value::Array(list)) => list,
        None | Some(Value::Null) if files && params.get("path").is_some_and(|p| p.is_string()) => {
            measured = sheet_layout(params)?;
            &measured
        }
        _ => {
            return Err(if files {
                "views must be a list of {view, path | rows, crop?}, or pass only path to read the sheet's layout as measure_sheet does".into()
            } else {
                "views must be a list of {view, rows, legend}".into()
            })
        }
    };
    if list.is_empty() || list.len() > 6 {
        return Err("views must hold 1 to 6 views".into());
    }
    let mut images = HashMap::new();
    let mut views = Vec::with_capacity(list.len());
    for (i, item) in list.iter().enumerate() {
        if !item.is_object() {
            return Err(format!("view {i} must be an object"));
        }
        let item = &sheet_keys(params, item);
        let side =
            Side::parse(&opt_str(item, "view")?.ok_or_else(|| format!("view {i} needs view"))?)?;
        let mask = view_mask(item, files, &mut images).map_err(|err| format!("view {i}: {err}"))?;
        views.push(SheetImage { side, mask });
    }
    let blend = match opt_str(params, "blend")?.as_deref() {
        None | Some("nearest") => Blend::Nearest {
            seam: opt_f64(params, "seam")?.unwrap_or(12.0) as f32,
        },
        Some("soft") => Blend::Soft {
            sharpness: opt_f64(params, "sharpness")?.unwrap_or(4.0) as f32,
        },
        Some(other) => return Err(format!("blend must be nearest or soft, not `{other}`")),
    };
    let fit = match opt_str(params, "fit")?.as_deref() {
        None | Some("ink") => ViewsFit::Ink,
        Some("crop") => ViewsFit::Crop,
        Some(other) => return Err(format!("fit must be ink or crop, not `{other}`")),
    };
    let quantize = match (slot_list(params, "slots")?, params.get("newSlots")) {
        (Some(_), Some(n)) if !n.is_null() => return Err("pass slots or newSlots, not both".into()),
        (Some(slots), _) => Quantize::Slots(slots),
        (None, Some(n)) if !n.is_null() => Quantize::New(
            n.as_u64()
                .ok_or("newSlots must be a whole number of slots")?
                .min(1 << 16) as usize,
        ),
        _ => {
            if default_slots.is_empty() {
                return Err(
                    "the object uses no slot to quantise into: pass slots or newSlots".into(),
                );
            }
            Quantize::Slots(default_slots)
        }
    };
    let guard = match params.get("accents") {
        Some(Value::Bool(false)) => None,
        _ => Some(accent_guard(params)?),
    };
    let corners = ["x0", "y0", "z0", "x1", "y1", "z1"];
    let region = match corners.iter().filter(|k| params.get(**k).is_some()).count() {
        0 => None,
        6 => {
            let (x0, y0, z0, x1, y1, z1) = super::box_corners(params)?;
            Some(((x0, y0, z0), (x1, y1, z1)))
        }
        _ => return Err("pass all of x0, y0, z0, x1, y1, z1 for a box, or none".into()),
    };
    Ok(PaintViewsSpec {
        views,
        blend,
        mirror: opt_bool(params, "mirror")?.unwrap_or(true),
        fit,
        reach: opt_f64(params, "reach")?.unwrap_or(3.0) as f32,
        inset: match params.get("inset") {
            // Rows have no anti-aliased edge to peel.
            None | Some(Value::Null) => {
                if list
                    .iter()
                    .all(|v| v.get("rows").is_some_and(|r| !r.is_null()))
                {
                    0
                } else {
                    2
                }
            }
            Some(found) => found
                .as_u64()
                .filter(|n| *n <= 16)
                .ok_or("inset must be 0..16 pixels")? as u32,
        },
        region,
        only: slot_list(params, "only")?,
        quantize,
        guard,
    })
}

/// The views of the sheet at `params`' `path`, as `measure_sheet` finds
/// its 2 x 2 layout: `[{view, crop}]`, read with the sheet-wide background
/// and mask keys.
pub(crate) fn sheet_layout(params: &Value) -> Result<Vec<Value>, String> {
    let mut item = json!({});
    for key in [
        "path",
        "background",
        "tolerance",
        "alphaThreshold",
        "excludeColors",
        "excludeTolerance",
        "flood",
        "keepLargest",
    ] {
        if let Some(value) = params.get(key).filter(|v| !v.is_null()) {
            item[key] = value.clone();
        }
    }
    let sheet = measure_sheet(&item)?;
    sheet
        .get("fitViews")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| {
            "measure_sheet found no 2 x 2 views layout in path: pass views with crops".into()
        })
}

/// The accent guard of `paint_from_views` and `reduce_palette`:
/// `minShare` (default 0.002), `contrast` (Oklab, default 0.12), and
/// `maxAccents` (default 4).
pub(crate) fn accent_guard(params: &Value) -> Result<crate::tools::colors::Guard, String> {
    let mut guard = crate::tools::colors::Guard::default();
    if let Some(share) = opt_f64(params, "minShare")? {
        if !(0.0..=1.0).contains(&share) {
            return Err("minShare must be 0..1".into());
        }
        guard.min_share = share as f32;
    }
    if let Some(contrast) = opt_f64(params, "contrast")? {
        if !(0.0..=1.5).contains(&contrast) {
            return Err("contrast must be 0..1.5 (an Oklab distance)".into());
        }
        guard.contrast = contrast as f32;
    }
    match params.get("maxAccents") {
        None | Some(Value::Null) => {}
        Some(found) => {
            guard.max_accents = found
                .as_u64()
                .filter(|n| *n <= 16)
                .ok_or("maxAccents must be 0..16")? as usize;
        }
    }
    Ok(guard)
}

/// The writes of `paint_from_views` against `cells` (any one frame of
/// coordinates) and its reply. New colours go into `shades.created`; the
/// caller writes them to the palette in the same undo step. With `files`
/// false (Rhai) the views must be `rows`.
pub(crate) fn views_paint_writes(
    cells: &HashMap<Cell, u8>,
    palette: &[[u8; 3]; 256],
    params: &Value,
    files: bool,
    shades: &mut crate::tools::Shades,
) -> Result<(Vec<(Cell, u8)>, Map<String, Value>), String> {
    let mut used: Vec<u8> = cells.values().copied().collect();
    used.sort_unstable();
    used.dedup();
    let spec = views_paint_spec(params, files, used)?;
    let done = crate::tools::paint_from_views(cells, &spec, palette, shades)?;
    let mut reply = Map::new();
    reply.insert("painted".into(), json!(done.cells.len()));
    reply.insert("surface".into(), json!(done.surface));
    reply.insert("unseen".into(), json!(done.unseen));
    reply.insert("blended".into(), json!(done.blended));
    reply.insert(
        "views".into(),
        json!(done
            .views
            .iter()
            .map(|v| json!({
                "view": v.side.name(),
                "mirrored": v.mirrored,
                "visible": v.visible,
                "used": v.used,
                "snapped": v.snapped,
                "missed": v.missed,
            }))
            .collect::<Vec<_>>()),
    );
    reply.insert(
        "slots".into(),
        json!(done
            .slots
            .iter()
            .map(|(slot, n)| json!({"slot": slot, "cells": n}))
            .collect::<Vec<_>>()),
    );
    reply.insert(
        "newSlots".into(),
        json!(shades
            .created
            .iter()
            .map(|s| json!({"slot": s.slot, "rgba": s.rgba}))
            .collect::<Vec<_>>()),
    );
    if matches!(spec.quantize, crate::tools::Quantize::New(_)) {
        reply.insert("accents".into(), json!(done.accents));
    }
    Ok((done.cells, reply))
}

/// MCP handler `paint_from_views`: recolour the surface of the active
/// object, or of every object in `objects` together (each hides what is
/// behind it from the others), from a whole views sheet, one undo step.
pub(super) fn paint_from_views(session: &mut Session, params: &Value) -> Result<Value, String> {
    let targets: Vec<usize> = match params.get("objects") {
        None | Some(Value::Null) => vec![session.selected],
        Some(Value::Array(list)) if !list.is_empty() => {
            let mut out = Vec::new();
            for item in list {
                let index = super::edit::object_arg(session, &json!({"o": item}), "o")?
                    .ok_or("objects must name objects by index or name")?;
                if !out.contains(&index) {
                    out.push(index);
                }
            }
            out
        }
        Some(_) => return Err("objects must be a list of object names or indices".into()),
    };
    // Everything in world cells; the box arrives in the active object's
    // local cells (space world turned it local), so it moves too.
    let active = session.active().translation;
    let mut params = params.clone();
    for (key, by) in [
        ("x0", active.0),
        ("x1", active.0),
        ("y0", active.1),
        ("y1", active.1),
        ("z0", active.2),
        ("z1", active.2),
    ] {
        if let Some(v) = params.get(key).and_then(Value::as_i64) {
            params[key] = json!(v + i64::from(by));
        }
    }
    let mut world: HashMap<Cell, u8> = HashMap::new();
    let mut owner: HashMap<Cell, usize> = HashMap::new();
    for &index in &targets {
        let object = &session.objects[index];
        let t = object.translation;
        for (cell, slot) in &object.cells {
            let w = (cell.0 + t.0, cell.1 + t.1, cell.2 + t.2);
            world.insert(w, *slot);
            owner.insert(w, index);
        }
    }
    let palette = session_palette(session);
    let mut shades = session.shades();
    let (painted, mut reply) = views_paint_writes(&world, &palette, &params, true, &mut shades)?;
    let mut per_object: HashMap<usize, Vec<(i32, i32, i32, i32)>> = HashMap::new();
    for (cell, slot) in painted {
        let index = owner[&cell];
        let t = session.objects[index].translation;
        per_object.entry(index).or_default().push((
            cell.0 - t.0,
            cell.1 - t.1,
            cell.2 - t.2,
            i32::from(slot),
        ));
    }
    let selected = session.selected;
    let changed = session.transaction(|session| {
        session.begin_edit();
        for shade in &shades.created {
            session.palette[shade.slot as usize] = shade.rgba;
            session.materials[shade.slot as usize] = session.materials[shade.from as usize];
        }
        let mut changed = 0;
        for &index in &targets {
            if let Some(writes) = per_object.get(&index) {
                session.selected = index;
                let result = session.write_cells(writes);
                session.selected = selected;
                changed += result?;
            }
        }
        session.end_edit();
        Ok(changed)
    })?;
    session.selected = selected;
    session.mesh_epoch = session.mesh_epoch.wrapping_add(1);
    reply.insert("changed".into(), json!(changed));
    reply.insert(
        "objects".into(),
        json!(targets
            .iter()
            .map(|i| session.objects[*i].name.clone())
            .collect::<Vec<_>>()),
    );
    reply.insert("voxels".into(), json!(session.voxel_count()));
    Ok(Value::Object(reply))
}
