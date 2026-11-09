//! MCP `paint_from_views`: recolour every surface cell of an object from a
//! whole orthographic views sheet in one call.
//!
//! An image-to-3D shape is good from the front and smeared on the sides it
//! never saw. Each surface cell here takes its colour from the view its
//! outward normal faces most (or a blend of the views it faces), but only
//! from a view that sees it: a cell is visible from an axis view when it is
//! the first solid cell of its column from that side, so a flipper never
//! paints the shell behind it. Each view's ink is stretched over the
//! object's box as the view sees it, so the sheet and the shape need the
//! same proportions, not the same size. Colours are averaged over the
//! cell's footprint in the image, blended in Oklab, then quantised into
//! palette slots or into a few new slots by clustering. Cells are never
//! added or removed.

use std::collections::HashMap;

use super::colors::{self, Guard, Lab};
use super::project::surface_normal;
use super::{Ink, Mask, Shades, Side};
use crate::state::Cell;

/// Most new slots `newSlots` asks for.
pub const MAX_NEW_SLOTS: usize = 64;

/// One view of the sheet.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetImage {
    pub side: Side,
    pub mask: Mask,
}

/// How a cell's views combine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Blend {
    /// Every view that sees the cell, weighted by `max(0, n · view)` to the
    /// power `sharpness`.
    Soft { sharpness: f32 },
    /// The view the normal faces most; within `seam` degrees of the
    /// boundary between the two best views, a linear blend of both.
    Nearest { seam: f32 },
}

/// Where the painted colours land.
#[derive(Clone, Debug, PartialEq)]
pub enum Quantize {
    /// The nearest of these slots.
    Slots(Vec<u8>),
    /// Up to this many new colours, clustered from the painted ones, in
    /// free slots (an existing slot of exactly the colour is reused).
    New(usize),
}

/// What part of each view's image maps onto the object's box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fit {
    /// The ink's bounds (the default): a loose crop still lines up.
    Ink,
    /// The whole crop.
    Crop,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaintViewsSpec {
    pub views: Vec<SheetImage>,
    pub blend: Blend,
    /// A missing right view is the left one mirrored, and the other way.
    pub mirror: bool,
    pub fit: Fit,
    /// Cells: a footprint outside a view's ink takes the nearest ink
    /// within this distance; further out the view gives no colour.
    pub reach: f32,
    /// Pixels: colours are read from the ink this far inside the
    /// silhouette's edge, so anti-aliased fringes of the background never
    /// land on the model (a view whose ink all lies closer keeps its edge).
    pub inset: u32,
    /// Only cells inside this inclusive box (in `cells`' coordinates) are
    /// painted; every cell still hides what is behind it.
    pub region: Option<(Cell, Cell)>,
    /// Slots that may be repainted.
    pub only: Option<Vec<u8>>,
    pub quantize: Quantize,
    /// Keeps small distinct colours their own new slot (`New` only).
    pub guard: Option<Guard>,
}

/// What one view did.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewCount {
    pub side: Side,
    /// Whether the view was mirrored from the opposite one.
    pub mirrored: bool,
    /// Cells the view sees.
    pub visible: usize,
    /// Cells it gave a colour to (any share).
    pub used: usize,
    /// Cells whose footprint missed the ink and took the nearest ink.
    pub snapped: usize,
    /// Cells it sees whose footprint was further than `reach` from ink.
    pub missed: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ViewsPainted {
    /// Every painted cell with its new slot, changed or not.
    pub cells: Vec<(Cell, u8)>,
    /// Surface cells (an empty face neighbour) in the region.
    pub surface: usize,
    /// Surface cells no view sees with ink (they keep their slot).
    pub unseen: usize,
    /// Cells blended from two or more views.
    pub blended: usize,
    pub views: Vec<ViewCount>,
    /// The slots the colours quantised into, with how many cells each.
    pub slots: Vec<(u8, usize)>,
    /// The slots made for new colours.
    pub new_slots: Vec<u8>,
    /// How many of the new slots the accent guard kept.
    pub accents: usize,
}

fn axis_vector(axis: usize, sign: i32) -> [f64; 3] {
    let mut v = [0.0; 3];
    v[axis] = f64::from(sign);
    v
}

fn coord(cell: Cell, axis: usize) -> i32 {
    match axis {
        0 => cell.0,
        1 => cell.1,
        _ => cell.2,
    }
}

