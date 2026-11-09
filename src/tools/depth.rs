//! Depth maps of orthographic views: where the surface lies along each ray
//! of a view, for MCP `carve_from_views` (a view's `depth`) and
//! `fit_to_views` (a view's depth target).
//!
//! A depth map is a grey image cut like a view's mask: white is near the
//! viewer, black far. Coloured pixels (a magenta background and its
//! fringe) hold no depth. A ray reads the median grey of its footprint, so
//! outlines and specks do not move it. A generated depth sheet or a
//! monocular depth model's output is relative, so each view's map is
//! calibrated to cells:
//!
//! - `hull` (the default): the nearest pixel of each image row is where
//!   the silhouette hull is nearest in that row, when another view looks
//!   along the row (a side view measures a front view's rows, a top view
//!   its columns). Those pairs give `near` and `far` by least squares, so a
//!   sphere's rim lands on its middle plane, not on the box's back.
//!   `gamma: "auto"` also picks the curve (`grey^gamma`) with the smallest
//!   error, for a map that is not linear in depth (Depth Anything's
//!   inverse depth);
//! - `box`: the map's darkest and lightest pixels span the box along the
//!   view's depth axis, which suits a relief on a flat back;
//! - `near` and `far` given: the cells where white and black land.
//!
//! A map that normalises each part on its own, as an image generator draws
//! depth like shading, takes `regions`: the map is cut into regions, one
//! per peak (a part's nearest point), apart where the grey jumps or dips
//! between peaks by more than `edge`, and each region is calibrated by
//! the same row and column pairs, restricted to its own rays, so its
//! extent along the axis comes from the other views' silhouettes. A
//! `detail` map combines two maps: the base map (calibrated as above) gives
//! each region of the detail map its offset, the mean of the base surface
//! over the region, and the detail map its curvature, its scale fitted
//! per region against the hull. So a monocular model's map, right in order
//! but flat inside each part, and a generated map, round inside each part
//! but wrong in order, give one surface.
//!
//! `invert` swaps black and white. A cell on a ray stays when its centre is
//! at or behind the depth surface less `depthTolerance` cells; with `back`,
//! also at or in front of the surface mirrored about the middle of the
//! silhouette hull along the view (where a round body's rim lies), for a
//! view whose opposite has no depth map.

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

    /// Each pixel under `rect` (`[x0, x1, y0, y1]` in shares 0..1 of the
    /// map's width and height) with the share of it covered, and the
    /// rectangle's area in pixels.
    fn footprint(&self, rect: [f64; 4], mut each: impl FnMut(u32, u32, f64)) -> f64 {
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        let (x0, x1, y0, y1) = (rect[0] * w, rect[1] * w, rect[2] * h, rect[3] * h);
        let area = (x1 - x0) * (y1 - y0);
        if area <= 0.0 || x1 <= 0.0 || y1 <= 0.0 || x0 >= w || y0 >= h {
            return 0.0;
        }
        let mut py = y0.max(0.0).floor();
        while py < y1.min(h) {
            let wy = (y1.min(py + 1.0) - y0.max(py)).max(0.0);
            let mut px = x0.max(0.0).floor();
            while px < x1.min(w) {
                let wx = (x1.min(px + 1.0) - x0.max(px)).max(0.0);
                each(px as u32, py as u32, wx * wy);
                px += 1.0;
            }
            py += 1.0;
        }
        area
    }

    /// The area-weighted median grey over `rect`; `None` when less than
    /// half of the area has a value.
    pub fn sample(&self, rect: [f64; 4]) -> Option<f32> {
        self.sample_in(rect, None)
    }

    /// [`Self::sample`] over the pixels of one region only, when given, so
    /// a ray across a part's edge reads its own part.
    fn sample_in(&self, rect: [f64; 4], region: Option<(&[Option<u32>], u32)>) -> Option<f32> {
        let mut found: Vec<(f32, f64)> = Vec::new();
        let mut any = 0.0;
        let area = self.footprint(rect, |x, y, w| {
            let i = (y * self.width + x) as usize;
            if let Some(v) = self.values[i] {
                any += w;
                if region.is_none_or(|(labels, label)| labels[i] == Some(label)) {
                    found.push((v, w));
                }
            }
        });
        let weight: f64 = found.iter().map(|(_, w)| w).sum();
        if area <= 0.0 || weight <= 0.0 || any < area * 0.5 {
            return None;
        }
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut sum = 0.0;
        for (v, w) in &found {
            sum += w;
            if sum >= weight / 2.0 {
                return Some(*v);
            }
        }
        found.last().map(|(v, _)| *v)
    }

    /// Regions: each is one peak of the map (a part's nearest point) and
    /// the pixels that climb to it, a watershed. Two peaks stay apart when
    /// the valley between them is deeper than `edge` below the lower peak,
    /// and neighbours further apart than `edge` never join, so a part
    /// drawn with its own range (an image generator's habit) is a region
    /// even where its rim fades into what lies behind it, while a speck or
    /// a block step is not.
    pub fn segment(&self, edge: f32) -> Vec<Option<u32>> {
        let (w, h) = (self.width as usize, self.height as usize);
        let mut order: Vec<usize> = (0..w * h).filter(|i| self.values[*i].is_some()).collect();
        let value = |i: usize| self.values[i].expect("valued pixels");
        order.sort_by(|a, b| value(*b).total_cmp(&value(*a)).then(a.cmp(b)));
        let mut parent: Vec<usize> = Vec::new();
        let mut peak: Vec<f32> = Vec::new();
        let mut of: Vec<Option<usize>> = vec![None; w * h];
        fn find(parent: &mut [usize], mut c: usize) -> usize {
            while parent[c] != c {
                parent[c] = parent[parent[c]];
                c = parent[c];
            }
            c
        }
        for &i in &order {
            let v = value(i);
            let (x, y) = (i % w, i / w);
            let mut roots: Vec<usize> = Vec::new();
            let around = [
                (x > 0).then(|| i - 1),
                (x + 1 < w).then(|| i + 1),
                (y > 0).then(|| i - w),
                (y + 1 < h).then(|| i + w),
            ];
            for j in around.into_iter().flatten() {
                if let Some(c) = of[j] {
                    if (value(j) - v).abs() <= edge {
                        let root = find(&mut parent, c);
                        if !roots.contains(&root) {
                            roots.push(root);
                        }
                    }
                }
            }
            if roots.is_empty() {
                parent.push(parent.len());
                peak.push(v);
                of[i] = Some(parent.len() - 1);
                continue;
            }
            roots.sort_by(|a, b| peak[*b].total_cmp(&peak[*a]).then(a.cmp(b)));
            let top = roots[0];
            of[i] = Some(top);
            for &r in &roots[1..] {
                if peak[r] - v < edge {
                    parent[r] = top;
                }
            }
        }
        let mut compact: std::collections::HashMap<usize, u32> = Default::default();
        (0..w * h)
            .map(|i| {
                let root = find(&mut parent, of[i]?);
                let next = compact.len() as u32;
                Some(*compact.entry(root).or_insert(next))
            })
            .collect()
    }

    /// The region covering most of `rect`.
    fn label(&self, labels: &[Option<u32>], rect: [f64; 4]) -> Option<u32> {
        let mut tally: Vec<(u32, f64)> = Vec::new();
        self.footprint(rect, |x, y, w| {
            if let Some(label) = labels[(y * self.width + x) as usize] {
                match tally.iter_mut().find(|(l, _)| *l == label) {
                    Some(found) => found.1 += w,
                    None => tally.push((label, w)),
                }
            }
        });
        tally
            .into_iter()
            .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
            .map(|(label, _)| label)
    }
}

