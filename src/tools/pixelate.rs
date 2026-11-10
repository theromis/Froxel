//! MCP `pixelate_views`: an orthographic views sheet turned into pixel art
//! at the cell grid it will be carved or painted at, before
//! `carve_from_views` or `paint_from_views` read it.
//!
//! A creature 20 cells long carved straight from a 480 pixel view loses its
//! eyes, teeth, and spots: each cell averages 24 x 24 pixels, so a pupil
//! becomes a brown smear and a thin leg falls under half a cell's coverage
//! and goes. This follows the content-adaptive downscaling of Kopf, Shamir
//! and Peers (SIGGRAPH Asia 2013) and the pixel-art abstraction of
//! Gerstner et al. (2012) in three parts:
//!
//! - a small palette, shared by every view, clustered in Oklab (median cut
//!   and k-means, with the accent guard that keeps small distinct colours);
//! - each cell takes the colour that holds most of a centre-weighted
//!   kernel over its footprint, never a mean, so edges stay crisp and no
//!   in-between colour appears; anti-aliased fringe pixels do not vote;
//! - small salient features (connected regions of one colour from
//!   `min_area` to `max_area` cells whose colour stands `contrast` from
//!   their surroundings: eyes, pupils, teeth, nostrils, spots) keep at
//!   least `min_cells` cells, smallest first so a pupil keeps its place
//!   inside the eye; and a cell on the midline of a part thinner than a
//!   cell (a fin, a leg, a whisker) stays inked from a `thin` share of ink.

use super::colors::{self, Guard, Lab};
use super::{Ink, Mask, Side};

/// Most clusters `colors` asks for.
pub const MAX_PIXEL_COLORS: usize = 32;
/// Most cells along one axis of the box.
pub const MAX_PIXEL_CELLS: u32 = 512;
/// The characters the rows use, one per colour; `.` is empty.
pub const PIXEL_CHARS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
/// Most clustered samples; larger sheets are read with a stride.
const MAX_SAMPLES: usize = 160_000;
/// Fringe pixels (this close to the background, in pixels) do not vote.
const FRINGE: f32 = 1.5;

/// How small salient features are kept.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Features {
    /// Smallest region kept, in cells of area.
    pub min_area: f32,
    /// Largest region that counts as a feature, in cells of area.
    pub max_area: f32,
    /// Oklab distance from the mean colour around the region.
    pub contrast: f32,
    /// Cells a feature keeps at least.
    pub min_cells: u32,
}