/// A view prepared for sampling.
struct Prepared {
    side: Side,
    mirrored: bool,
    mask: Mask,
    /// Image right, image down, toward the camera: axis and sign.
    axes: [(usize, i32); 3],
    /// The ink rectangle the object's box maps onto, pixels, half-open.
    rect: [f64; 4],
    /// For every pixel, the nearest inked pixel and its distance in pixels.
    nearest: Vec<(u32, f32)>,
    /// Pixels per cell, across and down.
    scale: [f64; 2],
    /// First solid coordinate along the view of each column.
    first: HashMap<(i32, i32), i32>,
}

/// For every pixel the nearest inked pixel (two-pass chamfer distance).
fn nearest_ink(mask: &Mask) -> Vec<(u32, f32)> {
    let (w, h) = (mask.width as usize, mask.height as usize);
    let mut out: Vec<(u32, f32)> = (0..w * h)
        .map(|i| {
            if mask.pixels[i].is_some() {
                (i as u32, 0.0)
            } else {
                (u32::MAX, f32::MAX)
            }
        })
        .collect();
    let dist = |i: usize, j: u32| -> f32 {
        let (x0, y0) = ((i % w) as f32, (i / w) as f32);
        let (x1, y1) = ((j as usize % w) as f32, (j as usize / w) as f32);
        ((x0 - x1).powi(2) + (y0 - y1).powi(2)).sqrt()
    };
    let relax = |out: &mut Vec<(u32, f32)>, i: usize, n: usize| {
        let j = out[n].0;
        if j == u32::MAX {
            return;
        }
        let d = dist(i, j);
        if d < out[i].1 {
            out[i] = (j, d);
        }
    };
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if x > 0 {
                relax(&mut out, i, i - 1);
            }
            if y > 0 {
                relax(&mut out, i, i - w);
                if x > 0 {
                    relax(&mut out, i, i - w - 1);
                }
                if x + 1 < w {
                    relax(&mut out, i, i - w + 1);
                }
            }
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            if x + 1 < w {
                relax(&mut out, i, i + 1);
            }
            if y + 1 < h {
                relax(&mut out, i, i + w);
                if x + 1 < w {
                    relax(&mut out, i, i + w + 1);
                }
                if x > 0 {
                    relax(&mut out, i, i + w - 1);
                }
            }
        }
    }
    out
}

/// `mask` with `inset` pixels of ink peeled off its edge (4-neighbours,
/// the crop's border counting as background), or the mask itself when
/// nothing would be left.
fn eroded(mask: &Mask, inset: u32) -> Mask {
    let (w, h) = (mask.width as i64, mask.height as i64);
    let mut out = mask.clone();
    for _ in 0..inset {
        let prev = out.clone();
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                if prev.pixels[i].is_none() {
                    continue;
                }
                let edge = [(-1, 0), (1, 0), (0, -1), (0, 1)].iter().any(|(dx, dy)| {
                    let (nx, ny) = (x + dx, y + dy);
                    nx < 0
                        || ny < 0
                        || nx >= w
                        || ny >= h
                        || prev.pixels[(ny * w + nx) as usize].is_none()
                });
                if edge {
                    out.pixels[i] = None;
                }
            }
        }
        if out.count() == 0 {
            return prev;
        }
    }
    out
}

/// The Oklab of a pixel's ink.
fn ink_lab(ink: Ink, palette: &[[u8; 3]; 256]) -> Lab {
    match ink {
        Ink::Rgb(rgb) => colors::oklab(rgb),
        Ink::Slot(slot) => colors::oklab(palette[slot as usize]),
        Ink::Plain => colors::oklab([128, 128, 128]),
    }
}

