//! MCP `measure_sheet`: read a views sheet before carving or fitting it.
//!
//! - **Views**: the inked pixels (a [`Mask`] of the whole sheet under its
//!   background rule) split into 8-connected regions; regions whose boxes
//!   lie within `gap` pixels of each other merge (a part drawn apart from
//!   its view), and regions under `min_share` of the largest are left out
//!   (labels, specks). Each view's crop is its tight box.
//! - **Layout**: four views in two rows and two columns take the names of
//!   `layout` in reading order (front, side, top, back by default).
//! - **Pitch**: the cube size in pixels per view, from the seams between
//!   cubes: the mean brightness step between neighbouring inked pixels per
//!   column (and per row) repeats every cube, so the profile's
//!   autocorrelation peaks at the pitch. The fundamental is the shortest
//!   strong peak; it is refined from the furthest multiple that is still a
//!   peak, to a fraction of a pixel.
//! - **Checks**: front height = side height, front width = top width, and
//!   side length = top length, in pixels and in cubes.
//! - **Parts**: the pieces of one view (the top view by default) outside
//!   its body, where the body is the largest region left after an opening
//!   (erode, then grow back) by a radius of half the view's thickest point,
//!   reaching more than 0.42 of the radius past it (an opened box's corner
//!   reaches a third): flippers, legs, a head, and a tail each count once.

use std::collections::VecDeque;

use super::silhouette::Mask;

/// Most views one sheet may hold.
pub const MAX_SHEET_VIEWS: usize = 32;

/// How to read a sheet.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetSpec {
    /// Pixels between two regions' boxes that still merge them.
    pub gap: u32,
    /// A region under this share of the largest region's ink is left out.
    pub min_share: f32,
    /// Names for a 2 x 2 layout, in reading order.
    pub layout: [String; 4],
    /// Shortest and longest pitch searched, in pixels.
    pub pitch_range: [u32; 2],
    /// Count the parts of this view (a layout name, or an index).
    pub parts: Option<PartsSpec>,
    /// Largest difference, in percent, a check passes with.
    pub tolerance: f32,
}

