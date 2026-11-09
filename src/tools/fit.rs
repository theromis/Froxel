//! MCP `fit_to_views`: refine an existing object so its orthographic views
//! match an approved views sheet, with colours from a palette.
//!
//! The idea is that of Voxify3D (Huang et al., "Voxify3D: Pixel Art Meets
//! Volumetric Rendering", CVPR 2026): orthographic parallel rays, so one
//! pixel of a view is one column of cells, and per view a colour loss on the
//! first visible cell, a mask loss that keeps the background empty, and an
//! optional depth loss against a reference. This is an independent CPU
//! implementation, not a port: no neural network, no CLIP, and a discrete
//! search instead of per-cell Gumbel-Softmax logits.
//!
//! Each view's crop is placed over the object's projection once, with the
//! placement of `compare_silhouette` (`fit` by default), and sampled to one
//! target per cell column: inside or background, and the mean colour of
//! its inked pixels. The energy of a view pixel is
//!
//! - `mask` when the target is inside and no cell covers it (missing) or
//!   the target is background and a cell does (excess);
//! - otherwise `depth * min(|d - d_ref|, 8) / 8` where a reference covers
//!   the pixel, `d` being the first visible cell's distance from the
//!   camera side of the box;
//! - plus, in the colour phase, `color * min(dist / 255, 1)` between the
//!   first visible cell's palette colour and the target;
//!
//! and each exposed cell face costs `smooth`, so a lone spike or pit costs
//! more than the pixel it wins.
//!
//! The search alternates two phases, each move kept only when it lowers the
//! energy of the pixels it touches (evaluated exactly, by re-casting those
//! rays):
//!
//! 1. Shape, on mask, depth, and smoothness only, so a colour is never a
//!    reason to carve: an excess ray is cleared; a missing ray is filled
//!    with a run of cells every other view sees as inside, within the
//!    depth span of the reference or of the neighbouring rays, or else with
//!    one such cell touching the body; with a reference, a covered ray's
//!    front is moved to the reference's front (that move leaves the surface
//!    term out, since one moved ray is a notch until its neighbours
//!    follow); then a cell with four or more exposed faces may go and an
//!    empty one with four or more solid neighbours may fill. Sweeps repeat,
//!    in an order shuffled by `seed`, until none is kept or `iterations`
//!    run out.
//! 2. Colour: every cell some view sees takes the slot that minimises the
//!    summed colour distance to the targets of the views that see it. This
//!    is the exact optimum for the shape, so it needs no annealing.
//!
//! Cells of other objects (`context`) cover and hide like the object's own
//! but never change. The result is deterministic for one seed.

use std::collections::HashMap;

use super::debris::Rng;
use super::silhouette::color_distance;
use super::{Ink, Mask, Side};
use crate::silhouette::Align;
use crate::state::Cell;

/// Most cells of the box the fit works in.
pub const MAX_FIT_CELLS: i64 = 4_000_000;
/// Most shape sweeps.
pub const MAX_FIT_ITERATIONS: u32 = 64;
/// Most margin cells around the object.
pub const MAX_FIT_MARGIN: u32 = 64;

#[derive(Clone, Debug, PartialEq)]
pub struct FitView {
    pub side: Side,
    pub mask: Mask,
    pub align: Align,
    /// The view's colours recolour cells; off uses it for the shape only.
    pub color: bool,
    /// The view's depth map: the depth target of its rays, in place of a
    /// reference object's front.
    pub depth: Option<super::depth::Depth>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FitSpec {
    pub views: Vec<FitView>,
    /// The slots colours may take; `None` is the slots the object holds.
    pub slots: Option<Vec<u8>>,
    /// Share of a cell column's footprint that must be inked, 0..1.
    pub coverage: f32,
    /// Most shape sweeps, 0..=64; 0 only recolours.
    pub iterations: u32,
    pub seed: u64,
    /// Empty cells around the bounds the fit may fill; `None` is 15% of the
    /// largest extent, at least 4.
    pub margin: Option<u32>,
    pub mask_weight: f32,
    pub color_weight: f32,
    pub depth_weight: f32,
    /// Cost of each exposed cell face: keeps the surface smooth, so a
    /// lone spike or pit costs more than it gains.
    pub smooth_weight: f32,
    /// Add and remove cells; off only recolours.
    pub shape: bool,
    /// Recolour the cells the views see; off keeps every slot.
    pub recolor: bool,
    /// Degrees: a view recolours only a cell whose surface faces it within
    /// this angle, so a feature does not wrap round a corner onto a face
    /// another view paints.
    pub normal_limit: f32,
    /// Slots whose cells the fit never removes or recolours (hand-set
    /// eyes, say); they still cover and hide.
    pub keep: Vec<u8>,
    /// Views placed with `fit` share one scale (the median of their own),
    /// with offsets refitted at it: a sheet is drawn at one scale.
    pub shared_scale: bool,
    /// Cost of keeping a cell the rounding prior puts outside the form, so
    /// cells no axis view can see (a box's diagonal corners) go while
    /// every silhouette stays; 0 (the default) keeps the maximal hull. It
    /// suits a round part such as a head; a flat-bottomed dome or a plate
    /// loses its rim to it.
    pub round_weight: f32,
    /// Exponent of the superellipse inscribed in each cross-section: 2 is
    /// an ellipse, larger is squarer.
    pub roundness: f32,
    /// The axes whose cross-sections the rounding prior rounds: y (front
    /// sections) and z (plan) by default. An x section runs along a
    /// creature facing −Y, through its neck, so its ellipse cuts the face.
    pub round_axes: [bool; 3],
    /// Cells in front of a depth map's surface that still count as on it.
    pub depth_tolerance: f32,
}

impl Default for FitSpec {
    fn default() -> Self {
        Self {
            views: Vec::new(),
            slots: None,
            coverage: 0.5,
            iterations: 8,
            seed: 1,
            margin: None,
            mask_weight: 1.0,
            color_weight: 1.0,
            depth_weight: 0.5,
            smooth_weight: 0.1,
            shape: true,
            recolor: true,
            normal_limit: 60.0,
            keep: Vec::new(),
            shared_scale: true,
            round_weight: 0.0,
            roundness: 2.5,
            round_axes: [false, true, true],
            depth_tolerance: 0.0,
        }
    }
}

/// How one view matches, in cell columns.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ViewScore {
    pub iou: f32,
    /// Share of the target the model does not cover.
    pub missing: f32,
    /// Share of the model outside the target.
    pub excess: f32,
    /// Mean RGB distance between the first visible cell and the target,
    /// over the columns both cover.
    pub color_error: f32,
    /// Mean cells between the first visible cell and the reference's, over
    /// the columns both cover; `None` without a reference.
    pub depth_error: Option<f32>,
    pub target: usize,
    pub model: usize,
    pub overlap: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FitViewReport {
    pub side: Side,
    /// The view's grid in cells, across and down.
    pub grid: [u32; 2],
    /// The placement: concept pixel `p` lands on cell column
    /// `p * scale + offset`.
    pub scale: f32,
    pub offset: [f32; 2],
    pub before: ViewScore,
    pub after: ViewScore,
    /// The depth map's calibration.
    pub depth: Option<super::depth::Calibration>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fitted {
    /// Every changed cell with its new slot, 0 removing it.
    pub writes: Vec<(Cell, u8)>,
    pub added: usize,
    pub removed: usize,
    pub recolored: usize,
    /// Shape sweeps run.
    pub sweeps: u32,
    /// Shape moves kept.
    pub moves: usize,
    /// Cells the rounding prior removed.
    pub rounded: usize,
    pub views: Vec<FitViewReport>,
    /// Total energy (mask, depth, and colour) before and after.
    pub energy: [f64; 2],
    /// The object's slots and their cell counts after the fit.
    pub palette: Vec<(u8, usize)>,
    /// The box worked in: minimum corner and size.
    pub origin: Cell,
    pub size: [u32; 3],
}

/// One view's target and its rays through the box.
struct ViewGrid {
    side: Side,
    w: usize,
    h: usize,
    depth: usize,
    inside: Vec<bool>,
    color: Vec<Option<[f32; 3]>>,
    /// The reference's first and last steps along each ray.
    reference: Vec<Option<(u32, u32)>>,
    scale: f32,
    offset: [f32; 2],
    /// Its colours count in the colour pass.
    paints: bool,
    depth_cal: Option<super::depth::Calibration>,
}

struct Fit<'a> {
    dims: [usize; 3],
    occ: Vec<u8>,
    fixed: Vec<bool>,
    /// Cells of other objects: they cover and hide, but the object grows
    /// only from its own cells.
    context: Vec<bool>,
    /// Cost of each cell being solid: the rounding or reference prior.
    prior: Vec<f32>,
    views: Vec<ViewGrid>,
    palette: &'a [[u8; 3]; 256],
    spec: &'a FitSpec,
    /// The colour term counts.
    colour: bool,
    energy: Vec<Vec<f64>>,
}

impl Fit<'_> {
    fn index(&self, p: [usize; 3]) -> usize {
        (p[2] * self.dims[1] + p[1]) * self.dims[0] + p[0]
    }

