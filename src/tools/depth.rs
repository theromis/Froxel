//! Depth maps of orthographic views: where the surface lies along each ray
//! of a view, for MCP `carve_from_views` (a view's `depth`) and
//! `fit_to_views` (a view's depth target).
//!
//! A depth map is a grey image cut like a view's mask: white is near the
//! viewer, black far. A generated depth sheet or a monocular depth model's
//! output is relative (affine at best), so each view's map is calibrated to
//! cells:
//!
//! - `hull` (the default): the nearest pixel of each image row is where
//!   the silhouette hull is nearest in that row, when another view looks
//!   along the row (a side view measures a front view's rows, a top view
//!   its columns). Those pairs give `near` and `far` by least squares,
//!   so a sphere's rim lands on its middle plane, not on the box's back;
//! - `box`: the map's darkest and lightest inked pixels span the box along
//!   the view's depth axis, which suits a relief on a flat back;
//! - `near` and `far` given: the cells from the camera side of the box
//!   where white and black land.
//!
//! `invert` swaps black and white (a far-is-white map), and `gamma` shapes
//! the grey (`value^gamma`) before any of these. A cell on a ray stays
//! when its centre is at or behind the depth surface less `depthTolerance`
//! cells; with `back`, also at or in front of the surface mirrored about the
//! middle of the silhouette hull along the view (where a round body's rim
//! lies), for a view whose opposite has no depth map.

use super::silhouette::{mask_from_rgba, Background, Crop, Ink, Mask};

/// Grey values per pixel, row-major with y down: 0..1, 1 nearest, `None`
/// background.
#[derive(Clone, Debug, PartialEq)]
pub struct DepthMap {
    pub width: u32,
    pub height: u32,
    pub values: Vec<Option<f32>>,
}

impl DepthMap {
    pub fn get(&self, x: u32, y: u32) -> Option<f32> {
        self.values[(y * self.width + x) as usize]
    }

    /// The same map mirrored left to right.
    pub fn flipped(&self) -> DepthMap {
        let mut values = Vec::with_capacity(self.values.len());
        for y in 0..self.height {
            for x in (0..self.width).rev() {
                values.push(self.get(x, y));
            }
        }
        DepthMap {
            width: self.width,
            height: self.height,
            values,
        }
    }

    /// The pixels with a value, as an uncoloured mask: a view's silhouette
    /// when it has no colour image.
    pub fn mask(&self) -> Mask {
        Mask {
            width: self.width,
            height: self.height,
            pixels: self.values.iter().map(|v| v.map(|_| Ink::Plain)).collect(),
        }
    }

    /// The area-weighted mean over `[x0, x1) x [y0, y1)`, in shares 0..1 of
    /// the map's width and height; `None` when less than half of the area
    /// has a value.
    pub fn sample(&self, x0: f64, x1: f64, y0: f64, y1: f64) -> Option<f32> {
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        let (x0, x1, y0, y1) = (x0 * w, x1 * w, y0 * h, y1 * h);
        let area = (x1 - x0) * (y1 - y0);
        if area <= 0.0 || x1 <= 0.0 || y1 <= 0.0 || x0 >= w || y0 >= h {
            return None;
        }
        let (mut sum, mut weight) = (0.0f64, 0.0f64);
        let mut py = y0.max(0.0).floor();
        while py < y1.min(h) {
            let wy = (y1.min(py + 1.0) - y0.max(py)).max(0.0);
            let mut px = x0.max(0.0).floor();
            while px < x1.min(w) {
                let wx = (x1.min(px + 1.0) - x0.max(px)).max(0.0);
                if let Some(v) = self.get(px as u32, py as u32) {
                    sum += f64::from(v) * wx * wy;
                    weight += wx * wy;
                }
                px += 1.0;
            }
            py += 1.0;
        }
        (weight > 0.0 && weight >= area * 0.5).then(|| (sum / weight) as f32)
    }
}

/// The grey of `crop` of an RGBA image, its luma. `background` keys out
/// background pixels; `None` gives every pixel a value, for a map whose
/// silhouette comes from the view's colour image.
pub fn depth_from_rgba(
    width: u32,
    height: u32,
    rgba: &[u8],
    crop: Option<Crop>,
    background: Option<Background>,
) -> Result<DepthMap, String> {
    let rule = background.unwrap_or(Background::Alpha { threshold: 1 });
    let mask = mask_from_rgba(width, height, rgba, crop, rule)?;
    let [cx, cy, _, _] = crop.unwrap_or([0, 0, width, height]);
    let mut values = Vec::with_capacity(mask.pixels.len());
    for y in 0..mask.height {
        for x in 0..mask.width {
            let keep = background.is_none() || mask.inked(x, y);
            let i = (((cy + y) * width + cx + x) * 4) as usize;
            let luma = 0.2126 * f32::from(rgba[i])
                + 0.7152 * f32::from(rgba[i + 1])
                + 0.0722 * f32::from(rgba[i + 2]);
            values.push(keep.then_some((luma / 255.0).clamp(0.0, 1.0)));
        }
    }
    Ok(DepthMap {
        width: mask.width,
        height: mask.height,
        values,
    })
}

