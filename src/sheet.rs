//! Contact sheets behind MCP `preview` with `inputs` or `variants` and
//! `froxel preview` with several inputs: one fast lit preview per tile, laid
//! out in a grid with labels, with an optional reference image (a concept)
//! as the first tile. Also the named camera presets (`game`) and the small
//! bitmap text the sheet labels and the preview overlays draw with.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::preview::Preview;
use crate::state::Session;

/// Most tiles in one sheet, the reference included.
pub const MAX_TILES: usize = 65;
/// Largest sheet side in pixels.
pub const MAX_SHEET: u32 = 8192;
/// A tile's size when the caller gives none.
pub const TILE: u32 = 512;
/// Space between tiles and around the sheet.
const GAP: u32 = 8;
/// Largest reference image side read.
const MAX_REFERENCE: u32 = 16_384;

/// A camera preset: angles, projection, and optionally the lens and margin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preset {
    pub yaw: f32,
    pub pitch: f32,
    pub orthographic: bool,
    pub fov: Option<f32>,
    pub margin: Option<f32>,
}

/// The built-in presets. `game` is the frog-world gameplay angle.
pub const PRESETS: [(&str, Preset); 2] = [
    (
        "game",
        Preset {
            yaw: -60.0,
            pitch: 25.0,
            orthographic: true,
            fov: None,
            margin: None,
        },
    ),
    (
        "default",
        Preset {
            yaw: 30.0,
            pitch: 25.0,
            orthographic: false,
            fov: None,
            margin: None,
        },
    ),
];

/// The preset `name`, from `custom` (MCP `presets`: name to `{yaw, pitch,
/// orthographic, fov, margin}`) over the built-in one of that name.
pub fn preset(name: &str, custom: Option<&Value>) -> Result<Preset, String> {
    let builtin = PRESETS.iter().find(|(n, _)| *n == name).map(|(_, p)| *p);
    let given = match custom {
        None | Some(Value::Null) => None,
        Some(Value::Object(map)) => map.get(name),
        Some(_) => return Err("presets must be an object of name: {yaw, pitch, ...}".into()),
    };
    let Some(given) = given else {
        return builtin.ok_or_else(|| {
            let mut names: Vec<String> = PRESETS.iter().map(|(n, _)| n.to_string()).collect();
            if let Some(Value::Object(map)) = custom {
                names.extend(map.keys().cloned());
            }
            format!("preset must be one of {}, not `{name}`", names.join(", "))
        });
    };
    let Value::Object(fields) = given else {
        return Err(format!(
            "preset `{name}` must be {{yaw, pitch, orthographic, fov, margin}}"
        ));
    };
    let base = builtin.unwrap_or(PRESETS[1].1);
    let number = |key: &str| -> Result<Option<f32>, String> {
        match fields.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(found) => found
                .as_f64()
                .filter(|v| v.is_finite())
                .map(|v| Some(v as f32))
                .ok_or_else(|| format!("preset `{name}` {key} must be a number")),
        }
    };
    for key in fields.keys() {
        if !["yaw", "pitch", "orthographic", "fov", "margin"].contains(&key.as_str()) {
            return Err(format!(
                "preset `{name}` takes yaw, pitch, orthographic, fov, and margin, not `{key}`"
            ));
        }
    }
    let orthographic = match fields.get("orthographic") {
        None | Some(Value::Null) => base.orthographic,
        Some(found) => found
            .as_bool()
            .ok_or_else(|| format!("preset `{name}` orthographic must be true or false"))?,
    };
    let preset = Preset {
        yaw: number("yaw")?.unwrap_or(base.yaw),
        pitch: number("pitch")?.unwrap_or(base.pitch),
        orthographic,
        fov: number("fov")?.or(base.fov),
        margin: number("margin")?.or(base.margin),
    };
    if !(-90.0..=90.0).contains(&preset.pitch) {
        return Err(format!("preset `{name}` pitch must be -90..90"));
    }
    Ok(preset)
}