    fn point(&self, i: usize) -> [usize; 3] {
        let [sx, sy, _] = self.dims;
        [i % sx, (i / sx) % sy, i / (sx * sy)]
    }

    /// The cell at `step` from the camera on ray `pixel` of view `v`.
    fn cell_on(&self, v: usize, pixel: usize, step: usize) -> usize {
        let view = &self.views[v];
        let [(u, su), (va, sv), (d, toward)] = view.side.axes();
        let (col, row) = (pixel % view.w, pixel / view.w);
        let mut p = [0usize; 3];
        p[u] = if su > 0 { col } else { view.w - 1 - col };
        p[va] = if sv > 0 { row } else { view.h - 1 - row };
        p[d] = if toward > 0 {
            view.depth - 1 - step
        } else {
            step
        };
        self.index(p)
    }

    /// The pixel of view `v` a cell projects to, and its step.
    fn pixel_of(&self, v: usize, cell: usize) -> (usize, usize) {
        let view = &self.views[v];
        let p = self.point(cell);
        let [(u, su), (va, sv), (d, toward)] = view.side.axes();
        let col = if su > 0 { p[u] } else { view.w - 1 - p[u] };
        let row = if sv > 0 { p[va] } else { view.h - 1 - p[va] };
        let step = if toward > 0 {
            view.depth - 1 - p[d]
        } else {
            p[d]
        };
        (row * view.w + col, step)
    }

    /// The first and last solid steps on a ray.
    fn span(&self, v: usize, pixel: usize) -> Option<(usize, usize)> {
        let depth = self.views[v].depth;
        let first = (0..depth).find(|s| self.occ[self.cell_on(v, pixel, *s)] != 0)?;
        let last = (first..depth)
            .rev()
            .find(|s| self.occ[self.cell_on(v, pixel, *s)] != 0)?;
        Some((first, last))
    }

    fn first(&self, v: usize, pixel: usize) -> Option<usize> {
        let depth = self.views[v].depth;
        (0..depth).find(|s| self.occ[self.cell_on(v, pixel, *s)] != 0)
    }

    fn colour_cost(&self, slot: u8, target: [f32; 3]) -> f64 {
        let c = self.palette[slot as usize];
        let d: f32 = (0..3)
            .map(|k| (f32::from(c[k]) - target[k]).powi(2))
            .sum::<f32>()
            .sqrt();
        f64::from(self.spec.color_weight) * f64::from((d / 255.0).min(1.0))
    }

    fn pixel_energy(&self, v: usize, pixel: usize) -> f64 {
        let view = &self.views[v];
        let mask = f64::from(self.spec.mask_weight);
        let Some(step) = self.first(v, pixel) else {
            return if view.inside[pixel] { mask } else { 0.0 };
        };
        if !view.inside[pixel] {
            return mask;
        }
        let mut e = 0.0;
        if let Some((front, _)) = view.reference[pixel] {
            let off = (step as f64 - f64::from(front)).abs().min(8.0);
            e += f64::from(self.spec.depth_weight) * off / 8.0;
        }
        if self.colour {
            if let Some(target) = view.color[pixel] {
                e += self.colour_cost(self.occ[self.cell_on(v, pixel, step)], target);
            }
        }
        e
    }

    fn recompute(&mut self) {
        self.energy = (0..self.views.len())
            .map(|v| {
                (0..self.views[v].w * self.views[v].h)
                    .map(|p| self.pixel_energy(v, p))
                    .collect()
            })
            .collect();
    }

