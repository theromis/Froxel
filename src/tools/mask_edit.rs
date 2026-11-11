//! Cleaning a concept mask before it is compared or carved: a hero view or
//! a photo has water, splashes, a rider, or a gradient sky in it, so the
//! background rule alone leaves more inked than the creature. In order:
//!
//! 1. `exclude`: pixels within a tolerance of listed colours go;
//! 2. `flood`: a fill from every pixel of the crop's border, stepping to a
//!    neighbour whose colour is within the tolerance of the pixel it comes
//!    from, removes the background it reaches (a soft gradient or a photo);
//! 3. `mask`: a second image cut like a view, whose inked pixels keep (or,
//!    inverted, remove) the concept's pixels under them;
//! 4. `erase`: rectangles of pixels go;
//! 5. `keep_largest`: only the largest 8-connected inked region stays;
//! 6. `fill_holes`: background regions the ink encloses (not reaching the
//!    crop's border by 4-neighbours) up to a share of the ink are inked
//!    again in their own colour: white eyes and teeth on a white sheet are
//!    keyed as background by the background rule, so a view loses them.
//!
//! MCP `compare_silhouette` and the views of `carve_from_views`,
//! `project_paint`, and `fit_to_views` take these keys.

use std::collections::VecDeque;

use super::silhouette::{color_distance, Crop, Ink, Mask};

/// Most rectangles in one `erase`.
pub const MAX_ERASE: usize = 256;
/// Most colours in one `exclude`.
pub const MAX_EXCLUDE: usize = 64;

/// What to remove from a mask.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MaskEdits {
    pub exclude: Vec<[u8; 3]>,
    pub exclude_tolerance: f32,
    /// The colour step of the border flood fill.
    pub flood: Option<f32>,
    /// A mask of any size, stretched over this one (nearest pixel).
    pub mask: Option<Mask>,
    pub invert_mask: bool,
    /// Rectangles `[x, y, width, height]` in the mask's own pixels (the
    /// crop's, so image pixels less the crop's corner).
    pub erase: Vec<[i64; 4]>,
    pub keep_largest: bool,
    /// Enclosed background regions up to this share of the inked pixels
    /// are inked again with the image's colour.
    pub fill_holes: Option<f32>,
}

impl MaskEdits {
    pub fn is_empty(&self) -> bool {
        self.exclude.is_empty()
            && self.flood.is_none()
            && self.mask.is_none()
            && self.erase.is_empty()
            && !self.keep_largest
            && self.fill_holes.is_none()
    }
}

/// Inked pixels each step removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaskEditReport {
    pub before: usize,
    pub excluded: usize,
    pub flooded: usize,
    pub masked: usize,
    pub erased: usize,
    pub dropped: usize,
    /// Pixels `fill_holes` inked.
    pub filled: usize,
    /// Enclosed regions `fill_holes` inked.
    pub holes: usize,
    pub after: usize,
}

/// The pixels of an image a mask was cut from: the RGBA bytes, the image
/// width, and the crop.
pub struct Source<'a> {
    pub rgba: &'a [u8],
    pub width: u32,
    pub crop: Crop,
}

impl Source<'_> {
    fn rgb(&self, x: u32, y: u32) -> [u8; 3] {
        let i = (((self.crop[1] + y) * self.width + self.crop[0] + x) * 4) as usize;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2]]
    }
}