/// Whether `params` ask `preview` for a contact sheet.
pub fn wanted(params: &Value) -> bool {
    ["inputs", "variants", "reference"]
        .iter()
        .any(|key| params.get(*key).is_some_and(|v| !v.is_null()))
}

/// One tile to render: a file or the open document, with parameters.
struct Source {
    path: Option<PathBuf>,
    label: String,
    params: Map<String, Value>,
}

fn sources(params: &Value) -> Result<Vec<Source>, String> {
    let mut out = Vec::new();
    match params.get("inputs") {
        None | Some(Value::Null) => {}
        Some(Value::Array(list)) => {
            for (index, item) in list.iter().enumerate() {
                out.push(match item {
                    Value::String(path) => Source {
                        path: Some(PathBuf::from(path)),
                        label: stem(Path::new(path)),
                        params: Map::new(),
                    },
                    Value::Object(map) => {
                        let path = match map.get("path") {
                            None | Some(Value::Null) => None,
                            Some(Value::String(path)) => Some(PathBuf::from(path)),
                            Some(_) => return Err(format!("input {index} path must be a string")),
                        };
                        let label = match map.get("label").or_else(|| map.get("name")) {
                            Some(Value::String(label)) => label.clone(),
                            None | Some(Value::Null) => path
                                .as_deref()
                                .map(stem)
                                .unwrap_or_else(|| format!("input {}", index + 1)),
                            Some(_) => return Err(format!("input {index} label must be a string")),
                        };
                        let params = match map.get("params") {
                            None | Some(Value::Null) => Map::new(),
                            Some(Value::Object(values)) => values.clone(),
                            Some(_) => {
                                return Err(format!("input {index} params must be an object"))
                            }
                        };
                        Source {
                            path,
                            label,
                            params,
                        }
                    }
                    _ => {
                        return Err(format!(
                            "input {index} must be a .vox path or {{path?, label?, params?}}"
                        ))
                    }
                });
            }
        }
        Some(_) => {
            return Err("inputs must be a list of .vox paths or {path?, label?, params?}".into())
        }
    }
    let sets = match params.get("variants") {
        None | Some(Value::Null) => None,
        Some(Value::String(path)) => Some(crate::variants::read_sets(Path::new(path))?),
        Some(list @ Value::Array(_)) => Some(list.clone()),
        Some(_) => {
            return Err("variants must be a list of {name?, params} or a variants JSON path".into())
        }
    };
    if let Some(Value::Array(sets)) = sets {
        for (index, set) in sets.iter().enumerate() {
            let label = match set.get("name") {
                Some(Value::String(name)) => name.clone(),
                None | Some(Value::Null) => format!("variant_{}", index + 1),
                Some(_) => return Err(format!("variant {index} name must be a string")),
            };
            let params = match set.get("params") {
                None | Some(Value::Null) => Map::new(),
                Some(Value::Object(values)) => values.clone(),
                Some(_) => return Err(format!("variant `{label}` params must be an object")),
            };
            out.push(Source {
                path: None,
                label,
                params,
            });
        }
    }
    Ok(out)
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// An RGBA image.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Render the contact sheet `params` ask for with the camera of `preview`,
/// write it to `path`, and describe it.
pub fn contact_sheet(
    session: &mut Session,
    params: &Value,
    preview: &Preview,
    object: Option<&str>,
    path: &Path,
) -> Result<Value, String> {
    let started = std::time::Instant::now();
    let sources = sources(params)?;
    let reference = match params.get("reference") {
        None | Some(Value::Null) => None,
        Some(Value::String(path)) => Some((PathBuf::from(path), None)),
        Some(Value::Object(map)) => {
            let path = map
                .get("path")
                .and_then(Value::as_str)
                .ok_or("reference must be a PNG path or {path, label?}")?;
            let label = map.get("label").and_then(Value::as_str).map(str::to_string);
            Some((PathBuf::from(path), label))
        }
        Some(_) => return Err("reference must be a PNG path or {path, label?}".into()),
    };
    let tiles = sources.len() + usize::from(reference.is_some()) + usize::from(sources.is_empty());
    if tiles > MAX_TILES {
        return Err(format!(
            "a contact sheet holds at most {} models and a reference",
            MAX_TILES - 1
        ));
    }
    let labels = match params.get("labels") {
        None | Some(Value::Null) => true,
        Some(found) => found.as_bool().ok_or("labels must be true or false")?,
    };
    let columns = match params.get("columns") {
        None | Some(Value::Null) => (tiles as f64).sqrt().ceil() as u32,
        Some(found) => found
            .as_u64()
            .filter(|c| (1..=MAX_TILES as u64).contains(c))
            .ok_or_else(|| format!("columns must be 1..={MAX_TILES}"))?
            as u32,
    }
    .min(tiles as u32)
    .max(1);
    let rows = (tiles as u32).div_ceil(columns);
    let (tw, th) = (preview.width, preview.height);
    let scale = if tw >= 256 { 2 } else { 1 };
    let lines = if labels { 2 } else { 0 };
    let strip = if labels {
        lines * line_height(scale) + 6
    } else {
        0
    };
    let sheet_w = GAP + columns * (tw + GAP);
    let sheet_h = GAP + rows * (th + strip + GAP);
    if sheet_w > MAX_SHEET || sheet_h > MAX_SHEET {
        return Err(format!(
            "the sheet would be {sheet_w}x{sheet_h}; keep it within {MAX_SHEET}x{MAX_SHEET} with a smaller width, height, or more columns"
        ));
    }
    // Every file is read before anything is drawn.
    let reference_image = match &reference {
        Some((path, _)) => Some(read_png(path)?),
        None => None,
    };
    let mut cells: Vec<(Image, String, String, Value)> = Vec::with_capacity(tiles);
    if let (Some((path, label)), Some(image)) = (&reference, reference_image) {
        let fitted = fit(&image, tw, th, preview.background);
        let label = label.clone().unwrap_or_else(|| stem(path));
        let info = json!({"reference": path.display().to_string(), "label": label});
        cells.push((fitted, label, "reference".into(), info));
    }
    let render = |session: &mut Session, source: &Source| -> Result<(Image, Value), String> {
        if !source.params.is_empty() {
            crate::bridge::apply_values(session, &source.params)?;
            for index in 0..session.objects.len() {
                if !session.objects[index].script.source.is_empty() {
                    session.regenerate_script_within(index, Some(crate::script::SCRIPT_TIMEOUT))?;
                }
            }
        }
        let mut tile = preview.clone();
        if let Some(key) = object {
            tile.object = Some(crate::preview::find_object(session, key)?);
        }
        let frame = crate::preview::render(session, &tile)?;
        let info = json!({"triangles": frame.triangles, "voxels": session.voxel_count()});
        Ok((
            Image {
                width: tw,
                height: th,
                rgba: frame.rgba,
            },
            info,
        ))
    };
    let current = || Source {
        path: None,
        label: session_label(session),
        params: Map::new(),
    };
    let list = if sources.is_empty() {
        vec![current()]
    } else {
        sources
    };
    for source in &list {
        let (image, mut info) = match &source.path {
            Some(path) => {
                let mut other = Session::new();
                other.open_vox(path)?;
                render(&mut other, source)
            }
            None => session.scratch_run(|session| render(session, source)),
        }
        .map_err(|err| format!("{}: {err}", source.label))?;
        let detail = source
            .params
            .iter()
            .map(|(key, value)| format!("{key}={}", compact(value)))
            .collect::<Vec<_>>()
            .join(" ");
        info["label"] = json!(source.label);
        if let Some(path) = &source.path {
            info["input"] = json!(path.display().to_string());
        }
        if !source.params.is_empty() {
            info["params"] = Value::Object(source.params.clone());
        }
        cells.push((image, source.label.clone(), detail, info));
    }
    let mut sheet = Image {
        width: sheet_w,
        height: sheet_h,
        rgba: preview
            .background
            .iter()
            .copied()
            .cycle()
            .take((sheet_w * sheet_h * 4) as usize)
            .collect(),
    };
    let ink = ink_for(preview.background);
    let mut placed = Vec::with_capacity(cells.len());
    for (index, (image, label, detail, mut info)) in cells.into_iter().enumerate() {
        let (col, row) = (index as u32 % columns, index as u32 / columns);
        let x = GAP + col * (tw + GAP);
        let y = GAP + row * (th + strip + GAP);
        blit(&mut sheet, &image, x, y);
        if labels {
            let ty = y + th + 4;
            draw_text_clipped(&mut sheet, x, ty, tw, &label, scale, ink);
            draw_text_clipped(
                &mut sheet,
                x,
                ty + line_height(scale),
                tw,
                &detail,
                scale.max(2) - 1,
                dim(ink),
            );
        }
        info["x"] = json!(x);
        info["y"] = json!(y);
        info["width"] = json!(tw);
        info["height"] = json!(th);
        placed.push(info);
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("Could not create {}: {err}", parent.display()))?;
    }
    crate::trace::write_rgba_png(path, sheet.width, sheet.height, &sheet.rgba)?;
    Ok(json!({
        "path": path.display().to_string(),
        "width": sheet.width,
        "height": sheet.height,
        "columns": columns,
        "rows": rows,
        "tiles": placed,
        "yaw": preview.yaw,
        "pitch": preview.pitch,
        "orthographic": preview.orthographic,
        "milliseconds": started.elapsed().as_millis() as u64
    }))
}