    fn total(&self) -> f64 {
        let faces: u64 = (0..self.occ.len()).map(|i| u64::from(self.faces(i))).sum();
        let prior: f64 = self
            .prior
            .iter()
            .zip(&self.occ)
            .filter(|(_, slot)| **slot != 0)
            .map(|(cost, _)| f64::from(*cost))
            .sum();
        self.energy.iter().flatten().sum::<f64>()
            + f64::from(self.spec.smooth_weight) * faces as f64
            + prior
    }

    /// The prior's cost of the cells in `changes` as they are now.
    fn prior_of(&self, changes: &[(usize, u8, u8)]) -> f64 {
        if self.prior.is_empty() {
            return 0.0;
        }
        changes
            .iter()
            .filter(|(i, _, _)| self.occ[*i] != 0)
            .map(|(i, _, _)| f64::from(self.prior[*i]))
            .sum()
    }

    /// The rounding prior: per axis, each cross-section of the object's own
    /// cells gets the superellipse inscribed in its bounds, and a cell
    /// costs `round_weight` for each axis whose section it lies outside.
    /// With a reference, a cell outside the reference costs three times
    /// `round_weight` instead: the reference is the shape where the views
    /// say nothing.
    fn build_prior(&mut self, reference: &[bool]) {
        let w = self.spec.round_weight;
        if w <= 0.0 {
            return;
        }
        let total = self.occ.len();
        if !reference.is_empty() {
            self.prior = (0..total)
                .map(|i| if reference[i] { 0.0 } else { 3.0 * w })
                .collect();
            return;
        }
        let p = f64::from(self.spec.roundness);
        let mut prior = vec![0.0f32; total];
        for axis in (0..3).filter(|a| self.spec.round_axes[*a]) {
            let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
            // Bounds of the own cells in each section along `axis`.
            let mut bounds: Vec<Option<[usize; 4]>> = vec![None; self.dims[axis]];
            for i in 0..total {
                if self.occ[i] == 0 || self.context[i] {
                    continue;
                }
                let q = self.point(i);
                let found = &mut bounds[q[axis]];
                *found = Some(match *found {
                    None => [q[a], q[a], q[b], q[b]],
                    Some([a0, a1, b0, b1]) => {
                        [a0.min(q[a]), a1.max(q[a]), b0.min(q[b]), b1.max(q[b])]
                    }
                });
            }
            for (i, cost) in prior.iter_mut().enumerate() {
                let q = self.point(i);
                let outside = match bounds[q[axis]] {
                    None => true,
                    Some([a0, a1, b0, b1]) => {
                        let term = |v: usize, lo: usize, hi: usize| -> f64 {
                            let centre = (lo + hi + 1) as f64 / 2.0;
                            let radius = (hi + 1 - lo) as f64 / 2.0;
                            ((v as f64 + 0.5 - centre) / radius).abs().powf(p)
                        };
                        term(q[a], a0, a1) + term(q[b], b0, b1) > 1.0 + 1e-9
                    }
                };
                if outside {
                    *cost += w;
                }
            }
        }
        self.prior = prior;
    }

    /// Remove surface cells the prior puts outside the form wherever no
    /// view loses for it, until none goes or `rounds` passes run out.
    fn round(&mut self, rounds: u32) -> usize {
        if self.prior.is_empty() {
            return 0;
        }
        let mut removed = 0;
        for _ in 0..rounds {
            let mut kept = 0;
            for i in 0..self.occ.len() {
                if self.occ[i] == 0 || self.fixed[i] || self.prior[i] <= 0.0 || self.faces(i) == 0 {
                    continue;
                }
                if self.attempt(&[(i, 0)]) {
                    kept += 1;
                }
            }
            removed += kept;
            if kept == 0 {
                break;
            }
        }
        removed
    }

    /// The face neighbours of cell `i` inside the box.
    fn neighbours(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        let p = self.point(i);
        (0..6).filter_map(move |k| {
            let (a, s) = (k / 2, if k % 2 == 0 { -1i64 } else { 1 });
            let q = p[a] as i64 + s;
            if q < 0 || q >= self.dims[a] as i64 {
                return None;
            }
            let mut n = p;
            n[a] = q as usize;
            Some(self.index(n))
        })
    }

    /// Exposed faces of cell `i`: 0 when it is empty.
    fn faces(&self, i: usize) -> u32 {
        if self.occ[i] == 0 {
            return 0;
        }
        let inside = self.neighbours(i).filter(|n| self.occ[*n] != 0).count() as u32;
        6 - inside
    }

    /// Apply `changes` (cell, new slot) if they lower the energy of the
    /// pixels they touch and of the surface; fixed cells are left out.
    /// Returns whether kept.
    fn attempt(&mut self, changes: &[(usize, u8)]) -> bool {
        self.attempt_with(changes, true)
    }

    /// [`Self::attempt`], with the surface term counted or not. A depth
    /// move leaves it out: one ray moved to the reference is a notch until
    /// its neighbours follow.
    fn attempt_with(&mut self, changes: &[(usize, u8)], smooth: bool) -> bool {
        let changes: Vec<(usize, u8, u8)> = changes
            .iter()
            .filter(|(i, slot)| !self.fixed[*i] && self.occ[*i] != *slot)
            .map(|&(i, slot)| (i, slot, self.occ[i]))
            .collect();
        if changes.is_empty() {
            return false;
        }
        let mut touched: Vec<(usize, usize)> = Vec::new();
        for v in 0..self.views.len() {
            for &(i, _, _) in &changes {
                touched.push((v, self.pixel_of(v, i).0));
            }
        }
        touched.sort_unstable();
        touched.dedup();
        let smooth = if smooth {
            f64::from(self.spec.smooth_weight)
        } else {
            0.0
        };
        let mut near: Vec<usize> = Vec::new();
        if smooth > 0.0 {
            for &(i, _, _) in &changes {
                near.push(i);
                near.extend(self.neighbours(i));
            }
            near.sort_unstable();
            near.dedup();
        }
        let surface = |fit: &Self| -> f64 {
            smooth * near.iter().map(|i| f64::from(fit.faces(*i))).sum::<f64>()
        };
        let before: f64 = touched.iter().map(|&(v, p)| self.energy[v][p]).sum::<f64>()
            + surface(self)
            + self.prior_of(&changes);
        for &(i, slot, _) in &changes {
            self.occ[i] = slot;
        }
        let after: Vec<f64> = touched
            .iter()
            .map(|&(v, p)| self.pixel_energy(v, p))
            .collect();
        if after.iter().sum::<f64>() + surface(self) + self.prior_of(&changes) < before - 1e-9 {
            for (&(v, p), e) in touched.iter().zip(after) {
                self.energy[v][p] = e;
            }
            true
        } else {
            for &(i, _, old) in &changes {
                self.occ[i] = old;
            }
            false
        }
    }

