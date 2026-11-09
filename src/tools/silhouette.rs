//! Silhouettes: masks cut from a concept image, and MCP `carve_from_views`,
//! the visual hull of two or three orthographic views.
//!
//! A [`Mask`] is a grid of pixels, each empty or inked. It comes from an
//! RGBA image (a crop and a background rule) or from rows of characters,
//! which is how a Rhai script, with no file access, passes one. The hull
//! extrudes each view's mask through a box of cells along that view's
//! depth axis and keeps the cells every view covers: the rough form of a
//! turnaround sheet in 3D. `compare_silhouette` cuts its concept mask the
//! same way.

use std::collections::HashMap;

use crate::state::Cell;

/// Most cells in the box a carve fills.
pub const MAX_CARVE_CELLS: i64 = 4_000_000;
/// Most passes of `shell` and `smooth`.
pub const MAX_CARVE_PASSES: u32 = 64;

/// What one pixel of a mask holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    /// A colour from an image, quantised to the palette.
    Rgb([u8; 3]),
    /// A palette index from a rows legend.
    Slot(u8),
    /// Inked with no colour: a rows character missing from the legend.
    /// Only a carve with one `palette` can use it.
    Plain,
}

/// A mask, row-major with y down, as the image is.
#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<Option<Ink>>,
}

impl Mask {
    pub fn get(&self, x: u32, y: u32) -> Option<Ink> {
        self.pixels[(y * self.width + x) as usize]
    }

    pub fn inked(&self, x: u32, y: u32) -> bool {
        self.get(x, y).is_some()
    }

    pub fn count(&self) -> usize {
        self.pixels.iter().filter(|pixel| pixel.is_some()).count()
    }

    /// The same mask mirrored left to right.
    pub fn flipped(&self) -> Mask {
        let mut pixels = Vec::with_capacity(self.pixels.len());
        for y in 0..self.height {
            for x in (0..self.width).rev() {
                pixels.push(self.get(x, y));
            }
        }
        Mask {
            width: self.width,
            height: self.height,
            pixels,
        }
    }

    /// Inclusive pixel bounds of the ink, `[x0, y0, x1, y1]`.
    pub fn bounds(&self) -> Option<[u32; 4]> {
        let mut found: Option<[u32; 4]> = None;
        for y in 0..self.height {
            for x in 0..self.width {
                if !self.inked(x, y) {
                    continue;
                }
                found = Some(match found {
                    None => [x, y, x, y],
                    Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
                });
            }
        }
        found
    }
}

/// Which pixels of an image are background.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Background {
    /// Transparent where the crop has any transparent pixel, otherwise the
    /// most common colour of the whole image's border within `tolerance`,
    /// so a tight crop still keys on the sheet's paper.
    Auto { tolerance: f32 },
    /// Pixels with alpha below `threshold` (1..255).
    Alpha { threshold: u8 },
    /// Pixels within `tolerance` (Euclidean RGB distance, 0..442) of `color`,
    /// and transparent pixels.
    Key { color: [u8; 3], tolerance: f32 },
}

impl Default for Background {
    fn default() -> Self {
        Background::Auto { tolerance: 40.0 }
    }
}

/// A crop rectangle in image pixels: `[x, y, width, height]`.
pub type Crop = [u32; 4];

/// The mask of `crop` of an RGBA image (`width * height * 4` bytes) under
/// `background`. `None` crops the whole image.
pub fn mask_from_rgba(
    width: u32,
    height: u32,
    rgba: &[u8],
    crop: Option<Crop>,
    background: Background,
) -> Result<Mask, String> {
    let [cx, cy, cw, ch] = crop.unwrap_or([0, 0, width, height]);
    if cw == 0 || ch == 0 {
        return Err("crop width and height must be at least 1".into());
    }
    if u64::from(cx) + u64::from(cw) > u64::from(width)
        || u64::from(cy) + u64::from(ch) > u64::from(height)
    {
        return Err(format!(
            "crop [{cx}, {cy}, {cw}, {ch}] is outside the {width}x{height} image"
        ));
    }
    let pixel = |x: u32, y: u32| {
        let i = (((cy + y) * width + cx + x) * 4) as usize;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    };
    let rule = match background {
        Background::Auto { tolerance } => {
            let transparent = (0..ch).any(|y| (0..cw).any(|x| pixel(x, y)[3] < 255));
            if transparent {
                Background::Alpha { threshold: 128 }
            } else {
                Background::Key {
                    color: border_colour(width, height, rgba),
                    tolerance,
                }
            }
        }
        other => other,
    };
    let mut pixels = Vec::with_capacity((cw * ch) as usize);
    for y in 0..ch {
        for x in 0..cw {
            let [r, g, b, a] = pixel(x, y);
            let empty = match rule {
                Background::Alpha { threshold } => a < threshold,
                Background::Key { color, tolerance } => {
                    a < 128 || color_distance([r, g, b], color) <= tolerance
                }
                Background::Auto { .. } => unreachable!("resolved above"),
            };
            pixels.push((!empty).then_some(Ink::Rgb([r, g, b])));
        }
    }
    Ok(Mask {
        width: cw,
        height: ch,
        pixels,
    })
}