/// The open document's label: its file stem, or `document`.
fn session_label(session: &Session) -> String {
    session
        .path
        .as_deref()
        .map(stem)
        .unwrap_or_else(|| "document".into())
}

/// A parameter value as short text: numbers trimmed, lists joined.
fn compact(value: &Value) -> String {
    match value {
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() != 0.0 => format!("{}", (f * 1000.0).round() / 1000.0),
            _ => n.to_string(),
        },
        Value::String(s) => s.clone(),
        Value::Array(list) => format!(
            "[{}]",
            list.iter().map(compact).collect::<Vec<_>>().join(",")
        ),
        other => other.to_string(),
    }
}

/// Copy `image` onto `sheet` at `(x, y)`, blending by alpha.
fn blit(sheet: &mut Image, image: &Image, x: u32, y: u32) {
    for row in 0..image.height {
        let sy = y + row;
        if sy >= sheet.height {
            break;
        }
        for col in 0..image.width {
            let sx = x + col;
            if sx >= sheet.width {
                break;
            }
            let from = ((row * image.width + col) * 4) as usize;
            let to = ((sy * sheet.width + sx) * 4) as usize;
            let a = u32::from(image.rgba[from + 3]);
            for c in 0..3 {
                let over = u32::from(image.rgba[from + c]) * a;
                let under = u32::from(sheet.rgba[to + c]) * (255 - a);
                sheet.rgba[to + c] = ((over + under + 127) / 255) as u8;
            }
            let under = u32::from(sheet.rgba[to + 3]);
            sheet.rgba[to + 3] = (a + under * (255 - a) / 255).min(255) as u8;
        }
    }
}