    /// Whether cell `i` has a face neighbour of the object's own.
    fn touches_own(&self, i: usize) -> bool {
        self.neighbours(i)
            .any(|n| self.occ[n] != 0 && !self.context[n])
    }

    /// Whether every view but `skip` sees cell `i` as inside its target.
    fn inside_others(&self, skip: usize, i: usize) -> bool {
        (0..self.views.len())
            .filter(|v| *v != skip)
            .all(|v| self.views[v].inside[self.pixel_of(v, i).0])
    }

    /// The slot nearest a target colour among `slots`.
    fn nearest(&self, target: [f32; 3], slots: &[u8]) -> u8 {
        slots
            .iter()
            .copied()
            .min_by(|a, b| {
                self.colour_cost(*a, target)
                    .total_cmp(&self.colour_cost(*b, target))
                    .then(a.cmp(b))
            })
            .unwrap_or(slots[0])
    }

    /// One shape sweep; returns the moves kept.
    fn sweep(&mut self, rng: &mut Rng, slots: &[u8], fill_slot: u8) -> usize {
        let mut kept = 0;
        let mut order: Vec<(usize, usize)> = (0..self.views.len())
            .flat_map(|v| (0..self.views[v].w * self.views[v].h).map(move |p| (v, p)))
            .collect();
        for i in (1..order.len()).rev() {
            let j = (rng.next() % (i as u64 + 1)) as usize;
            order.swap(i, j);
        }
        for (v, pixel) in order {
            let inside = self.views[v].inside[pixel];
            let span = self.span(v, pixel);
            let depth = self.views[v].depth;
            match (inside, span) {
                (false, Some((first, last))) => {
                    // Excess: clear the ray.
                    let changes: Vec<(usize, u8)> = (first..=last)
                        .map(|s| (self.cell_on(v, pixel, s), 0))
                        .collect();
                    kept += usize::from(self.attempt(&changes));
                }
                (true, None) => {
                    // Missing: fill the cells every other view sees inside.
                    let range = self.views[v].reference[pixel]
                        .map(|(a, b)| (a as usize, b as usize))
                        .or_else(|| self.neighbour_span(v, pixel))
                        .unwrap_or((0, depth - 1));
                    let slot =
                        self.views[v].color[pixel].map_or(fill_slot, |c| self.nearest(c, slots));
                    // Runs of cells every other view sees inside, each
                    // tried on its own, longest first.
                    let runs = |lo: usize, hi: usize| -> Vec<Vec<(usize, u8)>> {
                        let mut runs: Vec<Vec<(usize, u8)>> = Vec::new();
                        let mut open = false;
                        for s in lo..=hi.min(depth - 1) {
                            let i = self.cell_on(v, pixel, s);
                            if self.inside_others(v, i) {
                                if !open {
                                    runs.push(Vec::new());
                                    open = true;
                                }
                                runs.last_mut().expect("opened").push((i, slot));
                            } else {
                                open = false;
                            }
                        }
                        runs.sort_by(|a, b| b.len().cmp(&a.len()));
                        runs
                    };
                    // Only runs that touch the object's own cells: a gap
                    // beside another object is that object's to fill.
                    let touching = |found: Vec<Vec<(usize, u8)>>| -> Vec<Vec<(usize, u8)>> {
                        found
                            .into_iter()
                            .filter(|run| run.iter().any(|(i, _)| self.touches_own(*i)))
                            .collect()
                    };
                    let mut tries = touching(runs(range.0, range.1));
                    if tries.is_empty() {
                        tries = touching(runs(0, depth - 1));
                    }
                    // Then single cells that touch the body, nearest the
                    // middle of the span first: the smallest cover.
                    let middle = (range.0 + range.1) / 2;
                    let mut single: Vec<(usize, usize)> = tries
                        .iter()
                        .flatten()
                        .filter(|(i, _)| self.touches_own(*i))
                        .map(|(i, _)| (self.pixel_of(v, *i).1.abs_diff(middle), *i))
                        .collect();
                    single.sort_unstable();
                    tries.extend(single.into_iter().take(4).map(|(_, i)| vec![(i, slot)]));
                    for run in tries {
                        if self.attempt(&run) {
                            kept += 1;
                            break;
                        }
                    }
                }
                (true, Some((first, _))) => {
                    // A reference moves the ray's front to its front.
                    let Some((front, _)) = self.views[v].reference[pixel] else {
                        continue;
                    };
                    let front = front as usize;
                    if front == first || self.spec.depth_weight <= 0.0 {
                        continue;
                    }
                    let changes: Vec<(usize, u8)> = if front > first {
                        (first..front)
                            .map(|s| (self.cell_on(v, pixel, s), 0))
                            .collect()
                    } else {
                        let slot = self.occ[self.cell_on(v, pixel, first)];
                        (front..first)
                            .map(|s| (self.cell_on(v, pixel, s), slot))
                            .collect()
                    };
                    kept += usize::from(self.attempt_with(&changes, false));
                }
                (false, None) => {}
            }
        }
        kept + self.tidy()
    }

    /// Single-cell moves where the surface is rough: a changeable cell with
    /// four or more exposed faces may go, and an empty cell with four or
    /// more solid neighbours may fill with their commonest slot.
    fn tidy(&mut self) -> usize {
        if self.spec.smooth_weight <= 0.0 {
            return 0;
        }
        let mut kept = 0;
        for i in 0..self.occ.len() {
            if self.fixed[i] {
                continue;
            }
            if self.occ[i] != 0 {
                if self.faces(i) >= 4 && self.attempt(&[(i, 0)]) {
                    kept += 1;
                }
                continue;
            }
            let mut tally: Vec<(u8, u32)> = Vec::new();
            for n in self.neighbours(i) {
                let slot = self.occ[n];
                if slot == 0 || self.context[n] {
                    continue;
                }
                match tally.iter_mut().find(|(s, _)| *s == slot) {
                    Some(found) => found.1 += 1,
                    None => tally.push((slot, 1)),
                }
            }
            if tally.iter().map(|(_, n)| n).sum::<u32>() < 4 {
                continue;
            }
            let slot = tally
                .into_iter()
                .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
                .map(|(s, _)| s)
                .expect("four neighbours");
            if self.attempt(&[(i, slot)]) {
                kept += 1;
            }
        }
        kept
    }