impl Prepared {
    /// The mean colour of `cell`'s footprint in this view, and whether it
    /// had to snap to the nearest ink; `None` past `reach`.
    fn sample(
        &self,
        cell: Cell,
        lo: [i32; 3],
        hi: [i32; 3],
        reach: f32,
        palette: &[[u8; 3]; 256],
    ) -> Option<(Lab, bool)> {
        // Footprint along image right and image down, in pixels.
        let span = |k: usize| -> (f64, f64) {
            let (axis, sign) = self.axes[k];
            let n = f64::from(hi[axis] - lo[axis] + 1);
            let p = f64::from(coord(cell, axis));
            let (a, b) = if sign > 0 {
                (
                    (p - f64::from(lo[axis])) / n,
                    (p + 1.0 - f64::from(lo[axis])) / n,
                )
            } else {
                (
                    (f64::from(hi[axis]) + 1.0 - p - 1.0) / n,
                    (f64::from(hi[axis]) + 1.0 - p) / n,
                )
            };
            let (r0, r1) = if k == 0 {
                (self.rect[0], self.rect[2])
            } else {
                (self.rect[1], self.rect[3])
            };
            (r0 + a * (r1 - r0), r0 + b * (r1 - r0))
        };
        let (xa, xb) = span(0);
        let (ya, yb) = span(1);
        let (w, h) = (self.mask.width as i64, self.mask.height as i64);
        let range = |a: f64, b: f64, n: i64| -> (i64, i64) {
            let lo = (a - 0.5).ceil() as i64;
            let hi = (b - 0.5).ceil() as i64; // exclusive
            if hi <= lo {
                let c = ((a + b) / 2.0).floor() as i64;
                (c.clamp(0, n - 1), c.clamp(0, n - 1) + 1)
            } else {
                (lo.clamp(0, n), hi.clamp(0, n))
            }
        };
        let (x0, x1) = range(xa, xb, w);
        let (y0, y1) = range(ya, yb, h);
        let mut sum = [0.0f32; 3];
        let mut count = 0usize;
        for y in y0..y1 {
            for x in x0..x1 {
                if let Some(ink) = self.mask.get(x as u32, y as u32) {
                    let lab = ink_lab(ink, palette);
                    for a in 0..3 {
                        sum[a] += lab[a];
                    }
                    count += 1;
                }
            }
        }
        if count > 0 {
            return Some((sum.map(|s| s / count as f32), false));
        }
        let cx = (((xa + xb) / 2.0).floor() as i64).clamp(0, w - 1);
        let cy = (((ya + yb) / 2.0).floor() as i64).clamp(0, h - 1);
        let (j, d) = self.nearest[(cy * w + cx) as usize];
        if j == u32::MAX {
            return None;
        }
        let per_cell = self.scale[0].max(self.scale[1]) as f32;
        if d > reach * per_cell.max(1e-6) {
            return None;
        }
        let ink = self.mask.pixels[j as usize]?;
        Some((ink_lab(ink, palette), true))
    }

    fn sees(&self, cell: Cell) -> bool {
        let [(u, _), (v, _), (d, _)] = self.axes;
        self.first
            .get(&(coord(cell, u), coord(cell, v)))
            .is_some_and(|first| *first == coord(cell, d))
    }
}