/// The most common colour on the image's outermost pixels.
fn border_colour(width: u32, height: u32, rgba: &[u8]) -> [u8; 3] {
    let mut tally: HashMap<[u8; 3], u32> = HashMap::new();
    let mut add = |x: u32, y: u32| {
        let i = ((y * width + x) * 4) as usize;
        *tally
            .entry([rgba[i], rgba[i + 1], rgba[i + 2]])
            .or_default() += 1;
    };
    for x in 0..width {
        add(x, 0);
        add(x, height - 1);
    }
    for y in 0..height {
        add(0, y);
        add(width - 1, y);
    }
    tally
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
        .map_or([255, 255, 255], |(color, _)| color)
}

/// Euclidean distance between two sRGB colours.
pub fn color_distance(a: [u8; 3], b: [u8; 3]) -> f32 {
    let d = |i: usize| f32::from(a[i]) - f32::from(b[i]);
    (d(0) * d(0) + d(1) * d(1) + d(2) * d(2)).sqrt()
}

/// A mask from rows of characters: `.` and space are empty; any other
/// character is inked, with the palette index or `#rrggbb` colour its
/// `legend` gives, or [`Ink::Plain`] without one. Rows may differ in
/// length; short rows are empty at the end.
pub fn mask_from_rows(rows: &[String], legend: &HashMap<char, Ink>) -> Result<Mask, String> {
    let height = rows.len();
    let width = rows
        .iter()
        .map(|row| row.chars().count())
        .max()
        .unwrap_or(0);
    if width == 0 || height == 0 {
        return Err("rows must hold at least one character".into());
    }
    if width > 4096 || height > 4096 {
        return Err("rows are at most 4096 by 4096".into());
    }
    let mut pixels = vec![None; width * height];
    for (y, row) in rows.iter().enumerate() {
        for (x, c) in row.chars().enumerate() {
            if c == '.' || c == ' ' {
                continue;
            }
            pixels[y * width + x] = Some(legend.get(&c).copied().unwrap_or(Ink::Plain));
        }
    }
    Ok(Mask {
        width: width as u32,
        height: height as u32,
        pixels,
    })
}

/// The side a view looks from, for a model facing −Y with +Z up, named as
/// MCP `model_view` names them: `left` looks from +X (yaw 0), `right` from
/// −X.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub const NAMES: &'static str = "front, back, left, right, side (left), top, or bottom";

    pub fn parse(name: &str) -> Result<Side, String> {
        Ok(match name {
            "front" => Side::Front,
            "back" => Side::Back,
            "left" | "side" => Side::Left,
            "right" => Side::Right,
            "top" => Side::Top,
            "bottom" => Side::Bottom,
            other => return Err(format!("view must be {}, not `{other}`", Side::NAMES)),
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Side::Front => "front",
            Side::Back => "back",
            Side::Left => "left",
            Side::Right => "right",
            Side::Top => "top",
            Side::Bottom => "bottom",
        }
    }

    /// The world axis and sign of image right, of image down, and of the
    /// direction toward the camera.
    pub fn axes(self) -> [(usize, i32); 3] {
        match self {
            Side::Front => [(0, 1), (2, -1), (1, -1)],
            Side::Back => [(0, -1), (2, -1), (1, 1)],
            Side::Left => [(1, 1), (2, -1), (0, 1)],
            Side::Right => [(1, -1), (2, -1), (0, -1)],
            Side::Top => [(0, 1), (1, -1), (2, 1)],
            Side::Bottom => [(0, -1), (1, -1), (2, -1)],
        }
    }
}