/// Saturation (largest minus smallest channel) from which a pixel is a
/// colour, not a grey, and holds no depth.
pub const DEPTH_MAX_CHROMA: u8 = 48;

/// The grey of `crop` of an RGBA image, its luma. Coloured pixels (a key
/// background, its fringe) hold no value. `background` keys out other
/// background pixels; `None` keys only the colours, for a map whose
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
            let i = (((cy + y) * width + cx + x) * 4) as usize;
            let [r, g, b] = [rgba[i], rgba[i + 1], rgba[i + 2]];
            let grey = r.max(g).max(b) - r.min(g).min(b) < DEPTH_MAX_CHROMA;
            let keep = grey && (background.is_none() || mask.inked(x, y));
            let luma = 0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b);
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

/// The curve a grey value is raised by before calibration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gamma {
    Fixed(f32),
    /// The exponent of [`AUTO_GAMMAS`] whose hull calibration fits best.
    Auto,
}

/// The exponents `gamma: "auto"` tries.
pub const AUTO_GAMMAS: [f32; 9] = [0.25, 0.35, 0.5, 0.7, 1.0, 1.4, 2.0, 2.8, 4.0];

/// Rays a region needs to be calibrated on its own.
pub const MIN_REGION_RAYS: usize = 6;

/// One view's depth map and how to read it.
#[derive(Clone, Debug, PartialEq)]
pub struct Depth {
    pub map: DepthMap,
    /// Black is near.
    pub invert: bool,
    pub gamma: Gamma,
    /// Cells from the camera side of the box where white and black land.
    pub near_far: Option<[f32; 2]>,
    pub calibrate: Calibrate,
    /// Mirror the surface about the hull's middle for the far side.
    pub back: bool,
    /// Calibrate each region of the map on its own against the hull.
    pub regions: bool,
    /// The grey step (0..1) that separates two regions: a jump between
    /// neighbouring pixels, or a valley that deep between two peaks.
    pub edge: f32,
    /// A second map for the curvature inside each of its regions; this map
    /// gives their offsets. Only its map, `invert`, `gamma`, and `edge`
    /// count.
    pub detail: Option<Box<Depth>>,
}