/// `image` scaled to fit inside `width` x `height`, centred on `background`,
/// each output pixel the mean of the source pixels it covers.
pub fn fit(image: &Image, width: u32, height: u32, background: [u8; 4]) -> Image {
    let scale = (width as f64 / image.width as f64).min(height as f64 / image.height as f64);
    let (w, h) = (
        ((image.width as f64 * scale).round() as u32).clamp(1, width),
        ((image.height as f64 * scale).round() as u32).clamp(1, height),
    );
    let (ox, oy) = ((width - w) / 2, (height - h) / 2);
    let mut out = Image {
        width,
        height,
        rgba: background
            .iter()
            .copied()
            .cycle()
            .take((width * height * 4) as usize)
            .collect(),
    };
    let (fx, fy) = (
        image.width as f64 / w as f64,
        image.height as f64 / h as f64,
    );
    let mut scaled = Image {
        width: w,
        height: h,
        rgba: vec![0; (w * h * 4) as usize],
    };
    for y in 0..h {
        let y0 = (y as f64 * fy) as u32;
        let y1 = (((y + 1) as f64 * fy).ceil() as u32).clamp(y0 + 1, image.height);
        for x in 0..w {
            let x0 = (x as f64 * fx) as u32;
            let x1 = (((x + 1) as f64 * fx).ceil() as u32).clamp(x0 + 1, image.width);
            let mut sum = [0u64; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let i = ((sy * image.width + sx) * 4) as usize;
                    let a = u64::from(image.rgba[i + 3]);
                    for c in 0..3 {
                        sum[c] += u64::from(image.rgba[i + c]) * a;
                    }
                    sum[3] += a;
                }
            }
            let n = u64::from((y1 - y0) * (x1 - x0));
            let o = ((y * w + x) * 4) as usize;
            if sum[3] > 0 {
                for c in 0..3 {
                    scaled.rgba[o + c] = (sum[c] / sum[3]) as u8;
                }
            }
            scaled.rgba[o + 3] = (sum[3] / n) as u8;
        }
    }
    blit(&mut out, &scaled, ox, oy);
    out
}