    /// The depth span of the covered rays within two pixels.
    fn neighbour_span(&self, v: usize, pixel: usize) -> Option<(usize, usize)> {
        let view = &self.views[v];
        let (col, row) = ((pixel % view.w) as i64, (pixel / view.w) as i64);
        let mut found: Option<(usize, usize)> = None;
        for dy in -2i64..=2 {
            for dx in -2i64..=2 {
                let (x, y) = (col + dx, row + dy);
                if (dx, dy) == (0, 0)
                    || !(0..view.w as i64).contains(&x)
                    || !(0..view.h as i64).contains(&y)
                {
                    continue;
                }
                if let Some((a, b)) = self.span(v, (y * view.w as i64 + x) as usize) {
                    found = Some(match found {
                        None => (a, b),
                        Some((lo, hi)) => (lo.min(a), hi.max(b)),
                    });
                }
            }
        }
        found
    }

    /// Give every seen, changeable cell the slot that best matches the
    /// targets of the views that see it.
    fn recolour(&mut self, slots: &[u8]) {
        let mut seen: HashMap<usize, Vec<[f32; 3]>> = HashMap::new();
        // Cells no view faces within the limit: the view facing them most.
        let mut fallback: HashMap<usize, (f64, [f32; 3])> = HashMap::new();
        let cos_limit = f64::from(self.spec.normal_limit).to_radians().cos();
        let mut normals: HashMap<usize, [f64; 3]> = HashMap::new();
        for v in 0..self.views.len() {
            if !self.views[v].paints {
                continue;
            }
            let (axis, sign) = self.views[v].side.axes()[2];
            for pixel in 0..self.views[v].w * self.views[v].h {
                if !self.views[v].inside[pixel] {
                    continue;
                }
                let (Some(step), Some(target)) = (self.first(v, pixel), self.views[v].color[pixel])
                else {
                    continue;
                };
                let i = self.cell_on(v, pixel, step);
                if self.fixed[i] {
                    continue;
                }
                if self.spec.normal_limit < 90.0 {
                    let n = *normals.entry(i).or_insert_with(|| self.normal(i));
                    let facing = n[axis] * f64::from(sign);
                    if facing < cos_limit - 1e-9 {
                        let best = fallback.entry(i).or_insert((f64::MIN, target));
                        if facing > best.0 {
                            *best = (facing, target);
                        }
                        continue;
                    }
                }
                seen.entry(i).or_default().push(target);
            }
        }
        for (i, (_, target)) in fallback {
            seen.entry(i).or_insert_with(|| vec![target]);
        }
        let mut cells: Vec<_> = seen.into_iter().collect();
        cells.sort_unstable_by_key(|(i, _)| *i);
        for (i, targets) in cells {
            let best = slots
                .iter()
                .copied()
                .min_by(|a, b| {
                    let cost =
                        |s: u8| -> f64 { targets.iter().map(|t| self.colour_cost(s, *t)).sum() };
                    cost(*a).total_cmp(&cost(*b)).then(a.cmp(b))
                })
                .unwrap_or(self.occ[i]);
            self.occ[i] = best;
        }
    }

    /// The outward normal at cell `i`: the mean direction of the empty
    /// cells within two cells of it (as `project_paint` finds it); a cell
    /// with none around it has no direction and passes any limit.
    fn normal(&self, i: usize) -> [f64; 3] {
        let p = self.point(i).map(|c| c as i64);
        let mut sum = [0.0f64; 3];
        for dz in -2i64..=2 {
            for dy in -2i64..=2 {
                for dx in -2i64..=2 {
                    let r2 = dx * dx + dy * dy + dz * dz;
                    if r2 == 0 || r2 > 6 {
                        continue;
                    }
                    let q = [p[0] + dx, p[1] + dy, p[2] + dz];
                    let outside = (0..3).any(|a| q[a] < 0 || q[a] >= self.dims[a] as i64);
                    if !outside && self.occ[self.index(q.map(|c| c as usize))] != 0 {
                        continue;
                    }
                    let len = (r2 as f64).sqrt();
                    sum[0] += dx as f64 / len;
                    sum[1] += dy as f64 / len;
                    sum[2] += dz as f64 / len;
                }
            }
        }
        let len = (sum[0] * sum[0] + sum[1] * sum[1] + sum[2] * sum[2]).sqrt();
        if len < 0.5 {
            // No clear direction: count as facing every view.
            return [1.0, 1.0, 1.0];
        }
        sum.map(|c| c / len)
    }

    fn score(&self, v: usize) -> ViewScore {
        let view = &self.views[v];
        let mut s = ViewScore::default();
        let (mut colour_sum, mut colour_n) = (0.0f64, 0usize);
        let (mut depth_sum, mut depth_n) = (0.0f64, 0usize);
        for pixel in 0..view.w * view.h {
            let first = self.first(v, pixel);
            let inside = view.inside[pixel];
            s.target += usize::from(inside);
            s.model += usize::from(first.is_some());
            let Some(step) = first.filter(|_| inside) else {
                continue;
            };
            s.overlap += 1;
            if let Some(t) = view.color[pixel] {
                let c = self.palette[self.occ[self.cell_on(v, pixel, step)] as usize];
                colour_sum += f64::from(color_distance(
                    c,
                    t.map(|x| x.round().clamp(0.0, 255.0) as u8),
                ));
                colour_n += 1;
            }
            if let Some((front, _)) = view.reference[pixel] {
                depth_sum += (step as f64 - f64::from(front)).abs();
                depth_n += 1;
            }
        }
        let union = s.target + s.model - s.overlap;
        s.iou = if union == 0 {
            0.0
        } else {
            s.overlap as f32 / union as f32
        };
        s.missing = if s.target == 0 {
            0.0
        } else {
            (s.target - s.overlap) as f32 / s.target as f32
        };
        s.excess = if s.model == 0 {
            0.0
        } else {
            (s.model - s.overlap) as f32 / s.model as f32
        };
        s.color_error = if colour_n == 0 {
            0.0
        } else {
            (colour_sum / colour_n as f64) as f32
        };
        s.depth_error = view
            .reference
            .iter()
            .any(Option::is_some)
            .then(|| (depth_sum / depth_n.max(1) as f64) as f32);
        s
    }
}