impl Depth {
    pub fn new(map: DepthMap) -> Depth {
        Depth {
            map,
            invert: false,
            gamma: Gamma::Fixed(1.0),
            near_far: None,
            calibrate: Calibrate::Hull,
            back: false,
            regions: false,
            edge: 0.06,
            detail: None,
        }
    }

    pub fn check(&self) -> Result<(), String> {
        if let Gamma::Fixed(g) = self.gamma {
            if !(g > 0.0 && g <= 10.0) {
                return Err("depth gamma must be above 0, at most 10, or auto".into());
            }
        }
        if !(self.edge > 0.0 && self.edge <= 1.0) {
            return Err("depth edge must be above 0, at most 1".into());
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
        if let Some(detail) = &self.detail {
            if detail.gamma == Gamma::Auto {
                return Err("a detail map's gamma must be a number".into());
            }
            detail.check().map_err(|err| format!("detail: {err}"))?;
        }
        Ok(())
    }

    /// A ray's grey after `invert`.
    fn grey(&self, rect: Option<[f64; 4]>) -> Option<f32> {
        let g = self.map.sample(rect?)?;
        Some(if self.invert { 1.0 - g } else { g })
    }
}

fn shape(grey: f32, gamma: f32) -> f32 {
    grey.clamp(0.0, 1.0).powf(gamma)
}

/// How a view's map was calibrated: shaped grey `n` lies at `near + (far -
/// near) * (1 - n)` cells from the camera side of the box, unless regions
/// calibrate parts of it on their own.
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
    /// The exponent used.
    pub gamma: f32,
    /// The middle of the silhouette hull along the view's axis: the
    /// mirror plane of `back`.
    pub rim: f32,
    /// Regions (with `regions` or `detail`): how many were calibrated on
    /// their own, and the RMS error of their fits in cells.
    pub regions: usize,
    pub region_residual: Option<f32>,
}

/// One view's rays, for [`surfaces`]: a `w x h` face, index
/// `row * w + col`, rows along image rows.
pub struct Rays<'a> {
    pub w: usize,
    pub h: usize,
    /// The box's depth along the view.
    pub steps: usize,
    /// Another view measures the face's rows, and its columns.
    pub lines: [bool; 2],
    /// The middle of the hull along the view, in steps.
    pub middle: f32,
    /// The hull's first cell per ray, in steps from the camera side.
    pub hull: &'a [Option<f32>],
    /// The hull's last cell per ray.
    pub hull_back: &'a [Option<f32>],
    /// Each ray's footprint on the view's image, `[x0, x1, y0, y1]` in
    /// shares 0..1; `None` outside the silhouette.
    pub rects: &'a [Option<[f64; 4]>],
}