/// Apply `edits` to `mask`. `source` is the image it was cut from; the
/// flood fill needs it, as background pixels hold no colour in the mask.
pub fn edit_mask(
    mask: &mut Mask,
    source: Option<&Source>,
    edits: &MaskEdits,
) -> Result<MaskEditReport, String> {
    let mut report = MaskEditReport {
        before: mask.count(),
        ..Default::default()
    };
    let (w, h) = (mask.width, mask.height);
    if !edits.exclude.is_empty() {
        for (i, pixel) in mask.pixels.iter_mut().enumerate() {
            let colour = match *pixel {
                Some(Ink::Rgb(rgb)) => rgb,
                _ => match source {
                    Some(src) if pixel.is_some() => src.rgb(i as u32 % w, i as u32 / w),
                    _ => continue,
                },
            };
            if edits
                .exclude
                .iter()
                .any(|key| color_distance(colour, *key) <= edits.exclude_tolerance)
            {
                *pixel = None;
                report.excluded += 1;
            }
        }
    }
    if let Some(step) = edits.flood {
        let src = source.ok_or("flood needs an image (path), not rows")?;
        let mut seen = vec![false; (w * h) as usize];
        let mut queue = VecDeque::new();
        for x in 0..w {
            for y in [0, h - 1] {
                queue.push_back((x, y));
            }
        }
        for y in 0..h {
            for x in [0, w - 1] {
                queue.push_back((x, y));
            }
        }
        for &(x, y) in &queue {
            seen[(y * w + x) as usize] = true;
        }
        while let Some((x, y)) = queue.pop_front() {
            let here = src.rgb(x, y);
            let i = (y * w + x) as usize;
            if mask.pixels[i].take().is_some() {
                report.flooded += 1;
            }
            let mut next = |nx: u32, ny: u32| {
                let j = (ny * w + nx) as usize;
                if !seen[j] && color_distance(here, src.rgb(nx, ny)) <= step {
                    seen[j] = true;
                    queue.push_back((nx, ny));
                }
            };
            if x > 0 {
                next(x - 1, y);
            }
            if x + 1 < w {
                next(x + 1, y);
            }
            if y > 0 {
                next(x, y - 1);
            }
            if y + 1 < h {
                next(x, y + 1);
            }
        }
    }
    if let Some(keep) = &edits.mask {
        if keep.width == 0 || keep.height == 0 {
            return Err("mask is empty".into());
        }
        for y in 0..h {
            let my = ((u64::from(y) * 2 + 1) * u64::from(keep.height) / (u64::from(h) * 2)) as u32;
            for x in 0..w {
                let mx =
                    ((u64::from(x) * 2 + 1) * u64::from(keep.width) / (u64::from(w) * 2)) as u32;
                let on = keep.inked(mx.min(keep.width - 1), my.min(keep.height - 1));
                let i = (y * w + x) as usize;
                if on == edits.invert_mask && mask.pixels[i].take().is_some() {
                    report.masked += 1;
                }
            }
        }
    }
    for [x, y, rw, rh] in &edits.erase {
        let x0 = (*x).clamp(0, i64::from(w)) as u32;
        let y0 = (*y).clamp(0, i64::from(h)) as u32;
        let x1 = (x + rw).clamp(0, i64::from(w)) as u32;
        let y1 = (y + rh).clamp(0, i64::from(h)) as u32;
        for py in y0..y1 {
            for px in x0..x1 {
                if mask.pixels[(py * w + px) as usize].take().is_some() {
                    report.erased += 1;
                }
            }
        }
    }
    if edits.keep_largest {
        report.dropped = keep_largest(mask);
    }
    if let Some(share) = edits.fill_holes {
        let src = source.ok_or("fillHoles needs an image (path), not rows")?;
        let (holes, filled) = fill_holes(mask, src, share);
        report.holes = holes;
        report.filled = filled;
    }
    report.after = mask.count();
    if report.after == 0 {
        return Err(format!(
            "the mask edits left no inked pixel ({} before)",
            report.before
        ));
    }
    Ok(report)
}

/// Ink every background region of `mask` that does not reach its border
/// (4-neighbours) and holds at most `share` of its inked pixels, in the
/// colour of `src`; returns the regions and the pixels inked.
fn fill_holes(mask: &mut Mask, src: &Source, share: f32) -> (usize, usize) {
    let (w, h) = (mask.width as usize, mask.height as usize);
    let limit = (f64::from(share) * mask.count() as f64).floor() as usize;
    let mut seen = vec![false; w * h];
    let mut queue = VecDeque::new();
    let (mut holes, mut filled) = (0, 0);
    for start in 0..w * h {
        if mask.pixels[start].is_some() || seen[start] {
            continue;
        }
        let mut region = vec![start];
        let mut border = false;
        seen[start] = true;
        queue.push_back(start);
        while let Some(i) = queue.pop_front() {
            let (x, y) = (i % w, i / w);
            if x == 0 || y == 0 || x + 1 == w || y + 1 == h {
                border = true;
            }
            let mut next = |j: usize| {
                if mask.pixels[j].is_none() && !seen[j] {
                    seen[j] = true;
                    region.push(j);
                    queue.push_back(j);
                }
            };
            if x > 0 {
                next(i - 1);
            }
            if x + 1 < w {
                next(i + 1);
            }
            if y > 0 {
                next(i - w);
            }
            if y + 1 < h {
                next(i + w);
            }
        }
        if border || region.len() > limit {
            continue;
        }
        holes += 1;
        filled += region.len();
        for i in region {
            mask.pixels[i] = Some(Ink::Rgb(src.rgb((i % w) as u32, (i / w) as u32)));
        }
    }
    (holes, filled)
}

/// Keep the largest 8-connected inked region; returns the pixels removed.
fn keep_largest(mask: &mut Mask) -> usize {
    let (w, h) = (mask.width as i64, mask.height as i64);
    let mut label = vec![0u32; mask.pixels.len()];
    let mut sizes = vec![0usize];
    let mut queue = VecDeque::new();
    for start in 0..mask.pixels.len() {
        if mask.pixels[start].is_none() || label[start] != 0 {
            continue;
        }
        let id = sizes.len() as u32;
        sizes.push(0);
        label[start] = id;
        queue.push_back(start);
        while let Some(i) = queue.pop_front() {
            sizes[id as usize] += 1;
            let (x, y) = (i as i64 % w, i as i64 / w);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w || ny >= h {
                        continue;
                    }
                    let j = (ny * w + nx) as usize;
                    if mask.pixels[j].is_some() && label[j] == 0 {
                        label[j] = id;
                        queue.push_back(j);
                    }
                }
            }
        }
    }
    let Some(best) = (1..sizes.len()).max_by_key(|&id| (sizes[id], usize::MAX - id)) else {
        return 0;
    };
    let mut dropped = 0;
    for (pixel, id) in mask.pixels.iter_mut().zip(&label) {
        if pixel.is_some() && *id as usize != best {
            *pixel = None;
            dropped += 1;
        }
    }
    dropped
}

#[cfg(test)]
#[path = "../../tests/mask_edit.rs"]
mod tests;