impl Default for Features {
    fn default() -> Self {
        Self {
            min_area: 0.15,
            max_area: 6.0,
            contrast: 0.1,
            min_cells: 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PixelateSpec {
    /// Clusters of the shared palette.
    pub colors: usize,
    pub guard: Option<Guard>,
    /// Share of a cell's footprint that must be inked, 0..1.
    pub coverage: f32,
    /// The least share for a cell on the midline of a part thinner than a
    /// cell; `None` keeps only `coverage`.
    pub thin: Option<f32>,
    pub features: Option<Features>,
    /// Passes of a majority filter: a cell whose colour fewer than two of
    /// its eight inked neighbours share takes their commonest colour.
    /// Feature cells are kept.
    pub despeckle: u32,
}

impl Default for PixelateSpec {
    fn default() -> Self {
        Self {
            colors: 6,
            guard: Some(Guard::default()),
            coverage: 0.5,
            thin: Some(0.25),
            features: Some(Features::default()),
            despeckle: 1,
        }
    }
}

/// One feature a view kept.
#[derive(Clone, Debug, PartialEq)]
pub struct Feature {
    /// Index into [`Pixelated::colours`].
    pub colour: usize,
    /// Its area in the image, in cells.
    pub area: f32,
    /// Cells of its colour over it after the call.
    pub cells: usize,
    /// Cells it took from the majority vote.
    pub added: usize,
    /// Its middle, column and row of the grid.
    pub at: [u32; 2],
}

/// One view at its grid.
#[derive(Clone, Debug, PartialEq)]
pub struct PixelView {
    pub side: Side,
    /// Columns and rows.
    pub grid: [u32; 2],
    /// Each cell's colour index, row-major with y down; `None` is empty.
    pub cells: Vec<Option<usize>>,
    /// The ink rectangle of the source, `[x, y, width, height]` pixels.
    pub source: [u32; 4],
    pub inked: usize,
    /// Cells inked by the thin rule under `coverage`.
    pub thin: usize,
    pub features: Vec<Feature>,
}

impl PixelView {
    /// The rows: one character of [`PIXEL_CHARS`] per colour, `.` empty.
    pub fn rows(&self) -> Vec<String> {
        let chars: Vec<char> = PIXEL_CHARS.chars().collect();
        (0..self.grid[1])
            .map(|row| {
                (0..self.grid[0])
                    .map(
                        |col| match self.cells[(row * self.grid[0] + col) as usize] {
                            Some(i) => chars[i],
                            None => '.',
                        },
                    )
                    .collect()
            })
            .collect()
    }

    /// The view as a mask of the palette's colours (or plain ink).
    pub fn mask(&self, colours: &[Option<[u8; 3]>]) -> Mask {
        Mask {
            width: self.grid[0],
            height: self.grid[1],
            pixels: self
                .cells
                .iter()
                .map(|c| c.map(|i| colours[i].map_or(Ink::Plain, Ink::Rgb)))
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pixelated {
    /// The shared palette; `None` is plain ink (a rows character with no
    /// legend entry).
    pub colours: Vec<Option<[u8; 3]>>,
    /// How many of the colours the accent guard kept.
    pub accents: usize,
    /// The box in cells; 0 on an axis no view spans.
    pub size: [u32; 3],
    pub views: Vec<PixelView>,
}

impl Pixelated {
    /// The legend the rows use: character and colour.
    pub fn legend(&self) -> Vec<(char, Option<[u8; 3]>)> {
        PIXEL_CHARS
            .chars()
            .zip(self.colours.iter().copied())
            .collect()
    }
}

/// The box and each view's grid: an axis of `size` left out takes the
/// cells another view's ink proportions give it from an axis that is
/// known. `ink` is each view's ink width and height in pixels.
pub fn view_grids(
    sides: &[Side],
    ink: &[[u32; 2]],
    size: [Option<u32>; 3],
) -> Result<([u32; 3], Vec<[u32; 2]>), String> {
    let mut known: [Option<f64>; 3] = size.map(|s| s.map(f64::from));
    for (axis, given) in size.iter().enumerate() {
        if let Some(n) = given {
            if *n == 0 || *n > MAX_PIXEL_CELLS {
                return Err(format!(
                    "size along {} must be 1..={MAX_PIXEL_CELLS}",
                    ["x", "y", "z"][axis]
                ));
            }
        }
    }
    if known.iter().all(Option::is_none) {
        return Err("pass size [x, y, z] with at least one axis in cells".into());
    }
    for _ in 0..3 {
        let mut estimates: [Vec<f64>; 3] = Default::default();
        for (side, [w, h]) in sides.iter().zip(ink) {
            let [(u, _), (v, _), _] = side.axes();
            let (pu, pv) = (f64::from(*w), f64::from(*h));
            match (known[u], known[v]) {
                (Some(cu), None) => estimates[v].push(cu * pv / pu.max(1.0)),
                (None, Some(cv)) => estimates[u].push(cv * pu / pv.max(1.0)),
                _ => {}
            }
        }
        for axis in 0..3 {
            if known[axis].is_none() && !estimates[axis].is_empty() {
                let mean = estimates[axis].iter().sum::<f64>() / estimates[axis].len() as f64;
                known[axis] = Some(mean.round().clamp(1.0, f64::from(MAX_PIXEL_CELLS)));
            }
        }
    }
    let mut out = [0u32; 3];
    for axis in 0..3 {
        let spanned = sides.iter().any(|side| {
            let [(u, _), (v, _), _] = side.axes();
            u == axis || v == axis
        });
        match known[axis] {
            Some(n) if spanned || size[axis].is_some() => out[axis] = n as u32,
            None if spanned => {
                return Err(format!(
                    "the size along {} is unknown: pass it in size, or an axis of a view across it",
                    ["x", "y", "z"][axis]
                ))
            }
            _ => {}
        }
    }
    let grids = sides
        .iter()
        .map(|side| {
            let [(u, _), (v, _), _] = side.axes();
            [out[u], out[v]]
        })
        .collect();
    Ok((out, grids))
}

/// The colour of a pixel: `None` is plain ink.
fn ink_rgb(ink: Ink, palette: &[[u8; 3]; 256]) -> Option<[u8; 3]> {
    match ink {
        Ink::Rgb(rgb) => Some(rgb),
        Ink::Slot(slot) => Some(palette[slot as usize]),
        Ink::Plain => None,
    }
}

/// Distance in pixels to the nearest background pixel (outside the image
/// counts as background), by a two-pass chamfer.
fn background_distance(inked: &[bool], w: usize, h: usize) -> Vec<f32> {
    const D: f32 = std::f32::consts::SQRT_2;
    let mut d: Vec<f32> = inked
        .iter()
        .map(|i| if *i { f32::MAX } else { 0.0 })
        .collect();
    let at = |d: &Vec<f32>, x: i64, y: i64| -> f32 {
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            0.0
        } else {
            d[y as usize * w + x as usize]
        }
    };
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            let i = y as usize * w + x as usize;
            if d[i] == 0.0 {
                continue;
            }
            let best = (at(&d, x - 1, y) + 1.0)
                .min(at(&d, x, y - 1) + 1.0)
                .min(at(&d, x - 1, y - 1) + D)
                .min(at(&d, x + 1, y - 1) + D);
            d[i] = d[i].min(best);
        }
    }
    for y in (0..h as i64).rev() {
        for x in (0..w as i64).rev() {
            let i = y as usize * w + x as usize;
            if d[i] == 0.0 {
                continue;
            }
            let best = (at(&d, x + 1, y) + 1.0)
                .min(at(&d, x, y + 1) + 1.0)
                .min(at(&d, x + 1, y + 1) + D)
                .min(at(&d, x - 1, y + 1) + D);
            d[i] = d[i].min(best);
        }
    }
    d
}

/// A view cropped to its ink, with each pixel's colour label.
struct Source {
    w: usize,
    h: usize,
    /// Colour index per pixel; `None` is background.
    label: Vec<Option<usize>>,
    /// Distance to the background, pixels.
    dist: Vec<f32>,
    rect: [u32; 4],
}

/// Pixelate `views` (each side and its mask) into `grids`. `palette`
/// gives a rows legend's slots their colours.
pub fn pixelate(
    views: &[(Side, Mask)],
    grids: &[[u32; 2]],
    spec: &PixelateSpec,
    palette: &[[u8; 3]; 256],
) -> Result<Pixelated, String> {
    if views.is_empty() || views.len() > 6 {
        return Err("views must hold 1 to 6 views".into());
    }
    if spec.colors == 0 || spec.colors > MAX_PIXEL_COLORS {
        return Err(format!("colors must be 1..={MAX_PIXEL_COLORS}"));
    }
    if !(0.0..=1.0).contains(&spec.coverage) {
        return Err("coverage must be 0..1".into());
    }
    if let Some(thin) = spec.thin {
        if !(0.0..=1.0).contains(&thin) {
            return Err("thin must be 0..1".into());
        }
    }
    if let Some(f) = &spec.features {
        if !(f.min_area >= 0.0 && f.max_area >= f.min_area && f.max_area <= 1024.0) {
            return Err("features need 0 <= minArea <= maxArea <= 1024 cells".into());
        }
        if !(0.0..=1.5).contains(&f.contrast) {
            return Err("features contrast must be 0..1.5 (an Oklab distance)".into());
        }
        if f.min_cells == 0 || f.min_cells > 16 {
            return Err("features minCells must be 1..16".into());
        }
    }
    for (i, (side, mask)) in views.iter().enumerate() {
        if views[..i].iter().any(|(s, _)| s == side) {
            return Err(format!("view {} is given twice", side.name()));
        }
        if mask.count() == 0 {
            return Err(format!("view {} has no ink", side.name()));
        }
    }
    // Crop every view to its ink and measure the fringe.
    let mut crops: Vec<(Mask, [u32; 4], Vec<f32>)> = Vec::new();
    for (_, mask) in views {
        let [x0, y0, x1, y1] = mask.bounds().expect("checked above");
        let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
        let mut pixels = Vec::with_capacity((w * h) as usize);
        for y in y0..=y1 {
            for x in x0..=x1 {
                pixels.push(mask.get(x, y));
            }
        }
        let crop = Mask {
            width: w,
            height: h,
            pixels,
        };
        let inked: Vec<bool> = crop.pixels.iter().map(Option::is_some).collect();
        let dist = background_distance(&inked, w as usize, h as usize);
        crops.push((crop, [x0, y0, w, h], dist));
    }
    // The shared palette, from every view's non-fringe ink.
    let total: usize = crops.iter().map(|(m, _, _)| m.count()).sum();
    let stride = total.div_ceil(MAX_SAMPLES).max(1);
    let mut samples: Vec<(Lab, f32)> = Vec::new();
    let mut fallback: Vec<(Lab, f32)> = Vec::new();
    let mut plain = false;
    let mut n = 0usize;
    for (mask, _, dist) in &crops {
        for (i, pixel) in mask.pixels.iter().enumerate() {
            let Some(ink) = pixel else { continue };
            let Some(rgb) = ink_rgb(*ink, palette) else {
                plain = true;
                continue;
            };
            n += 1;
            if n % stride != 0 {
                continue;
            }
            if dist[i] > FRINGE {
                samples.push((colors::oklab(rgb), 1.0));
            } else {
                fallback.push((colors::oklab(rgb), 1.0));
            }
        }
    }
    if samples.is_empty() {
        samples = fallback;
    }
    let (centres, accents) = if samples.is_empty() {
        (Vec::new(), 0)
    } else {
        let clusters = colors::cluster(&samples, spec.colors, spec.guard);
        (clusters.centres, clusters.accents)
    };
    let mut colours: Vec<Option<[u8; 3]>> =
        centres.iter().map(|c| Some(colors::srgb(*c))).collect();
    let plain_label = plain.then(|| {
        colours.push(None);
        colours.len() - 1
    });
    if colours.len() > PIXEL_CHARS.len() {
        return Err("too many colours for the rows' characters".into());
    }
    let mut out = Vec::new();
    for (((side, _), (mask, rect, dist)), grid) in views.iter().zip(crops).zip(grids) {
        let label: Vec<Option<usize>> = mask
            .pixels
            .iter()
            .map(|pixel| {
                pixel.map(|ink| match ink_rgb(ink, palette) {
                    Some(rgb) => colors::nearest(colors::oklab(rgb), &centres),
                    None => plain_label.expect("plain ink seen"),
                })
            })
            .collect();
        let source = Source {
            w: mask.width as usize,
            h: mask.height as usize,
            label,
            dist,
            rect,
        };
        out.push(pixelate_view(
            *side, &source, *grid, spec, &centres, &colours,
        )?);
    }
    Ok(Pixelated {
        colours,
        accents,
        size: [0; 3],
        views: out,
    })
}

fn pixelate_view(
    side: Side,
    src: &Source,
    grid: [u32; 2],
    spec: &PixelateSpec,
    centres: &[Lab],
    colours: &[Option<[u8; 3]>],
) -> Result<PixelView, String> {
    let [cols, rows] = grid;
    if cols == 0 || rows == 0 || cols > MAX_PIXEL_CELLS || rows > MAX_PIXEL_CELLS {
        return Err(format!(
            "{} view grid must be 1..={MAX_PIXEL_CELLS} cells a side",
            side.name()
        ));
    }
    let (w, h) = (src.w, src.h);
    let fx = w as f64 / f64::from(cols);
    let fy = h as f64 / f64::from(rows);
    let cell_px = fx.min(fy) as f32;
    let ncol = colours.len();
    let index = |col: u32, row: u32| (row * cols + col) as usize;
    // A pixel on the midline of a part thinner than a cell.
    let ridge = |x: usize, y: usize| -> bool {
        let d = src.dist[y * w + x];
        if d <= 0.0 || d > 0.6 * cell_px.max(1.0) {
            return false;
        }
        let at = |x: i64, y: i64| -> f32 {
            if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                0.0
            } else {
                src.dist[y as usize * w + x as usize]
            }
        };
        let (x, y) = (x as i64, y as i64);
        (d >= at(x - 1, y) && d >= at(x + 1, y)) || (d >= at(x, y - 1) && d >= at(x, y + 1))
    };
    let mut cells: Vec<Option<usize>> = vec![None; (cols * rows) as usize];
    let mut thin_cells = 0usize;
    // Per cell, the pixels of its footprint (half-open pixel ranges).
    let span = |a: f64, b: f64, n: usize| -> (usize, usize) {
        let lo = (a.floor().max(0.0) as usize).min(n);
        let hi = (b.ceil().max(0.0) as usize).min(n);
        (lo, hi.max((lo + 1).min(n)))
    };
    for row in 0..rows {
        for col in 0..cols {
            let (xa, xb) = (f64::from(col) * fx, f64::from(col + 1) * fx);
            let (ya, yb) = (f64::from(row) * fy, f64::from(row + 1) * fy);
            // Coverage by exact area over the footprint.
            let (x0, x1) = span(xa, xb, w);
            let (y0, y1) = span(ya, yb, h);
            let mut area = 0.0;
            let mut inked = 0.0;
            let mut ridges = 0usize;
            for y in y0..y1 {
                let wy = (yb.min(y as f64 + 1.0) - ya.max(y as f64)).max(0.0);
                for x in x0..x1 {
                    let wx = (xb.min(x as f64 + 1.0) - xa.max(x as f64)).max(0.0);
                    let a = wx * wy;
                    area += a;
                    if src.label[y * w + x].is_some() {
                        inked += a;
                        if spec.thin.is_some() && ridge(x, y) {
                            ridges += 1;
                        }
                    }
                }
            }
            if area <= 0.0 || inked <= 0.0 {
                continue;
            }
            let share = (inked / area) as f32;
            let keep = if share >= spec.coverage {
                true
            } else if let Some(thin) = spec.thin {
                let enough = ridges as f32 >= (0.25 * cell_px).max(1.0);
                if share >= thin && enough {
                    thin_cells += 1;
                    true
                } else {
                    false
                }
            } else {
                false
            };
            if !keep {
                continue;
            }
            // The colour vote: a tent over the footprint widened by a
            // quarter cell on each side; fringe pixels vote only when
            // nothing else does.
            let (ka, kb) = (xa - 0.25 * fx, xb + 0.25 * fx);
            let (la, lb) = (ya - 0.25 * fy, yb + 0.25 * fy);
            let (cx, cy) = ((xa + xb) / 2.0, (ya + yb) / 2.0);
            let (hx, hy) = ((kb - ka) / 2.0, (lb - la) / 2.0);
            let (kx0, kx1) = span(ka, kb, w);
            let (ky0, ky1) = span(la, lb, h);
            let mut votes = vec![0.0f64; ncol];
            let mut fringe = vec![0.0f64; ncol];
            for y in ky0..ky1 {
                let ty = 1.0 - 0.8 * ((y as f64 + 0.5 - cy).abs() / hy).min(1.0);
                for x in kx0..kx1 {
                    let Some(l) = src.label[y * w + x] else {
                        continue;
                    };
                    let tx = 1.0 - 0.8 * ((x as f64 + 0.5 - cx).abs() / hx).min(1.0);
                    if src.dist[y * w + x] > FRINGE {
                        votes[l] += tx * ty;
                    } else {
                        fringe[l] += tx * ty;
                    }
                }
            }
            let pick = |v: &[f64]| {
                v.iter()
                    .enumerate()
                    .filter(|(_, w)| **w > 0.0)
                    .max_by(|a, b| a.1.total_cmp(b.1).then(b.0.cmp(&a.0)))
                    .map(|(i, _)| i)
            };
            cells[index(col, row)] = pick(&votes).or_else(|| pick(&fringe));
        }
    }
    let mut features = Vec::new();
    let mut locked = vec![false; cells.len()];
    if let Some(f) = spec.features {
        features = keep_features(src, grid, &mut cells, &mut locked, &f, centres, fx, fy);
    }
    despeckle(&mut cells, &locked, grid, spec.despeckle);
    let inked = cells.iter().filter(|c| c.is_some()).count();
    Ok(PixelView {
        side,
        grid,
        cells,
        source: src.rect,
        inked,
        thin: thin_cells,
        features,
    })
}

/// Give each small salient region of `src` at least `f.min_cells` cells of
/// its colour, smallest region first; a cell another feature took is not
/// taken again.
fn keep_features(
    src: &Source,
    grid: [u32; 2],
    cells: &mut [Option<usize>],
    locked: &mut [bool],
    f: &Features,
    centres: &[Lab],
    fx: f64,
    fy: f64,
) -> Vec<Feature> {
    let (w, h) = (src.w, src.h);
    let [cols, rows] = grid;
    let cell_area = fx * fy;
    // Connected regions of one colour (4-neighbours).
    let mut region = vec![usize::MAX; w * h];
    struct Region {
        label: usize,
        pixels: Vec<usize>,
        ring: Vec<usize>,
    }
    let mut regions: Vec<Region> = Vec::new();
    for start in 0..w * h {
        let Some(label) = src.label[start] else {
            continue;
        };
        if region[start] != usize::MAX {
            continue;
        }
        let id = regions.len();
        let mut pixels = vec![start];
        region[start] = id;
        let mut head = 0;
        let mut ring_labels: Vec<usize> = Vec::new();
        let max_px = (f.max_area as f64 * cell_area).ceil() as usize + 1;
        while head < pixels.len() {
            let p = pixels[head];
            head += 1;
            let (x, y) = (p % w, p / w);
            let mut near = |q: usize| match src.label[q] {
                Some(l) if l == label => {
                    if region[q] == usize::MAX {
                        region[q] = id;
                        pixels.push(q);
                    }
                }
                Some(l) => ring_labels.push(l),
                None => {}
            };
            if x > 0 {
                near(p - 1);
            }
            if x + 1 < w {
                near(p + 1);
            }
            if y > 0 {
                near(p - w);
            }
            if y + 1 < h {
                near(p + w);
            }
        }
        // Too big to be a feature: no ring is needed.
        if pixels.len() > max_px {
            ring_labels.clear();
        }
        regions.push(Region {
            label,
            pixels,
            ring: ring_labels,
        });
    }
    let mut order: Vec<usize> = (0..regions.len())
        .filter(|i| {
            let r = &regions[*i];
            let area = r.pixels.len() as f64 / cell_area;
            if area < f64::from(f.min_area) || area > f64::from(f.max_area) || r.ring.is_empty() {
                return false;
            }
            let Some(own) = centres.get(r.label) else {
                return false;
            };
            let mut sum = [0.0f32; 3];
            let mut n = 0.0f32;
            for l in &r.ring {
                if let Some(c) = centres.get(*l) {
                    for a in 0..3 {
                        sum[a] += c[a];
                    }
                    n += 1.0;
                }
            }
            n > 0.0 && colors::distance(*own, sum.map(|s| s / n)) >= f.contrast
        })
        .collect();
    order.sort_by_key(|i| (regions[*i].pixels.len(), *i));
    let mut kept = Vec::new();
    for id in order {
        let r = &regions[id];
        let area = r.pixels.len() as f64 / cell_area;
        // Pixels of the region under each cell.
        let mut under: std::collections::HashMap<usize, usize> = Default::default();
        let (mut sx, mut sy) = (0.0f64, 0.0f64);
        for p in &r.pixels {
            let (x, y) = (p % w, p / w);
            sx += x as f64 + 0.5;
            sy += y as f64 + 0.5;
            let col = ((x as f64 + 0.5) / fx).floor().min(f64::from(cols) - 1.0) as u32;
            let row = ((y as f64 + 0.5) / fy).floor().min(f64::from(rows) - 1.0) as u32;
            *under.entry((row * cols + col) as usize).or_default() += 1;
        }
        let mut ranked: Vec<(usize, usize)> = under.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let need = (area.round() as usize).clamp(f.min_cells as usize, 4.max(f.min_cells as usize));
        let need = need.min(ranked.len());
        let mut have: Vec<usize> = ranked
            .iter()
            .filter(|(c, _)| cells[*c] == Some(r.label))
            .map(|(c, _)| *c)
            .collect();
        let mut added = 0;
        for (c, _) in &ranked {
            if have.len() >= need {
                break;
            }
            if locked[*c] || cells[*c] == Some(r.label) {
                continue;
            }
            cells[*c] = Some(r.label);
            have.push(*c);
            added += 1;
        }
        for c in &have {
            locked[*c] = true;
        }
        let n = r.pixels.len() as f64;
        kept.push(Feature {
            colour: r.label,
            area: area as f32,
            cells: have.len(),
            added,
            at: [
                ((sx / n) / fx).floor().min(f64::from(cols) - 1.0) as u32,
                ((sy / n) / fy).floor().min(f64::from(rows) - 1.0) as u32,
            ],
        });
    }
    kept
}

/// `passes` of a majority filter over the grid: an inked cell that is not
/// `locked` and whose colour fewer than two of its eight inked neighbours
/// share takes the colour most of them have. Empty cells stay empty.
fn despeckle(cells: &mut [Option<usize>], locked: &[bool], grid: [u32; 2], passes: u32) {
    let [cols, rows] = grid.map(|n| n as i64);
    for _ in 0..passes {
        let before = cells.to_vec();
        let mut changed = false;
        for row in 0..rows {
            for col in 0..cols {
                let i = (row * cols + col) as usize;
                let Some(own) = before[i] else { continue };
                if locked[i] {
                    continue;
                }
                let mut tally: Vec<(usize, u32)> = Vec::new();
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (x, y) = (col + dx, row + dy);
                        if (dx, dy) == (0, 0) || x < 0 || y < 0 || x >= cols || y >= rows {
                            continue;
                        }
                        if let Some(l) = before[(y * cols + x) as usize] {
                            match tally.iter_mut().find(|(c, _)| *c == l) {
                                Some(t) => t.1 += 1,
                                None => tally.push((l, 1)),
                            }
                        }
                    }
                }
                let same = tally.iter().find(|(c, _)| *c == own).map_or(0, |t| t.1);
                if same >= 2 || tally.is_empty() {
                    continue;
                }
                let best = tally
                    .iter()
                    .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
                    .expect("not empty");
                if best.1 > same {
                    cells[i] = Some(best.0);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

/// A PNG of the pixelated views side by side, `scale` pixels a cell, on
/// a light grey ground with a faint grid: width, height, and RGBA.
pub fn pixelated_image(p: &Pixelated, scale: u32) -> (u32, u32, Vec<u8>) {
    let scale = scale.clamp(1, 64);
    let gap = 2 * scale;
    let width: u32 =
        p.views.iter().map(|v| v.grid[0] * scale).sum::<u32>() + gap * (p.views.len() as u32 + 1);
    let height: u32 = p.views.iter().map(|v| v.grid[1] * scale).max().unwrap_or(1) + 2 * gap;
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    for px in rgba.chunks_mut(4) {
        px.copy_from_slice(&[236, 236, 236, 255]);
    }
    let mut x0 = gap;
    for view in &p.views {
        let [cols, rows] = view.grid;
        for row in 0..rows {
            for col in 0..cols {
                let base = match view.cells[(row * cols + col) as usize] {
                    Some(i) => p.colours[i].unwrap_or([128, 128, 128]),
                    None => {
                        if (row + col) % 2 == 0 {
                            [250, 250, 250]
                        } else {
                            [224, 224, 224]
                        }
                    }
                };
                for dy in 0..scale {
                    for dx in 0..scale {
                        let edge = scale >= 4 && (dx == 0 || dy == 0);
                        let c = if edge {
                            base.map(|v| (f32::from(v) * 0.85) as u8)
                        } else {
                            base
                        };
                        let (x, y) = (x0 + col * scale + dx, gap + row * scale + dy);
                        let i = ((y * width + x) * 4) as usize;
                        rgba[i..i + 3].copy_from_slice(&c);
                    }
                }
            }
        }
        x0 += cols * scale + gap;
    }
    (width, height, rgba)
}

#[cfg(test)]
#[path = "../../tests/pixelate.rs"]
mod tests;