/// The cells, as steps from the camera side, a ray whose surface is `s`
/// keeps: centres in `[lo, hi]`.
pub fn kept_span(back: bool, rim: f32, s: f32, tolerance: f32) -> (f32, f32) {
    let hi = if back {
        2.0 * rim - s + tolerance
    } else {
        f32::INFINITY
    };
    (s - tolerance, hi)
}

/// A least-squares fit of `hull = a + b * (1 - n)` over the rows and
/// columns of `rays` (those `lines` allows) whose rays `keep` takes: per
/// line, the nearest value against the hull's nearest cell. Lines shorter
/// than half the longest are left out: a short line (near a pole) crosses
/// a steep surface, where a ray's value is far from its nearest point.
/// Returns `a`, `b`, the pairs used, and their RMS error.
fn fit_lines(
    n: &[Option<f32>],
    rays: &Rays,
    keep: &dyn Fn(usize) -> bool,
) -> Option<(f64, f64, usize, f64)> {
    let (w, h) = (rays.w, rays.h);
    let mut found: Vec<(f64, f64, usize)> = Vec::new();
    let mut line = |cells: &mut dyn Iterator<Item = usize>| {
        let mut best: Option<(f32, f32)> = None;
        let mut length = 0;
        for i in cells {
            if !keep(i) {
                continue;
            }
            if let (Some(v), Some(f)) = (n[i], rays.hull[i]) {
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
    if rays.lines[0] {
        for row in 0..h {
            line(&mut (0..w).map(|col| row * w + col));
        }
    }
    if rays.lines[1] {
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
    // When the long lines alone say nothing (they all meet one flat part),
    // the short ones count too.
    let mut fit = match solve(&pairs) {
        Some(fit) => fit,
        None => {
            pairs = found.iter().map(|l| (l.0, l.1)).collect();
            solve(&pairs)?
        }
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
    Some((fit.0, fit.1, pairs.len(), rms))
}

/// The depth surface of every ray of one view, in steps from the camera
/// side (`None` where the map has no value), and how it was calibrated.
pub fn surfaces(depth: &Depth, rays: &Rays) -> (Vec<Option<f32>>, Calibration) {
    let count = rays.w * rays.h;
    let grey: Vec<Option<f32>> = rays.rects.iter().map(|r| depth.grey(*r)).collect();
    let all = |_: usize| true;
    // The curve, and the global calibration.
    let mut best: Option<(f32, (f64, f64, usize, f64))> = None;
    if depth.near_far.is_none() && depth.calibrate == Calibrate::Hull {
        let gammas: Vec<f32> = match depth.gamma {
            Gamma::Fixed(g) => vec![g],
            Gamma::Auto => AUTO_GAMMAS.to_vec(),
        };
        // The rows' nearest values span only the near part of the curve, so
        // exponents fit them almost alike. The curve is chosen by the
        // silhouette's own contour instead: there a round body's surface
        // turns away at the middle of the hull along the ray. Each
        // exponent is scored by the median miss over the contour rays.
        let rim = contour(&grey, rays);
        let mut fits = Vec::new();
        for g in gammas {
            let n: Vec<Option<f32>> = grey.iter().map(|v| v.map(|v| shape(v, g))).collect();
            if let Some(fit) = fit_lines(&n, rays, &all) {
                let mut misses: Vec<f64> = rim
                    .iter()
                    .map(|(v, mid)| (fit.0 + fit.1 * (1.0 - f64::from(shape(*v, g))) - mid).abs())
                    .collect();
                misses.sort_by(f64::total_cmp);
                let score = misses.get(misses.len() / 2).copied().unwrap_or(fit.3);
                fits.push((g, fit, score));
            }
        }
        // Of the exponents within a tenth of a cell of the best, the one
        // closest to linear.
        let least = fits.iter().map(|f| f.2).fold(f64::INFINITY, f64::min);
        best = fits
            .into_iter()
            .filter(|f| f.2 <= least + 0.1)
            .min_by(|a, b| a.0.ln().abs().total_cmp(&b.0.ln().abs()))
            .map(|(g, fit, _)| (g, fit));
    }
    let gamma = match (best, depth.gamma) {
        (Some((g, _)), _) => g,
        (None, Gamma::Fixed(g)) => g,
        (None, Gamma::Auto) => 1.0,
    };
    let n: Vec<Option<f32>> = grey.iter().map(|v| v.map(|v| shape(v, gamma))).collect();
    let mut cal = Calibration {
        near: 0.0,
        far: 0.0,
        mode: "box",
        pairs: 0,
        residual: None,
        gamma,
        rim: rays.middle,
        regions: 0,
        region_residual: None,
    };
    if let Some([near, far]) = depth.near_far {
        (cal.near, cal.far, cal.mode) = (near, far, "given");
    } else if let Some((_, (a, b, pairs, rms))) = best {
        (cal.near, cal.far, cal.mode) = (a as f32, (a + b) as f32, "hull");
        (cal.pairs, cal.residual) = (pairs, Some(rms as f32));
    } else {
        // Box: the lightest value lands on step 0, the darkest on the last.
        let (lo, hi) = n
            .iter()
            .flatten()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| {
                (lo.min(*v), hi.max(*v))
            });
        if hi.is_finite() && hi > lo + 1e-6 {
            let span = rays.steps.saturating_sub(1) as f32 / (hi - lo);
            cal.near = -span * (1.0 - hi);
            cal.far = cal.near + span;
        }
    }
    let base: Vec<Option<f32>> = n
        .iter()
        .map(|v| v.map(|v| cal.near + (cal.far - cal.near) * (1.0 - v)))
        .collect();
    if depth.detail.is_none() && !depth.regions {
        return (base, cal);
    }
    // The map whose regions are calibrated, its values, and its regions.
    let (shape_map, shape_n, edge) = match &depth.detail {
        Some(detail) => {
            let g = match detail.gamma {
                Gamma::Fixed(g) => g,
                Gamma::Auto => 1.0,
            };
            let n: Vec<Option<f32>> = rays
                .rects
                .iter()
                .map(|r| detail.grey(*r).map(|v| shape(v, g)))
                .collect();
            (&detail.map, n, detail.edge)
        }
        None => (&depth.map, n.clone(), depth.edge),
    };
    let pixels = shape_map.segment(edge);
    let mut labels: Vec<Option<u32>> = (0..count)
        .map(|i| {
            shape_n[i]?;
            shape_map.label(&pixels, rays.rects[i]?)
        })
        .collect();
    // Each ray reads the pixels of its own region.
    let (shape_invert, shape_gamma) = match &depth.detail {
        Some(detail) => (
            detail.invert,
            match detail.gamma {
                Gamma::Fixed(g) => g,
                Gamma::Auto => 1.0,
            },
        ),
        None => (depth.invert, gamma),
    };
    let shape_n: Vec<Option<f32>> = (0..count)
        .map(|i| {
            let g = shape_map.sample_in(rays.rects[i]?, Some((&pixels, labels[i]?)))?;
            Some(shape(if shape_invert { 1.0 - g } else { g }, shape_gamma))
        })
        .collect();
    merge_small(&mut labels, &shape_n, rays.w, rays.h);
    let global_b = match &depth.detail {
        Some(_) => fit_lines(&shape_n, rays, &all).map(|f| f.1),
        None => None,
    }
    .unwrap_or(f64::from(cal.far - cal.near));
    let mut members: std::collections::BTreeMap<u32, Vec<usize>> = Default::default();
    for (i, label) in labels.iter().enumerate() {
        if let Some(label) = label {
            members.entry(*label).or_default().push(i);
        }
    }
    let mut out = base.clone();
    let (mut sq, mut fitted) = (0.0f64, 0usize);
    for (label, rays_in) in &members {
        if rays_in.len() < MIN_REGION_RAYS {
            continue;
        }
        let inside = |i: usize| labels[i] == Some(*label);
        let fit = fit_lines(&shape_n, rays, &inside);
        let b = fit.map_or(global_b, |f| f.1);
        let a = if depth.detail.is_some() {
            // The offset: the base surface's mean over the region.
            let (mut sum, mut k) = (0.0f64, 0usize);
            for &i in rays_in {
                if let (Some(s), Some(v)) = (base[i], shape_n[i]) {
                    sum += f64::from(s) - b * (1.0 - f64::from(v));
                    k += 1;
                }
            }
            if k == 0 {
                continue;
            }
            sum / k as f64
        } else if let Some((a, _, _, _)) = fit {
            a
        } else {
            // No fit: the region's nearest value meets its nearest hull cell.
            let nearest = rays_in
                .iter()
                .filter_map(|i| shape_n[*i])
                .fold(0.0f32, f32::max);
            let Some(front) = rays_in
                .iter()
                .filter_map(|i| rays.hull[*i])
                .reduce(f32::min)
            else {
                continue;
            };
            f64::from(front) - b * (1.0 - f64::from(nearest))
        };
        if let Some((_, _, _, rms)) = fit {
            sq += rms * rms;
            fitted += 1;
        }
        cal.regions += 1;
        for &i in rays_in {
            if let Some(v) = shape_n[i] {
                out[i] = Some((a + b * (1.0 - f64::from(v))) as f32);
            }
        }
    }
    if fitted > 0 {
        cal.region_residual = Some((sq / fitted as f64).sqrt() as f32);
    }
    (out, cal)
}

/// The rays on the view's silhouette contour (inked, beside a ray with no
/// value or the face's edge), each with its grey and the middle of the
/// hull along it.
fn contour(grey: &[Option<f32>], rays: &Rays) -> Vec<(f32, f64)> {
    let (w, h) = (rays.w, rays.h);
    let mut out = Vec::new();
    for i in 0..w * h {
        let (Some(v), Some(a), Some(b)) = (grey[i], rays.hull[i], rays.hull_back[i]) else {
            continue;
        };
        let (x, y) = (i % w, i / w);
        let edge = x == 0
            || y == 0
            || x + 1 == w
            || y + 1 == h
            || [i - 1, i + 1, i - w, i + w]
                .into_iter()
                .any(|j| grey[j].is_none());
        if edge {
            out.push((v, (f64::from(a) + f64::from(b) + 1.0) / 2.0));
        }
    }
    out
}

/// Rays of regions under [`MIN_REGION_RAYS`] (a steep rim, an outline,
/// a speck) join the big region of the neighbour whose value is closest,
/// a few rays deep: a part's rim continues the part, not what lies behind.
fn merge_small(labels: &mut [Option<u32>], n: &[Option<f32>], w: usize, h: usize) {
    for _ in 0..8 {
        let mut size: std::collections::HashMap<u32, usize> = Default::default();
        for label in labels.iter().flatten() {
            *size.entry(*label).or_default() += 1;
        }
        let big = |label: Option<u32>| label.filter(|l| size[l] >= MIN_REGION_RAYS);
        let mut changed = false;
        let before = labels.to_vec();
        for i in 0..w * h {
            if before[i].is_none() || big(before[i]).is_some() {
                continue;
            }
            let (x, y) = (i % w, i / w);
            let mut best: Option<(f32, u32)> = None;
            let around = [
                (x > 0).then(|| i - 1),
                (x + 1 < w).then(|| i + 1),
                (y > 0).then(|| i - w),
                (y + 1 < h).then(|| i + w),
            ];
            for j in around.into_iter().flatten() {
                if let (Some(label), Some(a), Some(b)) = (big(before[j]), n[i], n[j]) {
                    let gap = (a - b).abs();
                    if best.is_none_or(|(g, l)| gap < g || (gap == g && label < l)) {
                        best = Some((gap, label));
                    }
                }
            }
            if let Some((_, label)) = best {
                labels[i] = Some(label);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
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