/// How a view's mask maps to cells.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scale {
    /// Cells per mask pixel along both image axes.
    CellsPerPixel(f32),
    /// The cells the mask spans, across and down.
    Size([u32; 2]),
    /// The box's size along the view's two axes.
    Fit,
}

#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub side: Side,
    pub mask: Mask,
    pub scale: Scale,
    /// Where the surface lies along each ray: cells in front of it go.
    pub depth: Option<super::depth::Depth>,
}

/// The colour of the carved cells.
#[derive(Clone, Debug, PartialEq)]
pub enum Colour {
    /// Each cell takes the pixel of the view whose visible surface is
    /// nearest to it; an image colour goes to the nearest of `slots`.
    Views { slots: Vec<u8> },
    /// One palette index.
    Slot(u8),
}

#[derive(Clone, Debug, PartialEq)]
pub struct CarveSpec {
    pub views: Vec<View>,
    /// The box's minimum corner, in local cells.
    pub origin: Cell,
    /// The box in cells; an axis left out takes the largest view along it.
    pub size: [Option<u32>; 3],
    pub colour: Colour,
    /// Share of a cell's footprint in a view that must be inked, 0..1.
    pub coverage: f32,
    /// Keep only cells within this many cells of the outside.
    pub shell: Option<u32>,
    /// Passes of the `smooth` majority filter over the hull.
    pub smooth: u32,
    /// Cells in front of a depth surface that still stay.
    pub depth_tolerance: f32,
}

/// One view's part of a carve.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewReport {
    pub side: Side,
    /// The view's grid in cells, across and down.
    pub grid: [u32; 2],
    /// Grid cells the mask covers.
    pub covered: usize,
    /// The depth map's calibration, and the hull cells it carved.
    pub depth: Option<(super::depth::Calibration, usize)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Carved {
    /// The carved cells and their palette index.
    pub cells: Vec<(Cell, u8)>,
    /// The box in cells.
    pub size: [u32; 3],
    /// Inclusive bounds of the carved cells.
    pub bounds: Option<(Cell, Cell)>,
    pub views: Vec<ViewReport>,
}

/// A view's mask resampled to its grid: each grid cell's inked share and ink.
/// `project_paint` resamples its image the same way.
pub(super) struct Grid {
    pub(super) cols: u32,
    pub(super) rows: u32,
    /// Where the grid starts along its across and down box axes.
    offset: [i64; 2],
    pub(super) inside: Vec<bool>,
    pub(super) ink: Vec<Option<Ink>>,
}

impl Grid {
    pub(super) fn build(mask: &Mask, cols: u32, rows: u32, coverage: f32) -> Grid {
        let (fx, fy) = (
            f64::from(mask.width) / f64::from(cols),
            f64::from(mask.height) / f64::from(rows),
        );
        let mut inside = vec![false; (cols * rows) as usize];
        let mut ink = vec![None; (cols * rows) as usize];
        for row in 0..rows {
            let (y0, y1) = (f64::from(row) * fy, f64::from(row + 1) * fy);
            for col in 0..cols {
                let (x0, x1) = (f64::from(col) * fx, f64::from(col + 1) * fx);
                let mut area = 0.0;
                let mut inked = 0.0;
                let mut sum = [0.0f64; 3];
                let mut slots: Vec<(u8, f64)> = Vec::new();
                let mut plain = 0.0;
                let mut py = y0.floor() as u32;
                while f64::from(py) < y1 && py < mask.height {
                    let wy = (y1.min(f64::from(py + 1)) - y0.max(f64::from(py))).max(0.0);
                    let mut px = x0.floor() as u32;
                    while f64::from(px) < x1 && px < mask.width {
                        let wx = (x1.min(f64::from(px + 1)) - x0.max(f64::from(px))).max(0.0);
                        let w = wx * wy;
                        area += w;
                        match mask.get(px, py) {
                            None => {}
                            Some(Ink::Rgb(c)) => {
                                inked += w;
                                for i in 0..3 {
                                    sum[i] += f64::from(c[i]) * w;
                                }
                            }
                            Some(Ink::Slot(slot)) => {
                                inked += w;
                                match slots.iter_mut().find(|(s, _)| *s == slot) {
                                    Some(found) => found.1 += w,
                                    None => slots.push((slot, w)),
                                }
                            }
                            Some(Ink::Plain) => {
                                inked += w;
                                plain += w;
                            }
                        }
                        px += 1;
                    }
                    py += 1;
                }
                let index = (row * cols + col) as usize;
                if area <= 0.0 || inked / area < f64::from(coverage) || inked <= 0.0 {
                    continue;
                }
                inside[index] = true;
                let rgb_weight = inked - plain - slots.iter().map(|(_, w)| w).sum::<f64>();
                let best_slot = slots
                    .iter()
                    .copied()
                    .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)));
                ink[index] = Some(match best_slot {
                    Some((slot, w)) if w >= rgb_weight && w >= plain => Ink::Slot(slot),
                    _ if rgb_weight >= plain && rgb_weight > 0.0 => {
                        Ink::Rgb(sum.map(|s| (s / rgb_weight).round().clamp(0.0, 255.0) as u8))
                    }
                    _ => Ink::Plain,
                });
            }
        }
        Grid {
            cols,
            rows,
            offset: [0, 0],
            inside,
            ink,
        }
    }

    /// The grid cell over box coordinates `along` (the across and down box
    /// axes), with the view's image signs.
    fn at(&self, side: Side, along: [i64; 2]) -> Option<usize> {
        let [(_, su), (_, sv), _] = side.axes();
        let u = along[0] - self.offset[0];
        let v = along[1] - self.offset[1];
        if !(0..i64::from(self.cols)).contains(&u) || !(0..i64::from(self.rows)).contains(&v) {
            return None;
        }
        let col = if su > 0 {
            u
        } else {
            i64::from(self.cols) - 1 - u
        };
        // Image down is −Z or −Y: the bottom row is the lowest cell.
        let row = if sv > 0 {
            v
        } else {
            i64::from(self.rows) - 1 - v
        };
        Some((row * i64::from(self.cols) + col) as usize)
    }
}