/// Sample `mask` over one cell column `[col, col + 1) x [row, row + 1)`
/// under `p * scale + offset`: the inked share and the colour. Each pixel
/// votes for its nearest slot of `slots`, and the colour is the mean of the
/// pixels of the slot with the most weight, so a sheet's thin outlines and
/// cube bevels do not tint the fill they cross.
fn sample(
    mask: &Mask,
    scale: f32,
    offset: [f32; 2],
    (col, row): (usize, usize),
    palette: &[[u8; 3]; 256],
    slots: &[u8],
    nearest: &mut HashMap<[u8; 3], u8>,
) -> (f64, Option<[f32; 3]>) {
    let s = f64::from(scale);
    let x0 = (col as f64 - f64::from(offset[0])) / s;
    let x1 = (col as f64 + 1.0 - f64::from(offset[0])) / s;
    let y0 = (row as f64 - f64::from(offset[1])) / s;
    let y1 = (row as f64 + 1.0 - f64::from(offset[1])) / s;
    let (w, h) = (f64::from(mask.width), f64::from(mask.height));
    let area = (x1 - x0) * (y1 - y0);
    if area <= 0.0 || x1 <= 0.0 || y1 <= 0.0 || x0 >= w || y0 >= h {
        return (0.0, None);
    }
    let mut inked = 0.0f64;
    // Per slot: weight and weighted colour sum.
    let mut votes: Vec<(u8, f64, [f64; 3])> = Vec::new();
    let mut py = y0.max(0.0).floor();
    while py < y1.min(h) {
        let wy = (y1.min(py + 1.0) - y0.max(py)).max(0.0);
        let mut px = x0.max(0.0).floor();
        while px < x1.min(w) {
            let wx = (x1.min(px + 1.0) - x0.max(px)).max(0.0);
            let wgt = wx * wy;
            let rgb = match mask.get(px as u32, py as u32) {
                None => None,
                Some(Ink::Rgb(c)) => Some(c),
                Some(Ink::Slot(slot)) => Some(palette[slot as usize]),
                Some(Ink::Plain) => {
                    inked += wgt;
                    None
                }
            };
            if let Some(c) = rgb {
                inked += wgt;
                let slot = *nearest
                    .entry(c)
                    .or_insert_with(|| super::silhouette::quantize(c, palette, slots));
                let vote = match votes.iter_mut().position(|v| v.0 == slot) {
                    Some(found) => &mut votes[found],
                    None => {
                        votes.push((slot, 0.0, [0.0; 3]));
                        votes.last_mut().expect("pushed")
                    }
                };
                vote.1 += wgt;
                for k in 0..3 {
                    vote.2[k] += f64::from(c[k]) * wgt;
                }
            }
            px += 1.0;
        }
        py += 1.0;
    }
    let colour = votes
        .into_iter()
        .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
        .filter(|v| v.1 > 0.0)
        .map(|v| v.2.map(|c| (c / v.1) as f32));
    (inked / area, colour)
}

/// The offset, at `scale`, with the most overlap between the mask and the
/// model's coverage: a search of ±3 cells in quarter steps around the
/// placement that keeps the mask's centre where `placed` put it. Pixel
/// centres are sampled, as `compare_silhouette` does.
fn refit_offset(
    mask: &Mask,
    model: &[bool],
    w: usize,
    h: usize,
    placed: (f32, [f32; 2]),
    scale: f32,
) -> (f32, [f32; 2]) {
    let Some([x0, y0, x1, y1]) = mask.bounds() else {
        return placed;
    };
    let centre = [(x0 + x1 + 1) as f32 / 2.0, (y0 + y1 + 1) as f32 / 2.0];
    let (s0, o0) = placed;
    let start = [
        o0[0] + centre[0] * (s0 - scale),
        o0[1] + centre[1] * (s0 - scale),
    ];
    let iou = |offset: [f32; 2]| -> f64 {
        let (mut both, mut either) = (0usize, 0usize);
        for row in 0..h {
            let cy = ((row as f32 + 0.5 - offset[1]) / scale).floor();
            for col in 0..w {
                let cx = ((col as f32 + 0.5 - offset[0]) / scale).floor();
                let inked = cx >= 0.0
                    && cy >= 0.0
                    && (cx as u32) < mask.width
                    && (cy as u32) < mask.height
                    && mask.inked(cx as u32, cy as u32);
                let m = model[row * w + col];
                both += usize::from(inked && m);
                either += usize::from(inked || m);
            }
        }
        both as f64 / either.max(1) as f64
    };
    let mut best = (iou(start), start);
    for dy in -12i32..=12 {
        for dx in -12i32..=12 {
            let offset = [start[0] + dx as f32 * 0.25, start[1] + dy as f32 * 0.25];
            let score = iou(offset);
            if score > best.0 + 1e-9 {
                best = (score, offset);
            }
        }
    }
    (scale, best.1)
}