/// A PNG of any colour type as 8-bit RGBA.
pub fn read_png(path: &Path) -> Result<Image, String> {
    let file = std::fs::File::open(path)
        .map_err(|err| format!("Could not open {}: {err}", path.display()))?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .map_err(|err| format!("{} is not a PNG: {err}", path.display()))?;
    let (width, height) = reader.info().size();
    if width == 0 || height == 0 || width > MAX_REFERENCE || height > MAX_REFERENCE {
        return Err(format!(
            "{} is {width}x{height}; a reference is at most {MAX_REFERENCE} pixels a side",
            path.display()
        ));
    }
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| format!("{} is too large", path.display()))?;
    let mut buf = vec![0; size];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|err| format!("Could not read {}: {err}", path.display()))?;
    let pixels = (info.width * info.height) as usize;
    let mut rgba = Vec::with_capacity(pixels * 4);
    let stride = info.line_size;
    for row in 0..info.height as usize {
        let line = &buf[row * stride..row * stride + stride];
        for x in 0..info.width as usize {
            let px = match info.color_type {
                png::ColorType::Rgba => [
                    line[x * 4],
                    line[x * 4 + 1],
                    line[x * 4 + 2],
                    line[x * 4 + 3],
                ],
                png::ColorType::Rgb => [line[x * 3], line[x * 3 + 1], line[x * 3 + 2], 255],
                png::ColorType::GrayscaleAlpha => {
                    [line[x * 2], line[x * 2], line[x * 2], line[x * 2 + 1]]
                }
                png::ColorType::Grayscale => [line[x], line[x], line[x], 255],
                png::ColorType::Indexed => {
                    return Err(format!(
                        "{} kept its palette; save it as RGB",
                        path.display()
                    ))
                }
            };
            rgba.extend(px);
        }
    }
    Ok(Image {
        width: info.width,
        height: info.height,
        rgba,
    })
}

/// Light text on a dark background, dark text on a light one.
fn ink_for(background: [u8; 4]) -> [u8; 4] {
    let luma = 0.2126 * f32::from(background[0])
        + 0.7152 * f32::from(background[1])
        + 0.0722 * f32::from(background[2]);
    if background[3] < 128 || luma < 128.0 {
        [228, 230, 234, 255]
    } else {
        [24, 26, 30, 255]
    }
}