impl Default for SheetSpec {
    fn default() -> Self {
        Self {
            gap: 12,
            min_share: 0.02,
            layout: ["front", "side", "top", "back"].map(str::to_string),
            pitch_range: [4, 96],
            parts: None,
            tolerance: 3.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PartsSpec {
    pub view: String,
    /// Opening radius in pixels; `None` is half the view's thickest point.
    pub radius: Option<f32>,
    /// A part under this share of the view's ink is left out.
    pub min_share: f32,
}

impl Default for PartsSpec {
    fn default() -> Self {
        Self {
            view: "top".into(),
            radius: None,
            min_share: 0.004,
        }
    }
}

/// The repeat of the cube seams along one axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisPitch {
    pub pitch: f32,
    /// The autocorrelation at the pitch, 0..1.
    pub strength: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SheetView {
    pub name: Option<String>,
    /// `[x, y, width, height]`, tight on the ink.
    pub crop: [u32; 4],
    pub pixels: usize,
    pub pitch_x: Option<AxisPitch>,
    pub pitch_y: Option<AxisPitch>,
}

impl SheetView {
    /// The pitch of both axes together (their mean when both are found).
    pub fn pitch(&self) -> Option<f32> {
        match (self.pitch_x, self.pitch_y) {
            (Some(a), Some(b)) if (a.pitch - b.pitch).abs() <= 0.15 * a.pitch.max(b.pitch) => {
                Some((a.pitch * a.strength + b.pitch * b.strength) / (a.strength + b.strength))
            }
            (Some(a), Some(b)) => Some(if a.strength >= b.strength {
                a.pitch
            } else {
                b.pitch
            }),
            (Some(a), None) | (None, Some(a)) => Some(a.pitch),
            (None, None) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Check {
    pub name: &'static str,
    pub a: (String, &'static str, f32),
    pub b: (String, &'static str, f32),
    /// `|a - b| / max(a, b)` in percent, in pixels.
    pub percent: f32,
    /// The same in cubes, with each view's own pitch.
    pub percent_cubes: Option<f32>,
    pub cubes: Option<(f32, f32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub bounds: [u32; 4],
    pub pixels: usize,
    /// Compass direction from the body's centre: n (image up), ne, e, ...
    pub direction: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Parts {
    pub view: String,
    pub radius: f32,
    pub body_pixels: usize,
    pub parts: Vec<Part>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    pub views: Vec<SheetView>,
    /// The views were named by the 2 x 2 layout.
    pub layout: bool,
    /// Boxes of the regions left out.
    pub ignored: Vec<[u32; 4]>,
    pub checks: Vec<Check>,
    pub parts: Option<Parts>,
}

/// Read a sheet: `mask` is the whole image's ink, `luma` its brightness
/// (0..255) per pixel, row-major.
pub fn measure_sheet(mask: &Mask, luma: &[f32], spec: &SheetSpec) -> Result<Sheet, String> {
    let (w, h) = (mask.width as usize, mask.height as usize);
    if luma.len() != w * h {
        return Err("luma does not match the mask".into());
    }
    let mut regions = regions(mask);
    if regions.is_empty() {
        return Err("the sheet has no inked pixel under its background rule".into());
    }
    merge_close(&mut regions, spec.gap);
    let largest = regions.iter().map(|r| r.pixels).max().unwrap_or(0);
    let floor = (largest as f32 * spec.min_share).ceil() as usize;
    let (kept, dropped): (Vec<Region>, Vec<Region>) =
        regions.into_iter().partition(|r| r.pixels >= floor.max(1));
    if kept.len() > MAX_SHEET_VIEWS {
        return Err(format!(
            "the sheet has {} views over min share; raise gap or minShare, or check the background",
            kept.len()
        ));
    }
    let mut kept = kept;
    // Reading order: rows by centre, then left to right.
    kept.sort_by(|a, b| a.centre()[1].total_cmp(&b.centre()[1]));
    let mut rows: Vec<Vec<Region>> = Vec::new();
    for region in kept {
        match rows.last_mut() {
            Some(row)
                if row
                    .iter()
                    .any(|r| r.overlaps_rows(&region) || region.overlaps_rows(r)) =>
            {
                row.push(region)
            }
            _ => rows.push(vec![region]),
        }
    }
    for row in &mut rows {
        row.sort_by(|a, b| a.centre()[0].total_cmp(&b.centre()[0]));
    }
    let layout = rows.len() == 2 && rows.iter().all(|row| row.len() == 2) && {
        // Two columns: each left view is left of both right views.
        let left = [&rows[0][0], &rows[1][0]];
        let right = [&rows[0][1], &rows[1][1]];
        left.iter()
            .all(|l| right.iter().all(|r| l.centre()[0] < r.box_[0] as f32))
    };
    let ordered: Vec<Region> = rows.into_iter().flatten().collect();
    let mut views = Vec::with_capacity(ordered.len());
    for (index, region) in ordered.iter().enumerate() {
        let crop = region.crop();
        let (px, py) = seam_profiles(mask, luma, crop);
        let lo = spec.pitch_range[0].max(2) as usize;
        let hi = spec.pitch_range[1] as usize;
        views.push(SheetView {
            name: layout.then(|| spec.layout[index].clone()),
            crop,
            pixels: region.pixels,
            pitch_x: pitch_of(&px, lo, hi),
            pitch_y: pitch_of(&py, lo, hi),
        });
    }
    let checks = if layout { checks(&views) } else { Vec::new() };
    let parts = match &spec.parts {
        None => None,
        Some(parts) => {
            let found = views
                .iter()
                .position(|v| v.name.as_deref() == Some(parts.view.as_str()))
                .or_else(|| parts.view.parse::<usize>().ok().filter(|i| *i < views.len()))
                .ok_or_else(|| {
                    format!(
                        "parts: no view `{}` (name the view by index when the sheet is not a 2 x 2 layout)",
                        parts.view
                    )
                })?;
            Some(count_parts(
                mask,
                &views[found],
                parts,
                views[found].name.clone().unwrap_or(found.to_string()),
            ))
        }
    };
    Ok(Sheet {
        views,
        layout,
        ignored: dropped.iter().map(Region::crop).collect(),
        checks,
        parts,
    })
}

#[derive(Clone, Debug)]
struct Region {
    /// Inclusive `[x0, y0, x1, y1]`.
    box_: [u32; 4],
    pixels: usize,
}

impl Region {
    fn centre(&self) -> [f32; 2] {
        [
            (self.box_[0] + self.box_[2]) as f32 * 0.5,
            (self.box_[1] + self.box_[3]) as f32 * 0.5,
        ]
    }

    /// This region's centre row lies within the other's rows.
    fn overlaps_rows(&self, other: &Region) -> bool {
        let c = self.centre()[1];
        c >= other.box_[1] as f32 && c <= other.box_[3] as f32
    }

    fn crop(&self) -> [u32; 4] {
        [
            self.box_[0],
            self.box_[1],
            self.box_[2] - self.box_[0] + 1,
            self.box_[3] - self.box_[1] + 1,
        ]
    }
}

/// 8-connected inked regions.
fn regions(mask: &Mask) -> Vec<Region> {
    let (w, h) = (mask.width as i64, mask.height as i64);
    let mut seen = vec![false; mask.pixels.len()];
    let mut out = Vec::new();
    let mut queue = VecDeque::new();
    for start in 0..mask.pixels.len() {
        if seen[start] || mask.pixels[start].is_none() {
            continue;
        }
        seen[start] = true;
        queue.push_back(start);
        let mut region = Region {
            box_: [u32::MAX, u32::MAX, 0, 0],
            pixels: 0,
        };
        while let Some(i) = queue.pop_front() {
            let (x, y) = (i as i64 % w, i as i64 / w);
            region.pixels += 1;
            region.box_ = [
                region.box_[0].min(x as u32),
                region.box_[1].min(y as u32),
                region.box_[2].max(x as u32),
                region.box_[3].max(y as u32),
            ];
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w || ny >= h {
                        continue;
                    }
                    let j = (ny * w + nx) as usize;
                    if !seen[j] && mask.pixels[j].is_some() {
                        seen[j] = true;
                        queue.push_back(j);
                    }
                }
            }
        }
        out.push(region);
    }
    out
}

/// Merge regions whose boxes come within `gap` pixels, until none do.
fn merge_close(regions: &mut Vec<Region>, gap: u32) {
    let near = |a: &Region, b: &Region| {
        let dx = (a.box_[0].saturating_sub(b.box_[2])).max(b.box_[0].saturating_sub(a.box_[2]));
        let dy = (a.box_[1].saturating_sub(b.box_[3])).max(b.box_[1].saturating_sub(a.box_[3]));
        dx <= gap + 1 && dy <= gap + 1
    };
    // Small regions first would merge noise into views; big ones lead.
    regions.sort_by(|a, b| b.pixels.cmp(&a.pixels));
    let mut merged = true;
    while merged {
        merged = false;
        'outer: for i in 0..regions.len() {
            for j in (i + 1)..regions.len() {
                if near(&regions[i], &regions[j]) {
                    let other = regions.remove(j);
                    let region = &mut regions[i];
                    region.pixels += other.pixels;
                    region.box_ = [
                        region.box_[0].min(other.box_[0]),
                        region.box_[1].min(other.box_[1]),
                        region.box_[2].max(other.box_[2]),
                        region.box_[3].max(other.box_[3]),
                    ];
                    merged = true;
                    break 'outer;
                }
            }
        }
    }
}

/// The seam profiles of a view: per column (and per row) the mean depth of
/// thin dark lines across it, over inked pixels. A seam between cubes is a
/// line one or two pixels wide darker than both sides; a step from one
/// colour to another is not, so plate patterns and shading add little.
fn seam_profiles(mask: &Mask, luma: &[f32], crop: [u32; 4]) -> (Vec<f32>, Vec<f32>) {
    let w = mask.width as usize;
    let [cx, cy, cw, ch] = crop.map(|v| v as usize);
    let inked = |x: usize, y: usize| mask.pixels[y * w + x].is_some();
    let l = |x: usize, y: usize| luma[y * w + x];
    // Depth of a dark line at `i` along a run of samples `at(k)`.
    let line = |at: &dyn Fn(isize) -> Option<f32>| -> f32 {
        let (Some(a), Some(b), Some(c)) = (at(-1), at(0), at(1)) else {
            return 0.0;
        };
        let one = a.min(c) - b;
        let two = match at(2) {
            Some(d) => a.min(d) - b.max(c),
            None => 0.0,
        };
        one.max(two).clamp(0.0, 40.0)
    };
    let mut px = vec![(0.0f32, 0u32); cw];
    let mut py = vec![(0.0f32, 0u32); ch];
    for y in cy..cy + ch {
        for x in cx..cx + cw {
            if !inked(x, y) {
                continue;
            }
            let across = |k: isize| {
                let nx = x as isize + k;
                (nx >= cx as isize && nx < (cx + cw) as isize && inked(nx as usize, y))
                    .then(|| l(nx as usize, y))
            };
            let down = |k: isize| {
                let ny = y as isize + k;
                (ny >= cy as isize && ny < (cy + ch) as isize && inked(x, ny as usize))
                    .then(|| l(x, ny as usize))
            };
            let slot = &mut px[x - cx];
            slot.0 += line(&across);
            slot.1 += 1;
            let slot = &mut py[y - cy];
            slot.0 += line(&down);
            slot.1 += 1;
        }
    }
    let mean = |v: Vec<(f32, u32)>| -> Vec<f32> {
        v.into_iter()
            .map(|(sum, n)| if n > 0 { sum / n as f32 } else { 0.0 })
            .collect()
    };
    (mean(px), mean(py))
}

/// The pitch of a seam profile: the shortest strong autocorrelation peak
/// in `lo..=hi`, refined from its furthest strong multiple.
fn pitch_of(profile: &[f32], lo: usize, hi: usize) -> Option<AxisPitch> {
    let n = profile.len();
    if n < lo * 3 {
        return None;
    }
    // Detrend with a wide moving average, so shading across the view does
    // not read as a long period.
    let half = (hi.min(n / 2)).max(lo);
    let mut prefix = vec![0.0f64; n + 1];
    for (i, v) in profile.iter().enumerate() {
        prefix[i + 1] = prefix[i] + *v as f64;
    }
    let signal: Vec<f64> = (0..n)
        .map(|i| {
            let a = i.saturating_sub(half);
            let b = (i + half + 1).min(n);
            profile[i] as f64 - (prefix[b] - prefix[a]) / (b - a) as f64
        })
        .collect();
    let energy: f64 = signal.iter().map(|v| v * v).sum();
    if energy <= 1e-9 {
        return None;
    }
    let max_lag = (n / 2).max(lo + 2);
    let r: Vec<f64> = (0..=max_lag)
        .map(|lag| {
            let s: f64 = (0..n - lag).map(|i| signal[i] * signal[i + lag]).sum();
            // Biased: long lags overlap less and count less, so a stray
            // long-lag match does not beat the cube pitch.
            s / energy
        })
        .collect();
    let peak =
        |lag: usize| lag > 0 && lag < max_lag && r[lag] >= r[lag - 1] && r[lag] >= r[lag + 1];
    // A cube pitch repeats: score a peak by the autocorrelation at its
    // first multiples too (at least two), so a stray long-lag peak or half
    // the pitch loses to the fundamental.
    let near = |lag: usize| -> f64 {
        (lag.saturating_sub(1)..=(lag + 1).min(max_lag))
            .map(|l| r[l])
            .fold(f64::MIN, f64::max)
    };
    let score = |lag: usize| -> Option<f64> {
        let k = ((n / 3) / lag).min(3);
        (k >= 2).then(|| (1..=k).map(|m| near(lag * m)).sum::<f64>() / k as f64)
    };
    let (first, best) = (lo..=hi.min(max_lag - 1))
        .filter(|&lag| peak(lag))
        .filter_map(|lag| score(lag).map(|s| (lag, s)))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    if best <= 0.02 || r[first] <= 0.04 {
        return None;
    }
    // A pitch whose half (or third) is a peak nearly as strong is that
    // multiple of the real one: weak seams every other cube.
    let mut first = first;
    for d in [2usize, 3] {
        let base = (first as f64 / d as f64).round() as usize;
        if base < lo {
            continue;
        }
        let candidate = (base.saturating_sub(1)..=base + 1)
            .filter(|&lag| peak(lag))
            .max_by(|a, b| r[*a].total_cmp(&r[*b]));
        if let Some(lag) = candidate {
            if r[lag] >= 0.5 * r[first] {
                first = lag;
                break;
            }
        }
    }
    let top = r[first];
    let refine = |lag: usize| -> f64 {
        let (a, b, c) = (r[lag - 1], r[lag], r[lag + 1]);
        let d = a - 2.0 * b + c;
        if d.abs() < 1e-12 {
            lag as f64
        } else {
            lag as f64 + 0.5 * (a - c) / d
        }
    };
    let mut pitch = refine(first);
    // The furthest multiple still a peak gives the finest estimate.
    let mut k = 2;
    loop {
        let guess = (pitch * k as f64).round() as usize;
        if guess + 2 >= max_lag {
            break;
        }
        let around = (guess.saturating_sub(1)..=guess + 1).filter(|&lag| peak(lag));
        let Some(lag) = around.max_by(|a, b| r[*a].total_cmp(&r[*b])) else {
            break;
        };
        if r[lag] < 0.35 * top {
            break;
        }
        pitch = refine(lag) / k as f64;
        k += 1;
    }
    Some(AxisPitch {
        pitch: pitch as f32,
        strength: r[first].clamp(0.0, 1.0) as f32,
    })
}

fn checks(views: &[SheetView]) -> Vec<Check> {
    let find = |name: &str| views.iter().find(|v| v.name.as_deref() == Some(name));
    let mut out = Vec::new();
    let pairs: [(&'static str, &str, usize, &str, usize); 3] = [
        ("front height = side height", "front", 3, "side", 3),
        ("front width = top width", "front", 2, "top", 2),
        ("side length = top length", "side", 2, "top", 3),
    ];
    for (name, a, ai, b, bi) in pairs {
        let (Some(va), Some(vb)) = (find(a), find(b)) else {
            continue;
        };
        let (la, lb) = (va.crop[ai] as f32, vb.crop[bi] as f32);
        let axis = |i: usize| if i == 2 { "width" } else { "height" };
        let percent = (la - lb).abs() / la.max(lb) * 100.0;
        // A width counts in the view's across pitch, a height in its down
        // pitch: a sheet drawn stretched has cubes taller than wide.
        let along = |v: &SheetView, i: usize| {
            let axis = if i == 2 { v.pitch_x } else { v.pitch_y };
            axis.map(|p| p.pitch).or(v.pitch())
        };
        let cubes = match (along(va, ai), along(vb, bi)) {
            (Some(pa), Some(pb)) => Some((la / pa, lb / pb)),
            _ => None,
        };
        out.push(Check {
            name,
            a: (a.to_string(), axis(ai), la),
            b: (b.to_string(), axis(bi), lb),
            percent,
            percent_cubes: cubes.map(|(ca, cb)| (ca - cb).abs() / ca.max(cb) * 100.0),
            cubes,
        });
    }
    out
}

/// Chamfer (3-4) distance to the nearest pixel where `inside` is false,
/// in pixels. Past the edge counts as such a pixel when `edge` is set.
fn distance(inside: &[bool], w: usize, h: usize, edge: bool) -> Vec<f32> {
    const FAR: u32 = u32::MAX / 4;
    let beyond = if edge { 0 } else { FAR };
    let mut d: Vec<u32> = inside.iter().map(|&v| if v { FAR } else { 0 }).collect();
    let at = |x: isize, y: isize, d: &Vec<u32>| -> u32 {
        if x < 0 || y < 0 || x >= w as isize || y >= h as isize {
            beyond
        } else {
            d[y as usize * w + x as usize]
        }
    };
    for y in 0..h as isize {
        for x in 0..w as isize {
            let i = y as usize * w + x as usize;
            if d[i] == 0 {
                continue;
            }
            let best = [
                at(x - 1, y, &d) + 3,
                at(x, y - 1, &d) + 3,
                at(x - 1, y - 1, &d) + 4,
                at(x + 1, y - 1, &d) + 4,
            ]
            .into_iter()
            .min()
            .unwrap();
            d[i] = d[i].min(best);
        }
    }
    for y in (0..h as isize).rev() {
        for x in (0..w as isize).rev() {
            let i = y as usize * w + x as usize;
            if d[i] == 0 {
                continue;
            }
            let best = [
                at(x + 1, y, &d) + 3,
                at(x, y + 1, &d) + 3,
                at(x + 1, y + 1, &d) + 4,
                at(x - 1, y + 1, &d) + 4,
            ]
            .into_iter()
            .min()
            .unwrap();
            d[i] = d[i].min(best);
        }
    }
    d.into_iter().map(|v| v as f32 / 3.0).collect()
}

/// 8-connected components of `on`, as lists of pixel indices.
fn components(on: &[bool], w: usize, h: usize) -> Vec<Vec<usize>> {
    let mut seen = vec![false; on.len()];
    let mut out = Vec::new();
    for start in 0..on.len() {
        if !on[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        let mut list = vec![start];
        let mut k = 0;
        while k < list.len() {
            let i = list[k];
            k += 1;
            let (x, y) = ((i % w) as isize, (i / w) as isize);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    if on[j] && !seen[j] {
                        seen[j] = true;
                        list.push(j);
                    }
                }
            }
        }
        out.push(list);
    }
    out
}

fn count_parts(mask: &Mask, view: &SheetView, spec: &PartsSpec, name: String) -> Parts {
    let [cx, cy, cw, ch] = view.crop.map(|v| v as usize);
    // A margin so the opening sees background round the view.
    let pad = 2;
    let (w, h) = (cw + 2 * pad, ch + 2 * pad);
    let mut inside = vec![false; w * h];
    for y in 0..ch {
        for x in 0..cw {
            let src = (cy + y) * mask.width as usize + cx + x;
            inside[(y + pad) * w + x + pad] = mask.pixels[src].is_some();
        }
    }
    // Specks in the crop are no part; a piece drawn apart from the body
    // (a head behind an outline gap) is.
    let all = inside.iter().filter(|v| **v).count();
    let speck = ((all as f32 * spec.min_share).ceil() as usize).max(4);
    for piece in components(&inside, w, h) {
        if piece.len() < speck {
            for i in piece {
                inside[i] = false;
            }
        }
    }
    let total = inside.iter().filter(|v| **v).count();
    let dist = distance(&inside, w, h, true);
    let thickest = dist.iter().copied().fold(0.0f32, f32::max);
    let radius = spec.radius.unwrap_or(thickest * 0.5).max(1.0);
    let eroded: Vec<bool> = dist.iter().map(|d| *d > radius).collect();
    let core = components(&eroded, w, h)
        .into_iter()
        .max_by_key(|c| c.len())
        .unwrap_or_default();
    let mut core_mask = vec![false; w * h];
    for i in &core {
        core_mask[*i] = true;
    }
    // Grow the core back by the radius: the distance to it.
    let outside_core: Vec<bool> = core_mask.iter().map(|v| !v).collect();
    let to_core = distance(&outside_core, w, h, false);
    let body: Vec<bool> = (0..w * h)
        .map(|i| inside[i] && to_core[i] <= radius + 1.0)
        .collect();
    let rest: Vec<bool> = (0..w * h).map(|i| inside[i] && !body[i]).collect();
    let body_pixels = body.iter().filter(|v| **v).count();
    let (mut sx, mut sy) = (0.0f64, 0.0f64);
    for (i, b) in body.iter().enumerate() {
        if *b {
            sx += (i % w) as f64;
            sy += (i / w) as f64;
        }
    }
    let centre = if body_pixels > 0 {
        [sx / body_pixels as f64, sy / body_pixels as f64]
    } else {
        [w as f64 * 0.5, h as f64 * 0.5]
    };
    let floor = ((total as f32 * spec.min_share).ceil() as usize).max(4);
    let mut parts = Vec::new();
    for piece in components(&rest, w, h) {
        // A sliver along the body's outline is no part: it must be thicker
        // than a few pixels somewhere.
        let thick = piece.iter().map(|i| dist[*i]).fold(0.0f32, f32::max);
        if piece.len() < floor || thick < 3.0 {
            continue;
        }
        // The opening rounds a boxy body's corners off, but such a corner
        // reaches a third of the radius past the body (4/3 - 1 along the
        // chamfer diagonal); a limb, a head, or a tail sticks out further.
        let reach = piece.iter().map(|i| to_core[*i]).fold(0.0f32, f32::max) - radius;
        if reach < 0.42 * radius {
            continue;
        }
        let mut b = [usize::MAX, usize::MAX, 0, 0];
        let (mut px, mut py) = (0.0f64, 0.0f64);
        for i in &piece {
            let (x, y) = (i % w, i / w);
            b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
            px += x as f64;
            py += y as f64;
        }
        let n = piece.len() as f64;
        let angle = (-(py / n - centre[1]))
            .atan2(px / n - centre[0])
            .to_degrees();
        let dirs = ["e", "ne", "n", "nw", "w", "sw", "s", "se"];
        let sector = (((angle + 360.0 + 22.5) % 360.0) / 45.0) as usize % 8;
        parts.push(Part {
            bounds: [
                (b[0] - pad + cx) as u32,
                (b[1] - pad + cy) as u32,
                (b[2] - b[0] + 1) as u32,
                (b[3] - b[1] + 1) as u32,
            ],
            pixels: piece.len(),
            direction: dirs[sector],
        });
    }
    parts.sort_by_key(|p| (p.bounds[1], p.bounds[0]));
    Parts {
        view: name,
        radius,
        body_pixels,
        parts,
    }
}

/// Brightness (Rec. 601) of each pixel of an RGBA image.
pub fn luma_of(rgba: &[u8]) -> Vec<f32> {
    rgba.chunks_exact(4)
        .map(|p| 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32)
        .collect()
}

#[cfg(test)]
#[path = "../../tests/sheet_measure.rs"]
mod tests;