/// Each depth view's target: its map placed like its mask, calibrated
/// against the hull of the views' targets in the fit box, sets the front
/// (and with `back` the back) of every ray it covers, in place of a
/// reference object's.
fn depth_targets(fit: &mut Fit, spec: &FitSpec) {
    use super::depth;
    let sides: Vec<Side> = spec.views.iter().map(|view| view.side).collect();
    for (vi, view) in spec.views.iter().enumerate() {
        let Some(map) = &view.depth else {
            continue;
        };
        let (w, h, steps) = (fit.views[vi].w, fit.views[vi].h, fit.views[vi].depth);
        let (scale, offset) = (f64::from(fit.views[vi].scale), fit.views[vi].offset);
        let (mw, mh) = (f64::from(view.mask.width), f64::from(view.mask.height));
        let mut n = vec![None; w * h];
        let mut hull = vec![None; w * h];
        for row in 0..h {
            for col in 0..w {
                let p = row * w + col;
                if !fit.views[vi].inside[p] {
                    continue;
                }
                let x = |c: usize| (c as f64 - f64::from(offset[0])) / scale / mw;
                let y = |r: usize| (r as f64 - f64::from(offset[1])) / scale / mh;
                n[p] = map
                    .map
                    .sample(x(col), x(col + 1), y(row), y(row + 1))
                    .map(|grey| map.shaped(grey));
                hull[p] = (0..steps)
                    .find(|step| fit.inside_others(vi, fit.cell_on(vi, p, *step)))
                    .map(|step| step as f32);
            }
        }
        let [(u, _), (v, _), (d, _)] = view.side.axes();
        let lines = [depth::measured(&sides, v, d), depth::measured(&sides, u, d)];
        // The middle of the targets' hull along the view.
        let (mut lo, mut hi) = (usize::MAX, 0usize);
        for p in 0..w * h {
            if !fit.views[vi].inside[p] {
                continue;
            }
            for step in 0..steps {
                if fit.inside_others(vi, fit.cell_on(vi, p, step)) {
                    lo = lo.min(step);
                    hi = hi.max(step);
                }
            }
        }
        let middle = if lo == usize::MAX {
            steps as f32 / 2.0
        } else {
            (lo + hi + 1) as f32 / 2.0
        };
        let cal = depth::calibrate(map, &n, &hull, w, h, steps, lines, middle);
        let last = steps as f32 - 1.0;
        for (p, value) in n.iter().enumerate() {
            let Some(value) = value else {
                continue;
            };
            let (lo, hi) = depth::kept_span(map, &cal, *value, spec.depth_tolerance);
            let front = (lo - 0.5).ceil().clamp(0.0, last);
            let back = if hi.is_finite() {
                (hi - 0.5).floor().clamp(front, last)
            } else {
                last
            };
            fit.views[vi].reference[p] = Some((front as u32, back as u32));
        }
        fit.views[vi].depth_cal = Some(cal);
    }
}