fn dim(ink: [u8; 4]) -> [u8; 4] {
    let mid = |c: u8| ((u16::from(c) + 128) / 2) as u8;
    [mid(ink[0]), mid(ink[1]), mid(ink[2]), 255]
}

/// Pixel height of one line of text at `scale`, with its leading.
pub fn line_height(scale: u32) -> u32 {
    9 * scale
}

/// Pixel width of `text` at `scale`.
pub fn text_width(text: &str, scale: u32) -> u32 {
    (text.chars().count() as u32 * 6 * scale).saturating_sub(scale)
}

/// `text`, cut with `~` to fit `max_width` pixels.
fn draw_text_clipped(
    image: &mut Image,
    x: u32,
    y: u32,
    max_width: u32,
    text: &str,
    scale: u32,
    color: [u8; 4],
) {
    let fits = (max_width / (6 * scale)) as usize;
    let text: String = if text_width(text, scale) > max_width {
        text.chars()
            .take(fits.saturating_sub(1))
            .chain(['~'])
            .collect()
    } else {
        text.to_string()
    };
    draw_text(
        &mut image.rgba,
        image.width,
        image.height,
        x as i32,
        y as i32,
        &text,
        scale,
        color,
    );
}

/// Draw `text` in a 5x7 bitmap font with its top left at `(x, y)`, each
/// font pixel `scale` pixels wide. Characters outside ASCII draw as `?`.
#[allow(clippy::too_many_arguments)]
pub fn draw_text(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    text: &str,
    scale: u32,
    color: [u8; 4],
) {
    let s = scale as i32;
    for (index, ch) in text.chars().enumerate() {
        let code = if (' '..='~').contains(&ch) { ch } else { '?' } as usize - 32;
        let glyph = &FONT[code * 5..code * 5 + 5];
        let left = x + index as i32 * 6 * s;
        for (col, bits) in glyph.iter().enumerate() {
            for row in 0..7 {
                if bits >> row & 1 == 0 {
                    continue;
                }
                for dy in 0..s {
                    for dx in 0..s {
                        let px = left + col as i32 * s + dx;
                        let py = y + row * s + dy;
                        if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                            continue;
                        }
                        let i = ((py as u32 * width + px as u32) * 4) as usize;
                        rgba[i..i + 4].copy_from_slice(&color);
                    }
                }
            }
        }
    }
}