/// Remove the hull cells in front of each view's depth surface. Every map
/// is calibrated against the silhouette hull first, so the order of the
/// views does not matter.
fn carve_depths(
    spec: &CarveSpec,
    grids: &[Grid],
    size: [u32; 3],
    solid: &mut [bool],
    reports: &mut [ViewReport],
) {
    use super::depth;
    let sides: Vec<Side> = spec.views.iter().map(|view| view.side).collect();
    let [sx, sy, _] = size.map(|n| n as usize);
    let index = |p: [usize; 3]| (p[2] * sy + p[1]) * sx + p[0];
    let mut plans = Vec::new();
    for (vi, (view, grid)) in spec.views.iter().zip(grids).enumerate() {
        let Some(map) = &view.depth else {
            continue;
        };
        let [(u, _), (v, _), (d, toward)] = view.side.axes();
        let (nu, nv, nd) = (size[u] as usize, size[v] as usize, size[d] as usize);
        // Each ray's footprint on the view's image, and the hull's front.
        let (cols, rows) = (grid.cols as f64, grid.rows as f64);
        let mut rects = vec![None; nu * nv];
        let mut hull = vec![None; nu * nv];
        let mut hull_back = vec![None; nu * nv];
        for b in 0..nv {
            for a in 0..nu {
                let Some(cell) = grid.at(view.side, [a as i64, b as i64]) else {
                    continue;
                };
                if !grid.inside[cell] {
                    continue;
                }
                let (col, row) = (
                    (cell % grid.cols as usize) as f64,
                    (cell / grid.cols as usize) as f64,
                );
                rects[b * nu + a] = Some([
                    col / cols,
                    (col + 1.0) / cols,
                    row / rows,
                    (row + 1.0) / rows,
                ]);
                let on = |step: &usize| {
                    let mut p = [0usize; 3];
                    p[u] = a;
                    p[v] = b;
                    p[d] = if toward > 0 { nd - 1 - step } else { *step };
                    solid[index(p)]
                };
                hull[b * nu + a] = (0..nd).find(on).map(|step| step as f32);
                hull_back[b * nu + a] = (0..nd).rev().find(on).map(|step| step as f32);
            }
        }
        let middle = hull_middle(solid, size, d);
        let rays = depth::Rays {
            w: nu,
            h: nv,
            steps: nd,
            lines: [depth::measured(&sides, v, d), depth::measured(&sides, u, d)],
            middle: middle(toward),
            hull: &hull,
            hull_back: &hull_back,
            rects: &rects,
        };
        let (surface, cal) = depth::surfaces(map, &rays);
        plans.push((vi, cal, surface));
    }
    for (vi, cal, surface) in plans {
        let view = &spec.views[vi];
        let map = view.depth.as_ref().expect("a depth view");
        let [(u, _), (v, _), (d, toward)] = view.side.axes();
        let (nu, nd) = (size[u] as usize, size[d] as usize);
        let mut carved = 0;
        for (ray, s) in surface.iter().enumerate() {
            let Some(s) = s else {
                continue;
            };
            let (lo, hi) = depth::kept_span(map.back, cal.rim, *s, spec.depth_tolerance);
            for step in 0..nd {
                let centre = step as f32 + 0.5;
                if centre >= lo && centre <= hi {
                    continue;
                }
                let mut p = [0usize; 3];
                p[u] = ray % nu;
                p[v] = ray / nu;
                p[d] = if toward > 0 { nd - 1 - step } else { step };
                let i = index(p);
                if solid[i] {
                    solid[i] = false;
                    carved += 1;
                }
            }
        }
        reports[vi].depth = Some((cal, carved));
    }
}

