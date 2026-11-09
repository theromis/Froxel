//! MCP `compare_silhouette`: how closely the model's silhouette from a
//! camera matches a concept's. The model is drawn with the fast preview
//! ([`crate::preview`]); its mask is every pixel the model covers. The
//! concept's mask is cut from an image like a `carve_from_views` view
//! ([`crate::tools::mask_from_rgba`]). The concept is placed over the
//! preview with one uniform scale and an offset, fitted or given, so its
//! proportions are kept: a model too long for its concept stays too long.
//! The overlay shows the preview with the concept's outline, the concept
//! the model misses, and the model outside the concept.

use crate::preview::{self, Preview};
use crate::state::Session;
use crate::tools::Mask;

/// How the concept is placed over the preview: concept pixel `p` (in its
/// crop) lands on output pixel `p * scale + offset`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Align {
    /// Scale and offset that maximise the overlap, starting from equal
    /// areas and centroids.
    Fit,
    /// Equal heights, bottoms and centres lined up.
    Height,
    /// Equal widths, bottoms and centres lined up.
    Width,
    Given {
        scale: f32,
        offset: [f32; 2],
    },
}

impl Align {
    pub fn name(self) -> &'static str {
        match self {
            Align::Fit => "fit",
            Align::Height => "height",
            Align::Width => "width",
            Align::Given { .. } => "given",
        }
    }
}

pub const MISSING_COLOR: [u8; 3] = [255, 60, 150];
pub const EXCESS_COLOR: [u8; 3] = [255, 170, 30];
pub const OUTLINE_COLOR: [u8; 3] = [90, 225, 255];

#[derive(Clone, Debug)]
pub struct Compared {
    /// The overlay, `preview.width * preview.height` RGBA.
    pub rgba: Vec<u8>,
    pub iou: f32,
    /// Share of the concept the model does not cover.
    pub missing: f32,
    /// Share of the model outside the concept.
    pub excess: f32,
    pub concept_pixels: usize,
    pub model_pixels: usize,
    pub overlap_pixels: usize,
    pub scale: f32,
    pub offset: [f32; 2],
    /// Inclusive output-pixel bounds `[x0, y0, x1, y1]`.
    pub concept_bounds: Option<[i32; 4]>,
    pub model_bounds: Option<[i32; 4]>,
    pub pixels_per_voxel: Option<f32>,
}

/// The concept's mask in output pixels for one placement.
fn placed(concept: &Mask, w: u32, h: u32, scale: f32, offset: [f32; 2]) -> Vec<bool> {
    let mut out = vec![false; (w * h) as usize];
    for y in 0..h {
        let cy = ((y as f32 + 0.5 - offset[1]) / scale).floor();
        if cy < 0.0 || cy >= concept.height as f32 {
            continue;
        }
        for x in 0..w {
            let cx = ((x as f32 + 0.5 - offset[0]) / scale).floor();
            if cx < 0.0 || cx >= concept.width as f32 {
                continue;
            }
            out[(y * w + x) as usize] = concept.inked(cx as u32, cy as u32);
        }
    }
    out
}

/// Concept, model, and overlap pixel counts.
fn counts(concept: &[bool], model: &[bool]) -> (usize, usize, usize) {
    let mut c = 0;
    let mut m = 0;
    let mut both = 0;
    for (a, b) in concept.iter().zip(model) {
        c += usize::from(*a);
        m += usize::from(*b);
        both += usize::from(*a && *b);
    }
    (c, m, both)
}

fn iou((c, m, both): (usize, usize, usize)) -> f32 {
    let union = c + m - both;
    if union == 0 {
        0.0
    } else {
        both as f32 / union as f32
    }
}

fn bounds(mask: &[bool], w: u32) -> Option<[i32; 4]> {
    let mut found: Option<[i32; 4]> = None;
    for (i, inked) in mask.iter().enumerate() {
        if !inked {
            continue;
        }
        let (x, y) = ((i as u32 % w) as i32, (i as u32 / w) as i32);
        found = Some(match found {
            None => [x, y, x, y],
            Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
        });
    }
    found
}

/// Pixel count and centroid (pixel centres) of a mask's ink.
fn moments(width: u32, inked: impl Iterator<Item = bool>) -> (f64, [f64; 2]) {
    let mut n = 0.0;
    let mut sum = [0.0f64; 2];
    for (i, on) in inked.enumerate() {
        if on {
            n += 1.0;
            sum[0] += (i as u32 % width) as f64 + 0.5;
            sum[1] += (i as u32 / width) as f64 + 0.5;
        }
    }
    (n, [sum[0] / n.max(1.0), sum[1] / n.max(1.0)])
}