/// ASCII 32..=126 in a 5x7 font: five columns per character, bit 0 at the
/// top.
#[rustfmt::skip]
const FONT: [u8; 95 * 5] = [
    0x00,0x00,0x00,0x00,0x00, 0x00,0x00,0x5F,0x00,0x00, 0x00,0x07,0x00,0x07,0x00, 0x14,0x7F,0x14,0x7F,0x14,
    0x24,0x2A,0x7F,0x2A,0x12, 0x23,0x13,0x08,0x64,0x62, 0x36,0x49,0x56,0x20,0x50, 0x00,0x05,0x03,0x00,0x00,
    0x00,0x1C,0x22,0x41,0x00, 0x00,0x41,0x22,0x1C,0x00, 0x14,0x08,0x3E,0x08,0x14, 0x08,0x08,0x3E,0x08,0x08,
    0x00,0x50,0x30,0x00,0x00, 0x08,0x08,0x08,0x08,0x08, 0x00,0x60,0x60,0x00,0x00, 0x20,0x10,0x08,0x04,0x02,
    0x3E,0x51,0x49,0x45,0x3E, 0x00,0x42,0x7F,0x40,0x00, 0x42,0x61,0x51,0x49,0x46, 0x21,0x41,0x45,0x4B,0x31,
    0x18,0x14,0x12,0x7F,0x10, 0x27,0x45,0x45,0x45,0x39, 0x3C,0x4A,0x49,0x49,0x30, 0x01,0x71,0x09,0x05,0x03,
    0x36,0x49,0x49,0x49,0x36, 0x06,0x49,0x49,0x29,0x1E, 0x00,0x36,0x36,0x00,0x00, 0x00,0x56,0x36,0x00,0x00,
    0x08,0x14,0x22,0x41,0x00, 0x14,0x14,0x14,0x14,0x14, 0x00,0x41,0x22,0x14,0x08, 0x02,0x01,0x51,0x09,0x06,
    0x32,0x49,0x79,0x41,0x3E, 0x7E,0x11,0x11,0x11,0x7E, 0x7F,0x49,0x49,0x49,0x36, 0x3E,0x41,0x41,0x41,0x22,
    0x7F,0x41,0x41,0x22,0x1C, 0x7F,0x49,0x49,0x49,0x41, 0x7F,0x09,0x09,0x09,0x01, 0x3E,0x41,0x49,0x49,0x7A,
    0x7F,0x08,0x08,0x08,0x7F, 0x00,0x41,0x7F,0x41,0x00, 0x20,0x40,0x41,0x3F,0x01, 0x7F,0x08,0x14,0x22,0x41,
    0x7F,0x40,0x40,0x40,0x40, 0x7F,0x02,0x0C,0x02,0x7F, 0x7F,0x04,0x08,0x10,0x7F, 0x3E,0x41,0x41,0x41,0x3E,
    0x7F,0x09,0x09,0x09,0x06, 0x3E,0x41,0x51,0x21,0x5E, 0x7F,0x09,0x19,0x29,0x46, 0x46,0x49,0x49,0x49,0x31,
    0x01,0x01,0x7F,0x01,0x01, 0x3F,0x40,0x40,0x40,0x3F, 0x1F,0x20,0x40,0x20,0x1F, 0x3F,0x40,0x38,0x40,0x3F,
    0x63,0x14,0x08,0x14,0x63, 0x07,0x08,0x70,0x08,0x07, 0x61,0x51,0x49,0x45,0x43, 0x00,0x7F,0x41,0x41,0x00,
    0x02,0x04,0x08,0x10,0x20, 0x00,0x41,0x41,0x7F,0x00, 0x04,0x02,0x01,0x02,0x04, 0x40,0x40,0x40,0x40,0x40,
    0x00,0x01,0x02,0x04,0x00, 0x20,0x54,0x54,0x54,0x78, 0x7F,0x48,0x44,0x44,0x38, 0x38,0x44,0x44,0x44,0x20,
    0x38,0x44,0x44,0x48,0x7F, 0x38,0x54,0x54,0x54,0x18, 0x08,0x7E,0x09,0x01,0x02, 0x0C,0x52,0x52,0x52,0x3E,
    0x7F,0x08,0x04,0x04,0x78, 0x00,0x44,0x7D,0x40,0x00, 0x20,0x40,0x44,0x3D,0x00, 0x7F,0x10,0x28,0x44,0x00,
    0x00,0x41,0x7F,0x40,0x00, 0x7C,0x04,0x18,0x04,0x78, 0x7C,0x08,0x04,0x04,0x78, 0x38,0x44,0x44,0x44,0x38,
    0x7C,0x14,0x14,0x14,0x08, 0x08,0x14,0x14,0x18,0x7C, 0x7C,0x08,0x04,0x04,0x08, 0x48,0x54,0x54,0x54,0x20,
    0x04,0x3F,0x44,0x40,0x20, 0x3C,0x40,0x40,0x20,0x7C, 0x1C,0x20,0x40,0x20,0x1C, 0x3C,0x40,0x30,0x40,0x3C,
    0x44,0x28,0x10,0x28,0x44, 0x0C,0x50,0x50,0x50,0x3C, 0x44,0x64,0x54,0x4C,0x44, 0x00,0x08,0x36,0x41,0x00,
    0x00,0x00,0x7F,0x00,0x00, 0x00,0x41,0x36,0x08,0x00, 0x08,0x04,0x08,0x10,0x08,
];

#[cfg(test)]
#[path = "../tests/sheet.rs"]
mod tests;