/// The middle of the solid cells along axis `d`, in steps from the camera
/// side of a view whose camera is on side `toward`; the box's middle when
/// nothing is solid.
fn hull_middle(solid: &[bool], size: [u32; 3], d: usize) -> impl Fn(i32) -> f32 {
    let [sx, sy, _] = size.map(|n| n as usize);
    let (mut lo, mut hi) = (usize::MAX, 0usize);
    for (i, on) in solid.iter().enumerate() {
        if *on {
            let p = [i % sx, (i / sx) % sy, i / (sx * sy)];
            lo = lo.min(p[d]);
            hi = hi.max(p[d]);
        }
    }
    let n = size[d] as f32;
    let middle = if lo == usize::MAX {
        n / 2.0
    } else {
        (lo + hi + 1) as f32 / 2.0
    };
    move |toward: i32| if toward > 0 { n - middle } else { middle }
}

/// The nearest of `slots` to a colour.
pub(super) fn quantize(color: [u8; 3], palette: &[[u8; 3]; 256], slots: &[u8]) -> u8 {
    slots
        .iter()
        .copied()
        .min_by(|a, b| {
            color_distance(color, palette[*a as usize])
                .total_cmp(&color_distance(color, palette[*b as usize]))
                .then(a.cmp(b))
        })
        .unwrap_or(1)
}