/// Fit `cells` to `spec.views`. `context` cells (other objects, in the
/// same local frame) cover and hide but never change; `reference` cells
/// give the depth target. `palette` is the document's colours.
pub fn fit_to_views(
    cells: &HashMap<Cell, u8>,
    context: &[(Cell, u8)],
    reference: Option<&[Cell]>,
    spec: &FitSpec,
    palette: &[[u8; 3]; 256],
) -> Result<Fitted, String> {
    if spec.views.is_empty() || spec.views.len() > 6 {
        return Err("fit_to_views takes 1 to 6 views (at most one per side)".into());
    }
    for (i, view) in spec.views.iter().enumerate() {
        if spec.views[..i].iter().any(|other| other.side == view.side) {
            return Err(format!("two views look from the {}", view.side.name()));
        }
        if view.mask.count() == 0 {
            return Err(format!(
                "the {} view's mask is empty: check crop and background",
                view.side.name()
            ));
        }
        if let Align::Given { scale, offset } = view.align {
            if !(scale > 0.0 && scale.is_finite()) || !offset.iter().all(|v| v.is_finite()) {
                return Err("align scale must be above 0 and offset finite".into());
            }
        }
    }
    if !(0.0..=1.0).contains(&spec.coverage) {
        return Err("coverage must be 0..1".into());
    }
    if spec.iterations > MAX_FIT_ITERATIONS {
        return Err(format!("iterations must be 0..={MAX_FIT_ITERATIONS}"));
    }
    if spec.margin.is_some_and(|m| m > MAX_FIT_MARGIN) {
        return Err(format!("margin must be 0..={MAX_FIT_MARGIN}"));
    }
    for (name, w) in [
        ("maskWeight", spec.mask_weight),
        ("colorWeight", spec.color_weight),
        ("depthWeight", spec.depth_weight),
        ("smoothWeight", spec.smooth_weight),
    ] {
        if !(0.0..=100.0).contains(&w) {
            return Err(format!("{name} must be 0..100"));
        }
    }
    if !(0.0..=100.0).contains(&spec.round_weight) {
        return Err("roundWeight must be 0..100".into());
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
    if !(1.0..=16.0).contains(&spec.roundness) {
        return Err("roundness must be 1..16".into());
    }
    if !(0.0..=90.0).contains(&spec.normal_limit) {
        return Err("normalLimit must be 0..90 degrees".into());
    }
    if spec.keep.contains(&0) {
        return Err("keep must be palette indices 1..255".into());
    }
    if cells.is_empty() {
        return Err("the active object has no cells to fit: block it out first".into());
    }
    let slots: Vec<u8> = match &spec.slots {
        Some(list) => {
            if list.is_empty() || list.contains(&0) {
                return Err("slots must be palette indices 1..255".into());
            }
            list.clone()
        }
        None => {
            let mut used: Vec<u8> = cells.values().copied().collect();
            used.sort_unstable();
            used.dedup();
            used
        }
    };
    // The box: every cell involved, and a margin.
    let mut lo = [i32::MAX; 3];
    let mut hi = [i32::MIN; 3];
    let all = cells
        .keys()
        .copied()
        .chain(context.iter().map(|(c, _)| *c))
        .chain(reference.into_iter().flatten().copied());
    for (x, y, z) in all {
        for (a, v) in [x, y, z].into_iter().enumerate() {
            lo[a] = lo[a].min(v);
            hi[a] = hi[a].max(v);
        }
    }
    let extent = (0..3).map(|a| hi[a] - lo[a] + 1).max().unwrap_or(1);
    let margin = spec
        .margin
        .map_or(((extent as f32 * 0.15).ceil() as i32).max(4), |m| m as i32);
    let lo = lo.map(|v| v - margin);
    let dims: [usize; 3] = std::array::from_fn(|a| (hi[a] + margin - lo[a] + 1) as usize);
    let total = dims.iter().map(|d| *d as i64).product::<i64>();
    if total > MAX_FIT_CELLS {
        return Err(format!(
            "the fit box is {}x{}x{} = {total} cells, over {MAX_FIT_CELLS}: lower margin",
            dims[0], dims[1], dims[2]
        ));
    }
    let local = |c: Cell| -> Option<usize> {
        let p = [c.0 - lo[0], c.1 - lo[1], c.2 - lo[2]];
        (0..3)
            .all(|a| p[a] >= 0 && (p[a] as usize) < dims[a])
            .then(|| (p[2] as usize * dims[1] + p[1] as usize) * dims[0] + p[0] as usize)
    };
    let mut occ = vec![0u8; total as usize];
    let mut fixed = vec![false; total as usize];
    for &(cell, slot) in context {
        if let Some(i) = local(cell) {
            if slot != 0 {
                occ[i] = slot;
                fixed[i] = true;
            }
        }
    }
    let mut own = vec![false; total as usize];
    for (&cell, &slot) in cells {
        let i = local(cell).expect("inside the box");
        occ[i] = slot;
        fixed[i] = spec.keep.contains(&slot);
        own[i] = true;
    }
    let original = occ.clone();
    let mut refs = vec![
        false;
        if reference.is_some() {
            total as usize
        } else {
            0
        }
    ];
    for &cell in reference.into_iter().flatten() {
        refs[local(cell).expect("inside the box")] = true;
    }
    let mut fit = Fit {
        dims,
        occ,
        context: (0..own.len()).map(|i| fixed[i] && !own[i]).collect(),
        prior: Vec::new(),
        fixed,
        views: Vec::new(),
        palette,
        spec,
        colour: true,
        energy: Vec::new(),
    };
    // Each view's grid, and its placement over the model's coverage.
    let mut models = Vec::new();
    let mut placements = Vec::new();
    for view in &spec.views {
        let [(u, _), (v, _), (d, _)] = view.side.axes();
        let (w, h, depth) = (dims[u], dims[v], dims[d]);
        fit.views.push(ViewGrid {
            side: view.side,
            w,
            h,
            depth,
            inside: vec![false; w * h],
            color: vec![None; w * h],
            reference: vec![None; w * h],
            scale: 1.0,
            offset: [0.0, 0.0],
            paints: view.color,
            depth_cal: None,
        });
        let vi = fit.views.len() - 1;
        let model: Vec<bool> = (0..w * h).map(|p| fit.first(vi, p).is_some()).collect();
        placements.push(crate::silhouette::fit(
            &view.mask, &model, w as u32, h as u32, view.align,
        ));
        models.push(model);
    }
    let fitted: Vec<usize> = (0..spec.views.len())
        .filter(|v| spec.views[*v].align == Align::Fit)
        .collect();
    if spec.shared_scale && fitted.len() >= 2 {
        let mut scales: Vec<f32> = fitted.iter().map(|v| placements[*v].0).collect();
        scales.sort_by(f32::total_cmp);
        let shared = scales[scales.len() / 2];
        for &v in &fitted {
            let grid = &fit.views[v];
            placements[v] = refit_offset(
                &spec.views[v].mask,
                &models[v],
                grid.w,
                grid.h,
                placements[v],
                shared,
            );
        }
    }
    let mut nearest: HashMap<[u8; 3], u8> = HashMap::new();
    for (vi, view) in spec.views.iter().enumerate() {
        let (w, h, depth) = (fit.views[vi].w, fit.views[vi].h, fit.views[vi].depth);
        let (scale, offset) = placements[vi];
        let mut inside = vec![false; w * h];
        let mut color = vec![None; w * h];
        for row in 0..h {
            for col in 0..w {
                let (share, rgb) = sample(
                    &view.mask,
                    scale,
                    offset,
                    (col, row),
                    palette,
                    &slots,
                    &mut nearest,
                );
                let p = row * w + col;
                if share > 0.0 && share >= f64::from(spec.coverage) {
                    inside[p] = true;
                    color[p] = rgb;
                }
            }
        }
        let mut reference_span = vec![None; w * h];
        if !refs.is_empty() {
            for (p, span) in reference_span.iter_mut().enumerate() {
                let on = |s: usize| refs[fit.cell_on(vi, p, s)];
                if let Some(a) = (0..depth).find(|s| on(*s)) {
                    let b = (a..depth).rev().find(|s| on(*s)).unwrap_or(a);
                    *span = Some((a as u32, b as u32));
                }
            }
        }
        let grid = &mut fit.views[vi];
        grid.inside = inside;
        grid.color = color;
        grid.reference = reference_span;
        grid.scale = scale;
        grid.offset = offset;
    }
    depth_targets(&mut fit, spec);
    fit.build_prior(&refs);
    fit.recompute();
    let energy_before = fit.total();
    let before: Vec<ViewScore> = (0..fit.views.len()).map(|v| fit.score(v)).collect();
    let mut sweeps = 0;
    let mut moves = 0;
    let mut rounded = 0;
    if spec.shape {
        let fill_slot = {
            let mut tally: HashMap<u8, usize> = HashMap::new();
            for slot in cells.values() {
                *tally.entry(*slot).or_default() += 1;
            }
            tally
                .into_iter()
                .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
                .map_or(slots[0], |(slot, _)| slot)
        };
        fit.colour = false;
        fit.recompute();
        let mut rng = Rng::new(spec.seed);
        while sweeps < spec.iterations {
            sweeps += 1;
            let kept = fit.sweep(&mut rng, &slots, fill_slot);
            moves += kept;
            if kept == 0 {
                break;
            }
        }
        rounded = fit.round(spec.iterations.max(1) * 4);
        fit.colour = true;
    }
    if spec.recolor {
        fit.recolour(&slots);
    }
    fit.recompute();
    let energy_after = fit.total();
    let mut writes = Vec::new();
    let (mut added, mut removed, mut recolored) = (0, 0, 0);
    let mut usage: HashMap<u8, usize> = HashMap::new();
    for i in 0..fit.occ.len() {
        if !fit.context[i] && fit.occ[i] != 0 {
            *usage.entry(fit.occ[i]).or_default() += 1;
        }
        if fit.fixed[i] {
            continue;
        }
        let (old, new) = (original[i], fit.occ[i]);
        if old == new {
            continue;
        }
        match (old, new) {
            (0, _) => added += 1,
            (_, 0) => removed += 1,
            _ => recolored += 1,
        }
        let p = fit.point(i);
        writes.push((
            (
                lo[0] + p[0] as i32,
                lo[1] + p[1] as i32,
                lo[2] + p[2] as i32,
            ),
            new,
        ));
    }
    let mut palette_usage: Vec<(u8, usize)> = usage.into_iter().collect();
    palette_usage.sort_unstable();
    let views = fit
        .views
        .iter()
        .enumerate()
        .map(|(v, grid)| FitViewReport {
            side: grid.side,
            grid: [grid.w as u32, grid.h as u32],
            scale: grid.scale,
            offset: grid.offset,
            before: before[v],
            after: fit.score(v),
            depth: grid.depth_cal,
        })
        .collect();
    Ok(Fitted {
        writes,
        added,
        removed,
        recolored,
        sweeps,
        moves,
        rounded,
        views,
        energy: [energy_before, energy_after],
        palette: palette_usage,
        origin: (lo[0], lo[1], lo[2]),
        size: dims.map(|d| d as u32),
    })
}