/// A map from rows of numbers 0..1 (1 nearest) or `None` (background), as a
/// Rhai script passes one.
pub fn depth_from_values(rows: &[Vec<Option<f32>>]) -> Result<DepthMap, String> {
    let height = rows.len();
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    if width == 0 || height == 0 {
        return Err("depth values must hold at least one number".into());
    }
    if width > 4096 || height > 4096 {
        return Err("depth values are at most 4096 by 4096".into());
    }
    let mut values = vec![None; width * height];
    for (y, row) in rows.iter().enumerate() {
        for (x, v) in row.iter().enumerate() {
            if let Some(v) = v {
                if !(0.0..=1.0).contains(v) {
                    return Err("depth values must be 0..1 (1 nearest) or null".into());
                }
                values[y * width + x] = Some(*v);
            }
        }
    }
    Ok(DepthMap {
        width: width as u32,
        height: height as u32,
        values,
    })
}

/// How a relative map is calibrated to cells when `near` and `far` are not
/// given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Calibrate {
    /// From the silhouette hull's nearest cell per image row and column.
    Hull,
    /// The map's range spans the box along the view's depth axis.
    Box,
}

impl Calibrate {
    pub fn parse(name: &str) -> Result<Calibrate, String> {
        match name {
            "hull" => Ok(Calibrate::Hull),
            "box" => Ok(Calibrate::Box),
            other => Err(format!("calibrate must be hull or box, not `{other}`")),
        }
    }
}

/// One view's depth map and how to read it.
#[derive(Clone, Debug, PartialEq)]
pub struct Depth {
    pub map: DepthMap,
    /// Black is near.
    pub invert: bool,
    /// The grey is raised to this power before calibration.
    pub gamma: f32,
    /// Cells from the camera side of the box where white and black land.
    pub near_far: Option<[f32; 2]>,
    pub calibrate: Calibrate,
    /// Mirror the surface about the rim for the far side.
    pub back: bool,
}

impl Depth {
    pub fn new(map: DepthMap) -> Depth {
        Depth {
            map,
            invert: false,
            gamma: 1.0,
            near_far: None,
            calibrate: Calibrate::Hull,
            back: false,
        }
    }

    pub fn check(&self) -> Result<(), String> {
        if !(self.gamma > 0.0 && self.gamma <= 10.0) {
            return Err("depth gamma must be above 0, at most 10".into());
        }
        if let Some([near, far]) = self.near_far {
            if !near.is_finite() || !far.is_finite() || (near - far).abs() < 1e-3 {
                return Err("depth near and far must be finite and differ".into());
            }
        }
        if !self.map.values.iter().any(Option::is_some) {
            return Err(
                "the depth map has no pixel with a value: check crop and background".into(),
            );
        }
        Ok(())
    }

    /// A grey value after `invert` and `gamma`: 1 nearest.
    pub fn shaped(&self, grey: f32) -> f32 {
        let v = if self.invert { 1.0 - grey } else { grey };
        v.clamp(0.0, 1.0).powf(self.gamma)
    }
}

/// A view's depth in cells: shaped grey `n` lies at `near + (far - near) *
/// (1 - n)` cells from the camera side of the box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Calibration {
    pub near: f32,
    pub far: f32,
    /// `given`, `hull`, or `box`.
    pub mode: &'static str,
    /// Hull calibration: the row and column pairs fitted, and their RMS
    /// error in cells.
    pub pairs: usize,
    pub residual: Option<f32>,
    /// The middle of the silhouette hull along the view's axis: the
    /// mirror plane of `back`.
    pub rim: f32,
}

impl Calibration {
    pub fn surface(&self, n: f32) -> f32 {
        self.near + (self.far - self.near) * (1.0 - n)
    }
}

/// The cells, as steps from the camera side, a ray keeps: centres in
/// `[lo, hi]`.
pub fn kept_span(depth: &Depth, cal: &Calibration, n: f32, tolerance: f32) -> (f32, f32) {
    let s = cal.surface(n);
    let hi = if depth.back {
        2.0 * cal.rim - s + tolerance
    } else {
        f32::INFINITY
    };
    (s - tolerance, hi)
}