/// The scale and offset of `align` for the model mask `model`.
pub fn fit(concept: &Mask, model: &[bool], w: u32, h: u32, align: Align) -> (f32, [f32; 2]) {
    let concept_ink = || (0..concept.pixels.len()).map(|i| concept.pixels[i].is_some());
    let (Some([cx0, cy0, cx1, cy1]), Some([mx0, my0, mx1, my1])) =
        (concept.bounds(), bounds(model, w))
    else {
        return (1.0, [0.0, 0.0]);
    };
    let (cx0, cy0, cx1, cy1) = (cx0 as f32, cy0 as f32, cx1 as f32 + 1.0, cy1 as f32 + 1.0);
    let (mx0, my0, mx1, my1) = (mx0 as f32, my0 as f32, mx1 as f32 + 1.0, my1 as f32 + 1.0);
    let lined_up = |scale: f32| {
        // Centres across and bottoms lined up.
        let ox = (mx0 + mx1) / 2.0 - (cx0 + cx1) / 2.0 * scale;
        let oy = my1 - cy1 * scale;
        (scale, [ox, oy])
    };
    match align {
        Align::Given { scale, offset } => (scale, offset),
        Align::Height => lined_up((my1 - my0) / (cy1 - cy0)),
        Align::Width => lined_up((mx1 - mx0) / (cx1 - cx0)),
        Align::Fit => {
            let (ac, cc) = moments(concept.width, concept_ink());
            let (am, cm) = moments(w, model.iter().copied());
            let scale = (am / ac).sqrt() as f32;
            let mut best = (
                scale,
                [
                    (cm[0] - cc[0] * scale as f64) as f32,
                    (cm[1] - cc[1] * scale as f64) as f32,
                ],
            );
            let score = |(scale, offset): (f32, [f32; 2])| {
                iou(counts(&placed(concept, w, h, scale, offset), model))
            };
            let mut best_score = score(best);
            // Pattern search on log scale and the two offsets.
            let mut step_scale = 0.08f32;
            let mut step_offset = (w.max(h) as f32 / 16.0).max(1.0);
            let mut evals = 0;
            while (step_offset >= 0.25 || step_scale >= 0.002) && evals < 400 {
                let mut improved = false;
                for (ds, dx, dy) in [
                    (1.0, 0.0, 0.0),
                    (-1.0, 0.0, 0.0),
                    (0.0, 1.0, 0.0),
                    (0.0, -1.0, 0.0),
                    (0.0, 0.0, 1.0),
                    (0.0, 0.0, -1.0),
                ] {
                    let factor = (ds * step_scale).exp();
                    // Scale about the concept's centroid in the output, so a
                    // scale step does not also slide the silhouette.
                    let (s0, o0) = best;
                    let s1 = s0 * factor;
                    let centre = [cc[0] as f32 * s0 + o0[0], cc[1] as f32 * s0 + o0[1]];
                    let candidate = (
                        s1,
                        [
                            centre[0] - cc[0] as f32 * s1 + dx * step_offset,
                            centre[1] - cc[1] as f32 * s1 + dy * step_offset,
                        ],
                    );
                    if ds == 0.0 && dx == 0.0 && dy == 0.0 {
                        continue;
                    }
                    evals += 1;
                    let s = score(candidate);
                    if s > best_score + 1e-6 {
                        best = candidate;
                        best_score = s;
                        improved = true;
                    }
                }
                if !improved {
                    step_scale *= 0.5;
                    step_offset *= 0.5;
                }
            }
            best
        }
    }
}

/// The IoU of `concept` placed over the model mask `model` (`w * h`, row
/// major) with the scale and offset that fit it best (`Align::Fit`).
pub fn fit_iou(concept: &Mask, model: &[bool], w: u32, h: u32) -> f32 {
    if model.is_empty() || concept.count() == 0 {
        return 0.0;
    }
    let (scale, offset) = fit(concept, model, w, h, Align::Fit);
    iou(counts(&placed(concept, w, h, scale, offset), model))
}