/// Paint `spec`'s views onto `cells`. `palette` gives the slots' colours
/// (and a rows legend's); `shades` allocates new slots.
pub fn paint_from_views(
    cells: &HashMap<Cell, u8>,
    spec: &PaintViewsSpec,
    palette: &[[u8; 3]; 256],
    shades: &mut Shades,
) -> Result<ViewsPainted, String> {
    if spec.views.is_empty() {
        return Err("views must name at least one view".into());
    }
    match spec.blend {
        Blend::Soft { sharpness } if !(0.0..=64.0).contains(&sharpness) => {
            return Err("sharpness must be 0..64".into())
        }
        Blend::Nearest { seam } if !(0.0..=45.0).contains(&seam) => {
            return Err("seam must be 0..45 degrees".into())
        }
        _ => {}
    }
    if !(0.0..=64.0).contains(&spec.reach) {
        return Err("reach must be 0..64 cells".into());
    }
    match &spec.quantize {
        Quantize::Slots(slots) if slots.is_empty() || slots.contains(&0) => {
            return Err("slots must be palette indices 1..255".into())
        }
        Quantize::New(n) if *n == 0 || *n > MAX_NEW_SLOTS => {
            return Err(format!("newSlots must be 1..={MAX_NEW_SLOTS}"))
        }
        _ => {}
    }
    for (i, view) in spec.views.iter().enumerate() {
        if spec.views[..i].iter().any(|v| v.side == view.side) {
            return Err(format!("view {} is given twice", view.side.name()));
        }
        if view.mask.count() == 0 {
            return Err(format!("view {} has no ink", view.side.name()));
        }
    }
    let mut lo = [i32::MAX; 3];
    let mut hi = [i32::MIN; 3];
    for &(x, y, z) in cells.keys() {
        for (a, c) in [x, y, z].into_iter().enumerate() {
            lo[a] = lo[a].min(c);
            hi[a] = hi[a].max(c);
        }
    }
    if cells.is_empty() {
        return Err("there are no cells to paint".into());
    }
    // The views, with a missing right (or left) mirrored from its pair.
    let mut images: Vec<(Side, Mask, bool)> = spec
        .views
        .iter()
        .map(|v| (v.side, v.mask.clone(), false))
        .collect();
    if spec.mirror {
        for (have, want) in [(Side::Left, Side::Right), (Side::Right, Side::Left)] {
            let given = images.iter().any(|(s, _, _)| *s == want);
            if !given {
                if let Some((_, mask, false)) = images.iter().find(|(s, _, _)| *s == have) {
                    let flipped = mask.flipped();
                    images.push((want, flipped, true));
                }
            }
        }
    }
    let prepared: Vec<Prepared> = images
        .into_iter()
        .map(|(side, mask, mirrored)| {
            let axes = side.axes();
            let rect = match spec.fit {
                Fit::Crop => [0.0, 0.0, f64::from(mask.width), f64::from(mask.height)],
                Fit::Ink => {
                    let [x0, y0, x1, y1] = mask.bounds().unwrap_or([0, 0, 0, 0]);
                    [
                        f64::from(x0),
                        f64::from(y0),
                        f64::from(x1) + 1.0,
                        f64::from(y1) + 1.0,
                    ]
                }
            };
            let n = |k: usize| f64::from(hi[axes[k].0] - lo[axes[k].0] + 1);
            let scale = [(rect[2] - rect[0]) / n(0), (rect[3] - rect[1]) / n(1)];
            let [(u, _), (v, _), (d, sd)] = axes;
            let mut first: HashMap<(i32, i32), i32> = HashMap::new();
            for &cell in cells.keys() {
                let key = (coord(cell, u), coord(cell, v));
                let depth = coord(cell, d);
                first
                    .entry(key)
                    .and_modify(|f| {
                        if (sd > 0 && depth > *f) || (sd < 0 && depth < *f) {
                            *f = depth;
                        }
                    })
                    .or_insert(depth);
            }
            let mask = eroded(&mask, spec.inset);
            Prepared {
                side,
                mirrored,
                nearest: nearest_ink(&mask),
                mask,
                axes,
                rect,
                scale,
                first,
            }
        })
        .collect();
    let in_region = |c: Cell| {
        spec.region.is_none_or(|(a, b)| {
            c.0 >= a.0 && c.0 <= b.0 && c.1 >= a.1 && c.1 <= b.1 && c.2 >= a.2 && c.2 <= b.2
        })
    };
    let may_paint = |slot: u8| spec.only.as_ref().is_none_or(|only| only.contains(&slot));
    let mut counts: Vec<ViewCount> = prepared
        .iter()
        .map(|p| ViewCount {
            side: p.side,
            mirrored: p.mirrored,
            visible: 0,
            used: 0,
            snapped: 0,
            missed: 0,
        })
        .collect();
    let mut keys: Vec<Cell> = cells.keys().copied().collect();
    keys.sort_unstable();
    let faces = [
        (1, 0, 0),
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
    ];
    let mut colours: Vec<(Cell, Lab, u8)> = Vec::new();
    let (mut surface, mut unseen, mut blended) = (0usize, 0usize, 0usize);
    for cell in keys {
        let slot = cells[&cell];
        let mut exposed = [0.0f64; 3];
        let mut open = false;
        for f in faces {
            if !cells.contains_key(&(cell.0 + f.0, cell.1 + f.1, cell.2 + f.2)) {
                open = true;
                exposed[0] += f64::from(f.0);
                exposed[1] += f64::from(f.1);
                exposed[2] += f64::from(f.2);
            }
        }
        if !open || !in_region(cell) || !may_paint(slot) {
            continue;
        }
        surface += 1;
        let normal = surface_normal(cells, cell, exposed);
        // (view, cosine, colour)
        let mut seen: Vec<(usize, f64, Lab)> = Vec::new();
        for (i, view) in prepared.iter().enumerate() {
            if !view.sees(cell) {
                continue;
            }
            counts[i].visible += 1;
            let (axis, sign) = view.axes[2];
            let toward = axis_vector(axis, sign);
            let cos = normal[0] * toward[0] + normal[1] * toward[1] + normal[2] * toward[2];
            match view.sample(cell, lo, hi, spec.reach, palette) {
                Some((lab, snapped)) => {
                    if snapped {
                        counts[i].snapped += 1;
                    }
                    seen.push((i, cos, lab));
                }
                None => counts[i].missed += 1,
            }
        }
        if seen.is_empty() {
            unseen += 1;
            continue;
        }
        seen.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        let weights: Vec<f64> = match spec.blend {
            Blend::Soft { sharpness } => {
                let w: Vec<f64> = seen
                    .iter()
                    .map(|(_, cos, _)| cos.max(0.0).powf(f64::from(sharpness)))
                    .collect();
                if w.iter().sum::<f64>() <= 1e-9 {
                    // Facing none of them: the best one alone.
                    let mut w = vec![0.0; seen.len()];
                    w[0] = 1.0;
                    w
                } else {
                    w
                }
            }
            Blend::Nearest { seam } => {
                let mut w = vec![0.0; seen.len()];
                w[0] = 1.0;
                if seen.len() > 1 && seam > 0.0 {
                    let angle = |cos: f64| cos.clamp(-1.0, 1.0).acos().to_degrees();
                    let gap = angle(seen[1].1) - angle(seen[0].1);
                    if gap < f64::from(seam) {
                        let t = 0.5 + gap / (2.0 * f64::from(seam));
                        w[0] = t;
                        w[1] = 1.0 - t;
                    }
                }
                w
            }
        };
        let total: f64 = weights.iter().sum();
        let mut lab = [0.0f32; 3];
        let mut share = 0;
        for ((i, _, colour), w) in seen.iter().zip(&weights) {
            if *w <= 0.0 {
                continue;
            }
            share += 1;
            counts[*i].used += 1;
            for a in 0..3 {
                lab[a] += colour[a] * (*w / total) as f32;
            }
        }
        if share > 1 {
            blended += 1;
        }
        colours.push((cell, lab, slot));
    }
    // Quantise.
    let mut new_slots = Vec::new();
    let mut accents = 0;
    let (centres, centre_slots): (Vec<Lab>, Vec<u8>) = match &spec.quantize {
        Quantize::Slots(slots) => (
            slots
                .iter()
                .map(|s| colors::oklab(palette[*s as usize]))
                .collect(),
            slots.clone(),
        ),
        Quantize::New(n) => {
            let samples: Vec<(Lab, f32)> = colours.iter().map(|(_, lab, _)| (*lab, 1.0)).collect();
            let clusters = colors::cluster(&samples, *n, spec.guard);
            accents = clusters.accents;
            // The slot each centre lands in, with the material of the
            // slot most of its cells held.
            let mut from: Vec<HashMap<u8, usize>> = vec![HashMap::new(); clusters.centres.len()];
            for (_, lab, slot) in &colours {
                *from[colors::nearest(*lab, &clusters.centres)]
                    .entry(*slot)
                    .or_default() += 1;
            }
            let mut slots = Vec::with_capacity(clusters.centres.len());
            for (centre, from) in clusters.centres.iter().zip(&from) {
                let base = from
                    .iter()
                    .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
                    .map(|(s, _)| *s)
                    .unwrap_or(1);
                let before = shades.created.len();
                let slot = shades.colour(colors::srgb(*centre), base)?;
                if shades.created.len() > before {
                    new_slots.push(slot);
                }
                slots.push(slot);
            }
            new_slots.sort_unstable();
            new_slots.dedup();
            (clusters.centres, slots)
        }
    };
    let mut per_slot: HashMap<u8, usize> = HashMap::new();
    let painted: Vec<(Cell, u8)> = colours
        .iter()
        .map(|(cell, lab, _)| {
            let slot = centre_slots[colors::nearest(*lab, &centres)];
            *per_slot.entry(slot).or_default() += 1;
            (*cell, slot)
        })
        .collect();
    let mut slots: Vec<(u8, usize)> = per_slot.into_iter().collect();
    slots.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Ok(ViewsPainted {
        cells: painted,
        surface,
        unseen,
        blended,
        views: counts,
        slots,
        new_slots,
        accents,
    })
}

#[cfg(test)]
#[path = "../../tests/paint_views.rs"]
mod tests;