/// Calibrate one view. `n` (shaped values) and `hull` (the hull's first
/// cell, in steps from the camera side) are per ray of a `w x h` face,
/// index `row * w + col`; `steps` is the box's depth along the view.
/// `lines` says whether another view measures the face's rows (each at one
/// `row`) and its columns. `middle` is the middle of the hull along the
/// view, in steps.
#[allow(clippy::too_many_arguments)]
pub fn calibrate(
    depth: &Depth,
    n: &[Option<f32>],
    hull: &[Option<f32>],
    w: usize,
    h: usize,
    steps: usize,
    lines: [bool; 2],
    middle: f32,
) -> Calibration {
    let (lo, hi) = n
        .iter()
        .flatten()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| {
            (lo.min(*v), hi.max(*v))
        });
    let finish = |near: f32, far: f32, mode, pairs, residual| Calibration {
        near,
        far,
        mode,
        pairs,
        residual,
        rim: middle,
    };
    if let Some([near, far]) = depth.near_far {
        return finish(near, far, "given", 0, None);
    }
    let boxed = || {
        let last = steps.saturating_sub(1) as f32;
        if !hi.is_finite() || hi <= lo + 1e-6 {
            return finish(0.0, 0.0, "box", 0, None);
        }
        // `hi` lands on step 0 and `lo` on the last step.
        let span = last / (hi - lo);
        let near = -span * (1.0 - hi);
        finish(near, near + span, "box", 0, None)
    };
    if depth.calibrate == Calibrate::Box {
        return boxed();
    }
    // Per line: the nearest value, the hull's nearest cell, and the rays
    // the line holds. A short line (near a pole of the silhouette) crosses
    // a steep surface, where a cell's mean value is far from its nearest,
    // so only lines at least half as long as the longest count.
    let mut found: Vec<(f64, f64, usize)> = Vec::new();
    let mut line = |cells: &mut dyn Iterator<Item = usize>| {
        let mut best: Option<(f32, f32)> = None;
        let mut length = 0;
        for i in cells {
            if let (Some(v), Some(f)) = (n[i], hull[i]) {
                length += 1;
                best = Some(match best {
                    None => (v, f),
                    Some((bv, bf)) => (bv.max(v), bf.min(f)),
                });
            }
        }
        if let Some((v, f)) = best {
            found.push((f64::from(v), f64::from(f), length));
        }
    };
    if lines[0] {
        for row in 0..h {
            line(&mut (0..w).map(|col| row * w + col));
        }
    }
    if lines[1] {
        for col in 0..w {
            line(&mut (0..h).map(|row| row * w + col));
        }
    }
    let longest = found.iter().map(|l| l.2).max().unwrap_or(0);
    let mut pairs: Vec<(f64, f64)> = found
        .iter()
        .filter(|l| l.2 * 2 >= longest)
        .map(|l| (l.0, l.1))
        .collect();
    // Least squares `f = a + b * (1 - v)`: a is near, a + b far.
    let solve = |pairs: &[(f64, f64)]| -> Option<(f64, f64)> {
        if pairs.len() < 3 {
            return None;
        }
        let k = pairs.len() as f64;
        let mx = pairs.iter().map(|(v, _)| 1.0 - v).sum::<f64>() / k;
        let my = pairs.iter().map(|(_, f)| f).sum::<f64>() / k;
        let sxx: f64 = pairs.iter().map(|(v, _)| (1.0 - v - mx).powi(2)).sum();
        let sxy: f64 = pairs.iter().map(|(v, f)| (1.0 - v - mx) * (f - my)).sum();
        if sxx < 1e-6 * k {
            return None;
        }
        let b = sxy / sxx;
        (b > 1e-3).then_some((my - b * mx, b))
    };
    let residuals = |(a, b): (f64, f64), pairs: &[(f64, f64)]| -> Vec<f64> {
        pairs
            .iter()
            .map(|(v, f)| (a + b * (1.0 - v) - f).abs())
            .collect()
    };
    let Some(mut fit) = solve(&pairs) else {
        return boxed();
    };
    // One trimming pass: drop the pairs far off the line and refit.
    let errors = residuals(fit, &pairs);
    let mut sorted = errors.clone();
    sorted.sort_by(f64::total_cmp);
    let cut = (sorted[sorted.len() / 2] * 3.0).max(1.0);
    let kept: Vec<(f64, f64)> = pairs
        .iter()
        .zip(&errors)
        .filter(|(_, e)| **e <= cut)
        .map(|(p, _)| *p)
        .collect();
    if kept.len() < pairs.len() {
        if let Some(refit) = solve(&kept) {
            fit = refit;
            pairs = kept;
        }
    }
    let errors = residuals(fit, &pairs);
    let rms = (errors.iter().map(|e| e * e).sum::<f64>() / errors.len() as f64).sqrt();
    let (a, b) = fit;
    finish(
        a as f32,
        (a + b) as f32,
        "hull",
        pairs.len(),
        Some(rms as f32),
    )
}

/// The views whose image axes are `{a, b}`: they measure a line along the
/// third axis.
pub(super) fn measured(sides: &[super::Side], a: usize, b: usize) -> bool {
    sides.iter().any(|side| {
        let [(u, _), (v, _), _] = side.axes();
        (u == a && v == b) || (u == b && v == a)
    })
}

#[cfg(test)]
#[path = "../../tests/depth.rs"]
mod tests;