/// Fill the visual hull of `spec.views`. `palette` quantises image colours.
pub fn carve_from_views(spec: &CarveSpec, palette: &[[u8; 3]; 256]) -> Result<Carved, String> {
    let depths = spec.views.iter().any(|view| view.depth.is_some());
    if !((if depths { 1 } else { 2 })..=6).contains(&spec.views.len()) {
        return Err(
            "carve_from_views takes 2 to 6 views (at most one per side), or 1 with depth".into(),
        );
    }
    if !(0.0..=64.0).contains(&spec.depth_tolerance) {
        return Err("depthTolerance must be 0..64 cells".into());
    }
    for view in &spec.views {
        if let Some(depth) = &view.depth {
            depth
                .check()
                .map_err(|err| format!("{} view: {err}", view.side.name()))?;
        }
    }
    for (i, view) in spec.views.iter().enumerate() {
        if spec.views[..i].iter().any(|other| other.side == view.side) {
            return Err(format!("two views look from the {}", view.side.name()));
        }
    }
    if !(0.0..=1.0).contains(&spec.coverage) {
        return Err("coverage must be 0..1".into());
    }
    if spec.shell == Some(0) || spec.shell.is_some_and(|n| n > MAX_CARVE_PASSES) {
        return Err(format!("shell must be 1..={MAX_CARVE_PASSES}"));
    }
    if spec.smooth > MAX_CARVE_PASSES {
        return Err(format!("smooth must be 0..={MAX_CARVE_PASSES}"));
    }
    if let Colour::Views { slots } = &spec.colour {
        if slots.is_empty() || slots.contains(&0) {
            return Err("slots must be palette indices 1..255".into());
        }
        if spec
            .views
            .iter()
            .any(|view| view.depth.is_none() && view.mask.pixels.contains(&Some(Ink::Plain)))
        {
            return Err(
                "a rows character has no legend entry: add it to legend or pass palette".into(),
            );
        }
        if spec.views.iter().all(|view| {
            !view
                .mask
                .pixels
                .iter()
                .flatten()
                .any(|ink| *ink != Ink::Plain)
        }) {
            return Err("no view has a colour image: pass palette, or path on a view".into());
        }
    }
    // Each view's grid size, before the box is known for `Fit`.
    let mut grids: Vec<Option<[u32; 2]>> = Vec::new();
    for (i, view) in spec.views.iter().enumerate() {
        grids.push(match view.scale {
            Scale::CellsPerPixel(c) => {
                if !(c > 0.0 && c <= 64.0) {
                    return Err(format!(
                        "view {i} cellsPerPixel must be above 0, at most 64"
                    ));
                }
                let side = |pixels: u32| ((pixels as f32 * c).round() as u32).max(1);
                Some([side(view.mask.width), side(view.mask.height)])
            }
            Scale::Size([w, h]) => {
                if w == 0 || h == 0 || w > 2048 || h > 2048 {
                    return Err(format!("view {i} size must be 1..2048 cells a side"));
                }
                Some([w, h])
            }
            Scale::Fit => None,
        });
    }
    let mut size = [0u32; 3];
    for axis in 0..3 {
        if let Some(given) = spec.size[axis] {
            if given == 0 || given > 2048 {
                return Err("size must be 1..2048 cells a side".into());
            }
            size[axis] = given;
            continue;
        }
        let mut covered = false;
        for (view, grid) in spec.views.iter().zip(&grids) {
            let [(u, _), (v, _), _] = view.side.axes();
            if u != axis && v != axis {
                continue;
            }
            covered = true;
            if let Some([w, h]) = grid {
                size[axis] = size[axis].max(if u == axis { *w } else { *h });
            }
        }
        if !covered {
            return Err(format!(
                "no view spans {}: front and back span x and z, left and right y and z, top and bottom x and y",
                ["x", "y", "z"][axis]
            ));
        }
        if size[axis] == 0 {
            return Err(format!(
                "the size along {} is unknown: pass size, or cellsPerPixel on a view across it",
                ["x", "y", "z"][axis]
            ));
        }
    }
    let total = i64::from(size[0]) * i64::from(size[1]) * i64::from(size[2]);
    if total > MAX_CARVE_CELLS {
        return Err(format!(
            "the box is {}x{}x{} = {total} cells, over {MAX_CARVE_CELLS}",
            size[0], size[1], size[2]
        ));
    }
    let mut built = Vec::new();
    let mut reports = Vec::new();
    for (view, grid) in spec.views.iter().zip(&grids) {
        let [(u, _), (v, _), _] = view.side.axes();
        let [cols, rows] = grid.unwrap_or([size[u], size[v]]);
        let mut grid = Grid::build(&view.mask, cols, rows, spec.coverage);
        // Centred across; a vertical axis stands on the bottom of the box.
        let place = |axis: usize, n: u32| {
            if axis == 2 {
                0
            } else {
                (i64::from(size[axis]) - i64::from(n)).div_euclid(2)
            }
        };
        grid.offset = [place(u, cols), place(v, rows)];
        reports.push(ViewReport {
            side: view.side,
            grid: [cols, rows],
            covered: grid.inside.iter().filter(|inside| **inside).count(),
            depth: None,
        });
        built.push(grid);
    }
    let [sx, sy, sz] = size.map(|n| n as usize);
    let index = |p: [usize; 3]| (p[2] * sy + p[1]) * sx + p[0];
    let mut solid = vec![false; sx * sy * sz];
    for z in 0..sz {
        for y in 0..sy {
            for x in 0..sx {
                let p = [x as i64, y as i64, z as i64];
                solid[index([x, y, z])] = spec.views.iter().zip(&built).all(|(view, grid)| {
                    let [(u, _), (v, _), _] = view.side.axes();
                    grid.at(view.side, [p[u], p[v]])
                        .is_some_and(|cell| grid.inside[cell])
                });
            }
        }
    }
    carve_depths(spec, &built, size, &mut solid, &mut reports);
    if spec.smooth > 0 {
        let cells: HashMap<Cell, u8> = (0..sx * sy * sz)
            .filter(|i| solid[*i])
            .map(|i| {
                let (x, y, z) = (i % sx, (i / sx) % sy, i / (sx * sy));
                ((x as i32, y as i32, z as i32), 1)
            })
            .collect();
        let max = (sx as i32 - 1, sy as i32 - 1, sz as i32 - 1);
        let smoothed = crate::tools::smooth_checked(&cells, (0, 0, 0), max, spec.smooth, 17, 9)?;
        for (x, y, z, slot) in smoothed.writes {
            solid[index([x as usize, y as usize, z as usize])] = slot != 0;
        }
    }
    let filled = |p: [i64; 3]| {
        (0..3).all(|a| (0..size[a] as i64).contains(&p[a]))
            && solid[index([p[0] as usize, p[1] as usize, p[2] as usize])]
    };
    // Distance in cells, along each view's depth axis, from the view's
    // visible surface: the first solid cell its ray meets.
    let surface: Vec<Vec<i64>> = spec
        .views
        .iter()
        .map(|view| {
            let [(u, _), (v, _), (d, toward)] = view.side.axes();
            let (nu, nv, nd) = (size[u] as usize, size[v] as usize, size[d] as i64);
            let mut first = vec![i64::MIN; nu * nv];
            for a in 0..nu {
                for b in 0..nv {
                    for step in 0..nd {
                        let depth = if toward > 0 { nd - 1 - step } else { step };
                        let mut p = [0i64; 3];
                        p[u] = a as i64;
                        p[v] = b as i64;
                        p[d] = depth;
                        if filled(p) {
                            first[b * nu + a] = depth;
                            break;
                        }
                    }
                }
            }
            first
        })
        .collect();
    let mut keep = solid.clone();
    if let Some(shell) = spec.shell {
        // Erode `shell` times by the six face neighbours; what is left
        // inside is hollowed out.
        let mut core = solid.clone();
        for _ in 0..shell {
            let next: Vec<bool> = (0..core.len())
                .map(|i| {
                    if !core[i] {
                        return false;
                    }
                    let p = [
                        (i % sx) as i64,
                        ((i / sx) % sy) as i64,
                        (i / (sx * sy)) as i64,
                    ];
                    (0..3).all(|a| {
                        [-1, 1].into_iter().all(|s| {
                            let mut q = p;
                            q[a] += s;
                            (0..3).all(|b| (0..size[b] as i64).contains(&q[b]))
                                && core[index([q[0] as usize, q[1] as usize, q[2] as usize])]
                        })
                    })
                })
                .collect();
            core = next;
        }
        for (k, c) in keep.iter_mut().zip(core) {
            *k &= !c;
        }
    }
    let mut lookup: HashMap<[u8; 3], u8> = HashMap::new();
    let mut cells = Vec::new();
    let mut bounds: Option<(Cell, Cell)> = None;
    for z in 0..sz {
        for y in 0..sy {
            for x in 0..sx {
                if !keep[index([x, y, z])] {
                    continue;
                }
                let p = [x as i64, y as i64, z as i64];
                let slot = match &spec.colour {
                    Colour::Slot(slot) => *slot,
                    Colour::Views { slots } => {
                        let mut best: Option<(i64, Ink)> = None;
                        for ((view, grid), first) in spec.views.iter().zip(&built).zip(&surface) {
                            let [(u, _), (v, _), (d, _)] = view.side.axes();
                            let Some(ink) = grid
                                .at(view.side, [p[u], p[v]])
                                .and_then(|cell| grid.ink[cell])
                                .filter(|ink| *ink != Ink::Plain)
                            else {
                                continue;
                            };
                            let top = first[p[v] as usize * size[u] as usize + p[u] as usize];
                            let depth = (p[d] - top).abs();
                            if best.is_none_or(|(nearest, _)| depth < nearest) {
                                best = Some((depth, ink));
                            }
                        }
                        match best.map(|(_, ink)| ink) {
                            Some(Ink::Slot(slot)) => slot,
                            Some(Ink::Rgb(color)) => *lookup
                                .entry(color)
                                .or_insert_with(|| quantize(color, palette, slots)),
                            _ => slots[0],
                        }
                    }
                };
                if slot == 0 {
                    continue;
                }
                let cell = (
                    spec.origin.0 + x as i32,
                    spec.origin.1 + y as i32,
                    spec.origin.2 + z as i32,
                );
                bounds = Some(match bounds {
                    None => (cell, cell),
                    Some((lo, hi)) => (
                        (lo.0.min(cell.0), lo.1.min(cell.1), lo.2.min(cell.2)),
                        (hi.0.max(cell.0), hi.1.max(cell.1), hi.2.max(cell.2)),
                    ),
                });
                cells.push((cell, slot));
            }
        }
    }
    Ok(Carved {
        cells,
        size,
        bounds,
        views: reports,
    })
}