/// Draw the model with `preview`'s camera and compare its silhouette with
/// `concept`. The overlay is drawn on `background`.
pub fn compare(
    session: &Session,
    preview: &Preview,
    concept: &Mask,
    align: Align,
    background: [u8; 4],
) -> Result<Compared, String> {
    if concept.count() == 0 {
        return Err("the concept mask is empty: check crop and background".into());
    }
    if let Align::Given { scale, offset } = align {
        if !(scale > 0.0 && scale.is_finite()) || !offset.iter().all(|v| v.is_finite()) {
            return Err("align scale must be above 0 and offset finite".into());
        }
    }
    let frame = preview::render(
        session,
        &Preview {
            background: [0, 0, 0, 0],
            ..preview.clone()
        },
    )?;
    let (w, h) = (preview.width, preview.height);
    let model: Vec<bool> = frame.rgba.chunks_exact(4).map(|px| px[3] >= 128).collect();
    if !model.contains(&true) {
        return Err("the model covers no pixel of the image".into());
    }
    let (scale, offset) = fit(concept, &model, w, h, align);
    let concept_out = placed(concept, w, h, scale, offset);
    let (c, m, both) = counts(&concept_out, &model);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for (i, px) in frame.rgba.chunks_exact(4).enumerate() {
        let a = f32::from(px[3]) / 255.0;
        let bg = background;
        let mut rgb = [0, 1, 2].map(|k| {
            f32::from(px[k]) * a + f32::from(bg[k]) * (1.0 - a) * f32::from(bg[3]) / 255.0
        });
        let tint = match (concept_out[i], model[i]) {
            (true, false) => Some((MISSING_COLOR, 0.6)),
            (false, true) => Some((EXCESS_COLOR, 0.55)),
            _ => None,
        };
        if let Some((color, strength)) = tint {
            for k in 0..3 {
                rgb[k] = rgb[k] * (1.0 - strength) + f32::from(color[k]) * strength;
            }
        }
        rgba.extend(rgb.map(|v| v.round().clamp(0.0, 255.0) as u8));
        rgba.push(255);
    }
    // The concept's outline: inked pixels beside one that is not.
    let thick = 1 + (w.min(h) / 700) as i32;
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            if !concept_out[(y as u32 * w + x as u32) as usize] {
                continue;
            }
            let edge = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| {
                let (nx, ny) = (x + dx, y + dy);
                !(0..w as i32).contains(&nx)
                    || !(0..h as i32).contains(&ny)
                    || !concept_out[(ny as u32 * w + nx as u32) as usize]
            });
            if edge {
                let [r, g, b] = OUTLINE_COLOR;
                crate::sheet::fill_rect(
                    &mut rgba,
                    w,
                    h,
                    [x, y, x + thick, y + thick],
                    [r, g, b, 255],
                );
            }
        }
    }
    let result = Compared {
        rgba,
        iou: iou((c, m, both)),
        missing: if c == 0 {
            0.0
        } else {
            (c - both) as f32 / c as f32
        },
        excess: (m - both) as f32 / m as f32,
        concept_pixels: c,
        model_pixels: m,
        overlap_pixels: both,
        scale,
        offset,
        concept_bounds: bounds(&concept_out, w),
        model_bounds: bounds(&model, w),
        pixels_per_voxel: frame.pixels_per_voxel,
    };
    let mut result = result;
    let mut rgba = std::mem::take(&mut result.rgba);
    draw_legend(&mut rgba, w, h, &result);
    result.rgba = rgba;
    Ok(result)
}

/// The numbers in the overlay's bottom-left corner.
pub fn legend_lines(compared: &Compared) -> Vec<String> {
    vec![
        format!("IoU {:.3}", compared.iou),
        format!("missing {:.1}%", compared.missing * 100.0),
        format!("excess {:.1}%", compared.excess * 100.0),
    ]
}

fn draw_legend(rgba: &mut [u8], w: u32, h: u32, compared: &Compared) {
    let scale = if w.min(h) >= 400 { 2 } else { 1 };
    let lines = legend_lines(compared);
    let line = crate::sheet::line_height(scale) as i32;
    let swatch = 7 * scale as i32;
    let tw = lines
        .iter()
        .map(|text| crate::sheet::text_width(text, scale))
        .max()
        .unwrap_or(0) as i32
        + swatch
        + 4;
    let x = 4 * scale as i32;
    let y0 = h as i32 - x - line * lines.len() as i32;
    crate::sheet::fill_rect(
        rgba,
        w,
        h,
        [x - 3, y0 - 3, x + tw + 3, h as i32 - x + 1],
        [12, 12, 14, 210],
    );
    let swatches = [OUTLINE_COLOR, MISSING_COLOR, EXCESS_COLOR];
    for (i, text) in lines.iter().enumerate() {
        let y = y0 + line * i as i32;
        let [r, g, b] = swatches[i];
        crate::sheet::fill_rect(rgba, w, h, [x, y, x + swatch, y + swatch], [r, g, b, 255]);
        crate::sheet::draw_text(
            rgba,
            w,
            h,
            x + swatch + 4,
            y,
            text,
            scale,
            [230, 232, 236, 255],
        );
    }
}

#[cfg(test)]
#[path = "../tests/silhouette.rs"]
mod tests;
