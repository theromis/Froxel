//! MCP `optimize_to_views`: iterate a small voxel creature (20 to 60 cells)
//! until its orthographic projections match its pixel-art views.
//!
//! The idea is Voxify3D's (Huang et al., "Voxify3D: Pixel Art Meets
//! Volumetric Rendering", CVPR 2026): pixel-art views at the cell grid, one
//! pixel a column of cells, a shared palette, and an optimisation of
//! occupancy and colour against every view at once. This is an independent
//! CPU solver with no neural network: a bounded greedy search over the
//! shape, then a discrete labelling of the surface colours. It reads the
//! views as `pixelate_views` writes them (rows and legend), or pixelates a
//! sheet's crops at the object's box itself.
//!
//! The views are stretched over the object's box (the ink's bounds onto
//! the box, as `paint_from_views` does), so pixel `(col, row)` of a view is
//! exactly one column of cells. Two terms then alternate:
//!
//! 1. **Shape.** A view pixel costs 1 when its target is inked and no cell
//!    covers it (missing) or its target is empty and a cell does
//!    (excess); with a depth map also `depthWeight * min(|front - depth|,
//!    4) / 4` past `depthTolerance`; and each exposed face costs
//!    `surfaceWeight`. Moves are a whole excess column cleared, the cells
//!    in front of a depth map's surface cleared, one cell added on a
//!    missing ray (touching the body, inside every other view's
//!    silhouette, nearest the neighbouring rays' depth or the depth map),
//!    and a depth map's run filled. A move is kept only when it lowers the
//!    energy of the rays it touches (re-cast exactly), stays within `band`
//!    cells of the starting surface, keeps the part connected, and leaves
//!    no cell thinner than `minFeature` that was not already. Colour is
//!    never a reason to carve.
//! 2. **Colour.** Every surface cell takes the slot that minimises the
//!    weighted disagreement with the views that see it (the first solid
//!    cell of a view's ray), each view weighted by `max(0, n · view) ^
//!    sharpness` so the view facing the cell most squarely wins, plus a
//!    Potts term `smooth` over its surface neighbours, solved by iterated
//!    conditional modes from the per-cell vote, so colour regions stay as
//!    patches and a lone disagreeing cell joins its neighbours. A colour
//!    under `accentShare` of the views' pixels (eyes, pupils, nostrils) is
//!    an accent: a cell a view squarely paints with one keeps it, and no
//!    cell takes one unless a view paints it there, so the smoothing never
//!    grows or erases an eye.
//!
//! Every order is fixed, so the result is deterministic.

use std::collections::{HashMap, VecDeque};

use super::colors::{self, Lab};
use super::depth::{self, Calibration, Depth};
use super::{Ink, Mask, Shades, Side};
use crate::state::Cell;

/// Most cells of the box the solver works in.
pub const MAX_OPTIMIZE_CELLS: i64 = 2_000_000;
/// Most shape iterations.
pub const MAX_OPTIMIZE_ITERATIONS: u32 = 64;
/// Widest band about the starting surface.
pub const MAX_OPTIMIZE_BAND: u32 = 8;
/// Most distinct colours over all views.
pub const MAX_OPTIMIZE_COLORS: usize = 32;
/// Oklab distance under which a cell's colour agrees with a pixel.
pub const AGREE_DISTANCE: f32 = 0.06;
/// Most colour sweeps.
const MAX_COLOR_SWEEPS: u32 = 16;
/// A target pixel inked with no colour.
const PLAIN: u16 = u16::MAX;

#[derive(Clone, Debug, PartialEq)]
pub struct OptimizeView {
    pub side: Side,
    /// The view's pixel art (or any image; its colours must be few).
    pub mask: Mask,
    /// The view's depth map, in the pixels of `source`'s image.
    pub depth: Option<Depth>,
    /// When `mask` is a pixelated grid of a larger image: the ink
    /// rectangle `[x, y, width, height]` of that image (the depth map's
    /// pixels) the grid covers.
    pub source: Option<[u32; 4]>,
    /// The view's colours vote; off uses it for the shape only.
    pub color: bool,
    /// Its vote's weight (a hero view's colours win its conflicts).
    pub weight: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OptimizeSpec {
    pub views: Vec<OptimizeView>,
    /// The ink's bounds map onto the box (true), or the whole mask.
    pub fit_ink: bool,
    /// A missing right view is the left one mirrored, and the other way.
    pub mirror: bool,
    /// Most shape iterations, 0..=64; 0 only recolours.
    pub iterations: u32,
    /// Add and remove cells.
    pub shape: bool,
    /// Recolour the surface.
    pub recolor: bool,
    /// Cells: how far from the starting surface cells may be added
    /// (outside) or removed (inside).
    pub band: u32,
    /// No move leaves a cell thinner than this (its shortest solid run
    /// along x, y, or z) unless it already was; 1 is off.
    pub min_feature: u32,
    /// No removal splits the object into more parts.
    pub connected: bool,
    /// Weight of the Potts term against the views' vote, 0..4.
    pub smooth: f32,
    /// Exponent of a view's facing weight.
    pub sharpness: f32,
    /// The slots colours land in (nearest in Oklab); `None` makes a slot
    /// per view colour (an existing slot of exactly that colour is used).
    pub slots: Option<Vec<u8>>,
    /// Slots whose cells are never removed or recoloured.
    pub keep: Vec<u8>,
    /// The share of the views' coloured pixels under which a colour is an
    /// accent; `None` turns the guard off.
    pub accents: Option<f32>,
    pub depth_weight: f32,
    pub depth_tolerance: f32,
    /// Cost of each exposed cell face.
    pub surface_weight: f32,
    /// Remove a surface cell two views see first and paint differently
    /// when both rays still meet the body behind it and more of them then
    /// agree.
    pub color_carve: bool,
    /// The semantic term: a [`PatchScorer`] (CLIP) compares each view with
    /// the model's, and the views' votes in the patches that agree least
    /// weigh up to `1 + weight` more, for `rounds` rounds of recolouring,
    /// each kept only when the mean similarity rises.
    pub semantic: Option<Semantic>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Semantic {
    pub weight: f32,
    pub rounds: u32,
    /// The least similar patches tried per round.
    pub patches: usize,
}

/// Patches per side of a [`PatchScorer`]'s grid (CLIP ViT-B/32 at 224).
pub const PATCH_GRID: usize = 7;
/// Most semantic rounds.
pub const MAX_SEMANTIC_ROUNDS: u32 = 8;

/// A view as an image: one pixel a cell, `None` empty.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewImage {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<Option<[u8; 3]>>,
}

/// Patch similarity between image pairs: for each pair, the
/// [`PATCH_GRID`] x [`PATCH_GRID`] similarities (row-major, about 0..1) of
/// the images letterboxed onto a square.
pub trait PatchScorer {
    fn similarity(&mut self, pairs: &[(ViewImage, ViewImage)]) -> Result<Vec<Vec<f32>>, String>;
    fn name(&self) -> String;
}

/// What the semantic term did.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticReport {
    pub scorer: String,
    /// Mean patch similarity per view: of the starting model, after the
    /// colour step, and at the end.
    pub start: Vec<f32>,
    pub before: Vec<f32>,
    pub after: Vec<f32>,
    pub rounds: u32,
    /// Rounds whose reweighted recolouring was kept.
    pub kept: u32,
    /// Patch moves tried, and kept.
    pub tried: usize,
    pub patches: usize,
    /// Cells the kept rounds recoloured.
    pub recolored: usize,
}

impl Default for OptimizeSpec {
    fn default() -> Self {
        Self {
            views: Vec::new(),
            fit_ink: true,
            mirror: true,
            iterations: 8,
            shape: true,
            recolor: true,
            band: 3,
            min_feature: 1,
            connected: true,
            smooth: 0.6,
            sharpness: 2.0,
            slots: None,
            keep: Vec::new(),
            accents: Some(0.04),
            depth_weight: 0.5,
            depth_tolerance: 0.5,
            surface_weight: 0.05,
            color_carve: true,
            semantic: None,
        }
    }
}

/// How one view agrees with the model, in pixels of its grid.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Agreement {
    /// Pixels the target inks.
    pub target: usize,
    /// Pixels the model covers.
    pub model: usize,
    /// Pixels both cover.
    pub overlap: usize,
    /// Overlap pixels whose first cell's colour is the target's (within
    /// [`AGREE_DISTANCE`]), or any colour where the target has none.
    pub matched: usize,
    /// Mean Oklab distance over the coloured overlap pixels.
    pub color_error: f32,
    /// Mean cells between the first cell and the depth map's surface.
    pub depth_error: Option<f32>,
}

impl Agreement {
    pub fn iou(&self) -> f32 {
        let union = self.target + self.model - self.overlap;
        if union == 0 {
            1.0
        } else {
            self.overlap as f32 / union as f32
        }
    }

    /// Pixels that agree in shape and colour over the pixels either
    /// covers: the headline number.
    pub fn agreement(&self) -> f32 {
        let union = self.target + self.model - self.overlap;
        if union == 0 {
            1.0
        } else {
            self.matched as f32 / union as f32
        }
    }

    /// Matched over overlap: colour alone.
    pub fn color(&self) -> f32 {
        if self.overlap == 0 {
            1.0
        } else {
            self.matched as f32 / self.overlap as f32
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OptimizeViewReport {
    pub side: Side,
    pub grid: [u32; 2],
    pub mirrored: bool,
    pub before: Agreement,
    pub after: Agreement,
    pub depth: Option<Calibration>,
}

/// One of the views' colours.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewColour {
    /// The image colour, or `None` for a legend slot.
    pub rgb: Option<[u8; 3]>,
    pub slot: u8,
    pub accent: bool,
    /// Pixels of it over all views.
    pub pixels: usize,
}

/// Moves the constraints refused.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Blocked {
    pub band: usize,
    pub keep: usize,
    pub connected: usize,
    pub thin: usize,
    /// Moves that would not lower the energy.
    pub gain: usize,
    /// Missing rays with no cell that touches the body and lies inside
    /// every other view.
    pub unreachable: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Optimized {
    /// Every changed cell with its new slot, 0 removing it.
    pub writes: Vec<(Cell, u8)>,
    pub added: usize,
    pub removed: usize,
    pub recolored: usize,
    /// Shape iterations run.
    pub iterations: u32,
    /// Shape moves kept.
    pub moves: usize,
    /// Cells colour carving removed.
    pub color_carved: usize,
    /// Colour sweeps run.
    pub color_sweeps: u32,
    /// Surface cells the colour step labelled; of them, cells no view sees.
    pub surface: usize,
    pub unseen: usize,
    /// Cells an accent locked.
    pub accent_cells: usize,
    pub blocked: Blocked,
    pub views: Vec<OptimizeViewReport>,
    pub colours: Vec<ViewColour>,
    /// Shape energy before and after.
    pub energy: [f64; 2],
    pub semantic: Option<SemanticReport>,
    pub origin: Cell,
    pub size: [u32; 3],
}

/// The dense box.
struct Grid {
    lo: [i32; 3],
    n: [usize; 3],
    occ: Vec<u8>,
}

impl Grid {
    fn index(&self, c: [usize; 3]) -> usize {
        c[0] + self.n[0] * (c[1] + self.n[1] * c[2])
    }

    fn coords(&self, i: usize) -> [usize; 3] {
        let x = i % self.n[0];
        let y = (i / self.n[0]) % self.n[1];
        let z = i / (self.n[0] * self.n[1]);
        [x, y, z]
    }

    /// The neighbour of `i` one step along `axis` (sign `dir`), if inside.
    fn step(&self, i: usize, axis: usize, dir: i32) -> Option<usize> {
        let mut c = self.coords(i);
        if dir < 0 {
            if c[axis] == 0 {
                return None;
            }
            c[axis] -= 1;
        } else {
            if c[axis] + 1 >= self.n[axis] {
                return None;
            }
            c[axis] += 1;
        }
        Some(self.index(c))
    }

    fn solid(&self, i: usize) -> bool {
        self.occ[i] != 0
    }

    fn neighbours6(&self, i: usize) -> impl Iterator<Item = Option<usize>> + '_ {
        (0..3).flat_map(move |a| [self.step(i, a, -1), self.step(i, a, 1)])
    }

    /// Exposed faces of `i` (the box's outside counts as empty).
    fn exposed(&self, i: usize) -> usize {
        self.neighbours6(i)
            .filter(|n| n.is_none_or(|n| !self.solid(n)))
            .count()
    }

    fn surface(&self, i: usize) -> bool {
        self.solid(i) && self.exposed(i) > 0
    }

    /// The shortest solid run through `i` along x, y, or z, capped.
    fn thickness(&self, i: usize, cap: u32) -> u32 {
        let mut best = cap;
        for axis in 0..3 {
            let mut run = 1u32;
            for dir in [-1, 1] {
                let mut at = i;
                while run < cap {
                    match self.step(at, axis, dir) {
                        Some(n) if self.solid(n) => {
                            run += 1;
                            at = n;
                        }
                        _ => break,
                    }
                }
            }
            best = best.min(run);
        }
        best
    }
}

/// One view over the box.
struct View {
    side: Side,
    mirrored: bool,
    color: bool,
    weight: f32,
    /// Per pixel, how much more its vote weighs (the semantic term).
    vote: Vec<f32>,
    w: usize,
    h: usize,
    steps: usize,
    axes: [(usize, i32); 3],
    /// Target colour index per pixel ([`PLAIN`] uncoloured).
    target: Vec<Option<u16>>,
    /// The depth map's front step per pixel.
    front: Vec<Option<f32>>,
    calibration: Option<Calibration>,
}

impl View {
    /// The box cell of pixel `p` at `step` from the camera side.
    fn cell(&self, grid: &Grid, p: usize, step: usize) -> usize {
        let (col, row) = (p % self.w, p / self.w);
        let [(u, su), (v, sv), (d, sd)] = self.axes;
        let mut c = [0usize; 3];
        c[u] = if su > 0 { col } else { grid.n[u] - 1 - col };
        c[v] = if sv > 0 { row } else { grid.n[v] - 1 - row };
        c[d] = if sd > 0 { grid.n[d] - 1 - step } else { step };
        grid.index(c)
    }

    /// The pixel of box cell `i` in this view.
    fn pixel(&self, grid: &Grid, i: usize) -> usize {
        let c = grid.coords(i);
        let [(u, su), (v, sv), _] = self.axes;
        let col = if su > 0 { c[u] } else { grid.n[u] - 1 - c[u] };
        let row = if sv > 0 { c[v] } else { grid.n[v] - 1 - c[v] };
        row * self.w + col
    }

    /// The first solid step of pixel `p`'s ray.
    fn first(&self, grid: &Grid, p: usize) -> Option<usize> {
        (0..self.steps).find(|s| grid.solid(self.cell(grid, p, *s)))
    }

    fn toward(&self) -> [f64; 3] {
        let (d, sd) = self.axes[2];
        let mut v = [0.0; 3];
        v[d] = f64::from(sd);
        v
    }
}

struct Solver<'a> {
    grid: Grid,
    views: Vec<View>,
    labs: Vec<Lab>,
    slot_of: Vec<u8>,
    palette: [Lab; 256],
    rgb: [[u8; 3]; 256],
    /// Each view colour's RGB.
    colour_rgb: Vec<[u8; 3]>,
    spec: &'a OptimizeSpec,
    /// Distance in cells from the starting solid (empty cells), and from
    /// the starting empty (solid cells).
    out_dist: Vec<u32>,
    in_dist: Vec<u32>,
    keep: [bool; 256],
    blocked: Blocked,
}

/// Distances (6-connected) from every cell of `from` within `cap`; the
/// box's outside is a source when `outside` is set.
fn distances(grid: &Grid, from: &dyn Fn(usize) -> bool, outside: bool, cap: u32) -> Vec<u32> {
    let mut dist = vec![u32::MAX; grid.occ.len()];
    let mut queue = VecDeque::new();
    for i in 0..grid.occ.len() {
        if from(i) {
            dist[i] = 0;
            queue.push_back(i);
        } else if outside && grid.neighbours6(i).any(|n| n.is_none()) {
            dist[i] = 1;
            queue.push_back(i);
        }
    }
    while let Some(i) = queue.pop_front() {
        if dist[i] >= cap {
            continue;
        }
        for n in grid.neighbours6(i).flatten() {
            if dist[n] == u32::MAX {
                dist[n] = dist[i] + 1;
                queue.push_back(n);
            }
        }
    }
    dist
}

impl Solver<'_> {
    /// Whether every view inks the pixel of cell `i`, except `skip`.
    fn inside(&self, i: usize, skip: Option<usize>) -> bool {
        self.views
            .iter()
            .enumerate()
            .all(|(vi, v)| Some(vi) == skip || v.target[v.pixel(&self.grid, i)].is_some())
    }

    /// Shape energy of pixel `p` of view `vi`.
    fn pixel_energy(&self, vi: usize, p: usize) -> f64 {
        let view = &self.views[vi];
        let first = view.first(&self.grid, p);
        match (view.target[p].is_some(), first) {
            (true, None) | (false, Some(_)) => 1.0,
            (false, None) => 0.0,
            (true, Some(s)) => match view.front[p] {
                Some(f) => {
                    let off = ((s as f32 - f).abs() - self.spec.depth_tolerance).max(0.0);
                    f64::from(self.spec.depth_weight * off.min(4.0) / 4.0)
                }
                None => 0.0,
            },
        }
    }

    fn energy(&self) -> f64 {
        let mut e = 0.0;
        for (vi, view) in self.views.iter().enumerate() {
            for p in 0..view.w * view.h {
                e += self.pixel_energy(vi, p);
            }
        }
        let faces: usize = (0..self.grid.occ.len())
            .filter(|i| self.grid.solid(*i))
            .map(|i| self.grid.exposed(i))
            .sum();
        e + f64::from(self.spec.surface_weight) * faces as f64
    }

    /// The rays (view, pixel) the cells `cells` lie on.
    fn rays(&self, cells: &[usize]) -> Vec<(usize, usize)> {
        let mut out: Vec<(usize, usize)> = cells
            .iter()
            .flat_map(|i| {
                self.views
                    .iter()
                    .enumerate()
                    .map(move |(vi, v)| (vi, v.pixel(&self.grid, *i)))
            })
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Exposed faces of `cells` and their neighbours.
    fn local_faces(&self, cells: &[usize]) -> usize {
        let mut around: Vec<usize> = cells.to_vec();
        for i in cells {
            around.extend(self.grid.neighbours6(*i).flatten());
        }
        around.sort_unstable();
        around.dedup();
        around
            .into_iter()
            .filter(|i| self.grid.solid(*i))
            .map(|i| self.grid.exposed(i))
            .sum()
    }

    /// Apply `changes` (cell, new slot) when the energy drops and the
    /// constraints hold; otherwise put them back. Returns whether kept.
    fn try_move(&mut self, changes: &[(usize, u8)], removal: bool) -> bool {
        let cells: Vec<usize> = changes.iter().map(|c| c.0).collect();
        let rays = self.rays(&cells);
        let before: f64 = rays
            .iter()
            .map(|(v, p)| self.pixel_energy(*v, *p))
            .sum::<f64>()
            + f64::from(self.spec.surface_weight) * self.local_faces(&cells) as f64;
        let watch = if removal {
            self.thin_watch(&cells)
        } else {
            (Vec::new(), Vec::new())
        };
        let old: Vec<(usize, u8)> = changes
            .iter()
            .map(|(i, _)| (*i, self.grid.occ[*i]))
            .collect();
        for (i, slot) in changes {
            self.grid.occ[*i] = *slot;
        }
        let undo = |s: &mut Self| {
            for (i, slot) in &old {
                s.grid.occ[*i] = *slot;
            }
        };
        if removal && self.thinned(&watch) {
            undo(self);
            self.blocked.thin += 1;
            return false;
        }
        if removal && self.spec.connected && !self.still_connected(&cells) {
            undo(self);
            self.blocked.connected += 1;
            return false;
        }
        let after: f64 = rays
            .iter()
            .map(|(v, p)| self.pixel_energy(*v, *p))
            .sum::<f64>()
            + f64::from(self.spec.surface_weight) * self.local_faces(&cells) as f64;
        if after < before - 1e-9 {
            true
        } else {
            undo(self);
            self.blocked.gain += 1;
            false
        }
    }

    /// The cells whose thickness removing `cells` may change, and their
    /// thickness now (empty when `minFeature` is off).
    fn thin_watch(&self, cells: &[usize]) -> (Vec<usize>, Vec<u32>) {
        let m = self.spec.min_feature;
        if m <= 1 {
            return (Vec::new(), Vec::new());
        }
        let mut q = Vec::new();
        for &i in cells {
            for axis in 0..3 {
                for dir in [-1, 1] {
                    let mut at = i;
                    for _ in 1..m {
                        match self.grid.step(at, axis, dir) {
                            Some(n) => {
                                q.push(n);
                                at = n;
                            }
                            None => break,
                        }
                    }
                }
            }
        }
        q.sort_unstable();
        q.dedup();
        q.retain(|i| self.grid.solid(*i) && !cells.contains(i));
        let before = q.iter().map(|i| self.grid.thickness(*i, m)).collect();
        (q, before)
    }

    /// Whether a watched cell is now thinner than `minFeature` and than it
    /// was.
    fn thinned(&self, (cells, before): &(Vec<usize>, Vec<u32>)) -> bool {
        let m = self.spec.min_feature;
        cells.iter().zip(before).any(|(i, b)| {
            let t = self.grid.thickness(*i, m);
            t < m && t < *b
        })
    }

    /// Whether pixel `p` of view `vi` shows its target colour (any cell
    /// where the target has no colour or the view votes for shape only).
    fn matched(&self, vi: usize, p: usize) -> bool {
        let view = &self.views[vi];
        let (Some(t), Some(s)) = (view.target[p], view.first(&self.grid, p)) else {
            return false;
        };
        if t == PLAIN || !view.color {
            return true;
        }
        let slot = self.grid.occ[view.cell(&self.grid, p, s)];
        colors::distance(self.palette[slot as usize], self.labs[t as usize]) <= AGREE_DISTANCE
    }

    /// The slot for cell `i` that the most of `rays` (those it is now the
    /// first cell of) show right; ties go to the view facing it most.
    fn best_slot(&self, i: usize, rays: &[(usize, usize)], accent: &[bool]) -> Option<u8> {
        let mine: Vec<(usize, u16, f32)> = rays
            .iter()
            .filter_map(|&(vi, p)| {
                let view = &self.views[vi];
                let t = view.target[p].filter(|t| *t != PLAIN && view.color)?;
                let s = view.first(&self.grid, p)?;
                (view.cell(&self.grid, p, s) == i).then(|| {
                    let n = self.normal(i);
                    let d = view.toward();
                    let facing = (n[0] * d[0] + n[1] * d[1] + n[2] * d[2]).max(0.0) as f32;
                    (vi, t, facing * view.weight * view.vote[p])
                })
            })
            .collect();
        if mine.is_empty() {
            return None;
        }
        let mut best: Option<(usize, f32, u8)> = None;
        for (k, slot) in self.slot_of.iter().enumerate() {
            if accent[k] && !mine.iter().any(|m| m.1 as usize == k) {
                continue;
            }
            let lab = self.palette[*slot as usize];
            let mut hits = 0;
            let mut cost = 0.0;
            for (_, t, w) in &mine {
                let d = colors::distance(lab, self.labs[*t as usize]);
                if d <= AGREE_DISTANCE {
                    hits += 1;
                }
                cost += w * d;
            }
            let better = match best {
                None => true,
                Some((h, c, s)) => {
                    hits > h || (hits == h && (cost < c - 1e-6 || (cost <= c + 1e-6 && *slot < s)))
                }
            };
            if better {
                best = Some((hits, cost, *slot));
            }
        }
        best.map(|b| b.2)
    }

    /// Colour carving: a surface cell that two or more views see first and
    /// that cannot show all their colours goes when every one of those rays
    /// still meets the body behind it, the cells it uncovers take the
    /// colours of the rays that now meet them, and more of those rays then
    /// agree. A boxy hull's edges round off where a front and a top view
    /// paint the same cell differently. Returns the cells removed.
    fn carve_conflicts(&mut self, accent: &[bool], passes: u32) -> usize {
        let mut removed = 0;
        for _ in 0..passes {
            let mut kept = 0;
            let surface: Vec<usize> = (0..self.grid.occ.len())
                .filter(|i| self.grid.surface(*i))
                .collect();
            for a in surface {
                if !self.grid.solid(a) || self.keep[self.grid.occ[a] as usize] {
                    continue;
                }
                let rays: Vec<(usize, usize)> = (0..self.views.len())
                    .filter_map(|vi| {
                        let view = &self.views[vi];
                        if !view.color {
                            return None;
                        }
                        let p = view.pixel(&self.grid, a);
                        let s = view.first(&self.grid, p)?;
                        (view.cell(&self.grid, p, s) == a && view.target[p].is_some())
                            .then_some((vi, p))
                    })
                    .collect();
                if rays.len() < 2 {
                    continue;
                }
                let before = rays.iter().filter(|(v, p)| self.matched(*v, *p)).count();
                if before == rays.len() {
                    continue;
                }
                // An accent a view shows here stays.
                let slot = self.grid.occ[a];
                if rays.iter().any(|(v, p)| {
                    self.matched(*v, *p)
                        && self.views[*v].target[*p]
                            .is_some_and(|t| t != PLAIN && accent[t as usize])
                }) {
                    continue;
                }
                if self.in_dist[a] > self.spec.band {
                    continue;
                }
                let energy: f64 = rays.iter().map(|(v, p)| self.pixel_energy(*v, *p)).sum();
                let watch = self.thin_watch(&[a]);
                self.grid.occ[a] = 0;
                let mut uncovered = Vec::new();
                let mut fine = true;
                for &(vi, p) in &rays {
                    let view = &self.views[vi];
                    match view.first(&self.grid, p) {
                        Some(s) => {
                            let i = view.cell(&self.grid, p, s);
                            // A cell that was inside has no colour of its own yet.
                            let inside = self
                                .grid
                                .neighbours6(i)
                                .flatten()
                                .filter(|n| *n != a)
                                .all(|n| self.grid.solid(n))
                                && self.grid.neighbours6(i).all(|n| n.is_some());
                            if inside && !uncovered.iter().any(|(j, _)| *j == i) {
                                uncovered.push((i, self.grid.occ[i]));
                            }
                        }
                        None => fine = false,
                    }
                }
                let restore = |s: &mut Self, uncovered: &[(usize, u8)]| {
                    for (i, old) in uncovered {
                        s.grid.occ[*i] = *old;
                    }
                    s.grid.occ[a] = slot;
                };
                if !fine
                    || self.thinned(&watch)
                    || (self.spec.connected && !self.still_connected(&[a]))
                {
                    restore(self, &uncovered);
                    continue;
                }
                for (i, _) in &uncovered {
                    if let Some(best) = self.best_slot(*i, &rays, accent) {
                        self.grid.occ[*i] = best;
                    }
                }
                let after = rays.iter().filter(|(v, p)| self.matched(*v, *p)).count();
                let energy_after: f64 = rays.iter().map(|(v, p)| self.pixel_energy(*v, *p)).sum();
                if after > before && energy_after <= energy + 1e-9 {
                    kept += 1;
                } else {
                    restore(self, &uncovered);
                }
            }
            removed += kept;
            if kept == 0 {
                break;
            }
        }
        removed
    }

    /// After removing `cells`, their solid neighbours still reach each
    /// other through the solid.
    fn still_connected(&self, cells: &[usize]) -> bool {
        let mut seeds: Vec<usize> = cells
            .iter()
            .flat_map(|i| self.grid.neighbours6(*i).flatten())
            .filter(|n| self.grid.solid(*n))
            .collect();
        seeds.sort_unstable();
        seeds.dedup();
        if seeds.len() <= 1 {
            return true;
        }
        let mut seen = HashMap::new();
        let mut queue = VecDeque::from([seeds[0]]);
        seen.insert(seeds[0], ());
        let mut left = seeds.len() - 1;
        while let Some(i) = queue.pop_front() {
            for n in self.grid.neighbours6(i).flatten() {
                if self.grid.solid(n) && seen.insert(n, ()).is_none() {
                    if seeds.binary_search(&n).is_ok() {
                        left -= 1;
                        if left == 0 {
                            return true;
                        }
                    }
                    queue.push_back(n);
                }
            }
        }
        false
    }

    fn removable(&mut self, i: usize) -> bool {
        if self.keep[self.grid.occ[i] as usize] {
            self.blocked.keep += 1;
            return false;
        }
        if self.in_dist[i] > self.spec.band {
            self.blocked.band += 1;
            return false;
        }
        true
    }

    fn addable(&self, i: usize, skip: Option<usize>) -> bool {
        !self.grid.solid(i)
            && self.out_dist[i] <= self.spec.band
            && self.inside(i, skip)
            && self
                .grid
                .neighbours6(i)
                .flatten()
                .any(|n| self.grid.solid(n))
    }

    /// The slot a cell added on pixel `p` of view `vi` takes.
    fn added_slot(&self, vi: usize, p: usize, i: usize) -> u8 {
        if let Some(t) = self.views[vi].target[p].filter(|t| *t != PLAIN) {
            return self.slot_of[t as usize];
        }
        let mut counts = [0usize; 256];
        for n in self.grid.neighbours6(i).flatten() {
            counts[self.grid.occ[n] as usize] += 1;
        }
        (1..=255usize)
            .max_by_key(|s| (counts[*s], 255 - s))
            .unwrap_or(1) as u8
    }

    /// One shape pass: removals, then additions. Returns moves kept.
    fn shape_pass(&mut self) -> usize {
        let mut moves = 0;
        for vi in 0..self.views.len() {
            let (w, h, steps) = (self.views[vi].w, self.views[vi].h, self.views[vi].steps);
            for p in 0..w * h {
                let view = &self.views[vi];
                let first = view.first(&self.grid, p);
                let Some(first) = first else { continue };
                // An excess ray: clear its column.
                let cut_to = if view.target[p].is_none() {
                    steps
                } else if let Some(f) = view.front[p] {
                    let f = (f - self.spec.depth_tolerance).ceil().max(0.0) as usize;
                    if first < f {
                        f
                    } else {
                        continue;
                    }
                } else {
                    continue;
                };
                let column: Vec<usize> = (first..cut_to.min(steps))
                    .map(|s| self.views[vi].cell(&self.grid, p, s))
                    .filter(|i| self.grid.solid(*i))
                    .collect();
                if column.is_empty() || !column.clone().into_iter().all(|i| self.removable(i)) {
                    continue;
                }
                let changes: Vec<(usize, u8)> = column.iter().map(|i| (*i, 0)).collect();
                if self.try_move(&changes, true) {
                    moves += 1;
                }
            }
        }
        for vi in 0..self.views.len() {
            let (w, h, steps) = (self.views[vi].w, self.views[vi].h, self.views[vi].steps);
            for p in 0..w * h {
                if self.views[vi].target[p].is_none() {
                    continue;
                }
                let first = self.views[vi].first(&self.grid, p);
                let front = self.views[vi].front[p];
                let changes: Vec<(usize, u8)> = match (first, front) {
                    // A missing ray: one cell where it reaches the body.
                    (None, _) => {
                        let want = match front {
                            Some(f) => f,
                            None => self.neighbour_depth(vi, p).unwrap_or(steps as f32 / 2.0),
                        };
                        let best = (0..steps)
                            .map(|s| (s, self.views[vi].cell(&self.grid, p, s)))
                            .filter(|(_, i)| self.addable(*i, None))
                            .min_by(|a, b| {
                                ((a.0 as f32 - want).abs())
                                    .total_cmp(&(b.0 as f32 - want).abs())
                                    .then(a.0.cmp(&b.0))
                            });
                        match best {
                            Some((_, i)) => vec![(i, self.added_slot(vi, p, i))],
                            None => {
                                self.blocked.unreachable += 1;
                                continue;
                            }
                        }
                    }
                    // A front behind the depth map's surface: fill the run.
                    (Some(first), Some(f)) => {
                        let f = (f + self.spec.depth_tolerance).floor().max(0.0) as usize;
                        if first <= f {
                            continue;
                        }
                        let run: Vec<usize> = (f..first)
                            .map(|s| self.views[vi].cell(&self.grid, p, s))
                            .collect();
                        if !run.iter().all(|i| {
                            !self.grid.solid(*i)
                                && self.out_dist[*i] <= self.spec.band
                                && self.inside(*i, None)
                        }) {
                            self.blocked.band += 1;
                            continue;
                        }
                        let slot = self.grid.occ[self.views[vi].cell(&self.grid, p, first)];
                        run.into_iter().map(|i| (i, slot)).collect()
                    }
                    (Some(_), None) => continue,
                };
                if self.try_move(&changes, false) {
                    moves += 1;
                }
            }
        }
        moves
    }

    /// The median first step of the covered rays around pixel `p`.
    fn neighbour_depth(&self, vi: usize, p: usize) -> Option<f32> {
        let view = &self.views[vi];
        let (col, row) = ((p % view.w) as i64, (p / view.w) as i64);
        let mut found = Vec::new();
        for r in 1..=2i64 {
            for dy in -r..=r {
                for dx in -r..=r {
                    let (x, y) = (col + dx, row + dy);
                    if x < 0 || y < 0 || x >= view.w as i64 || y >= view.h as i64 {
                        continue;
                    }
                    if let Some(s) = view.first(&self.grid, (y as usize) * view.w + x as usize) {
                        found.push(s);
                    }
                }
            }
            if !found.is_empty() {
                break;
            }
        }
        found.sort_unstable();
        found.get(found.len() / 2).map(|s| *s as f32)
    }

    /// Each view's target and the model's view, as image pairs.
    fn images(&self) -> Vec<(ViewImage, ViewImage)> {
        self.views
            .iter()
            .map(|view| {
                let target = view
                    .target
                    .iter()
                    .map(|t| {
                        t.map(|t| {
                            if t == PLAIN {
                                [128, 128, 128]
                            } else {
                                self.colour_rgb[t as usize]
                            }
                        })
                    })
                    .collect();
                let model = (0..view.w * view.h)
                    .map(|p| {
                        view.first(&self.grid, p)
                            .map(|s| self.rgb[self.grid.occ[view.cell(&self.grid, p, s)] as usize])
                    })
                    .collect();
                let image = |pixels| ViewImage {
                    width: view.w,
                    height: view.h,
                    pixels,
                };
                (image(target), image(model))
            })
            .collect()
    }

    /// Mean patch similarity per view, and the patches.
    fn semantic_score(
        &self,
        scorer: &mut dyn PatchScorer,
    ) -> Result<(Vec<f32>, Vec<Vec<f32>>), String> {
        let patches = scorer.similarity(&self.images())?;
        let means = patches
            .iter()
            .map(|p| p.iter().sum::<f32>() / p.len().max(1) as f32)
            .collect();
        Ok((means, patches))
    }

    /// Each view pixel's vote weight from its patch: the least similar
    /// patch of all views weighs `1 + weight`, the most similar 1.
    fn semantic_votes(&mut self, patches: &[Vec<f32>], weight: f32) {
        let all = patches.iter().flatten();
        let lo = all.clone().copied().fold(f32::INFINITY, f32::min);
        let hi = all.copied().fold(f32::NEG_INFINITY, f32::max);
        let span = (hi - lo).max(1e-6);
        for (view, sims) in self.views.iter_mut().zip(patches) {
            // The view letterboxed onto the square, as the scorer sees it.
            let side = view.w.max(view.h) as f32;
            let (ox, oy) = ((side - view.w as f32) / 2.0, (side - view.h as f32) / 2.0);
            for p in 0..view.w * view.h {
                let (x, y) = (
                    (p % view.w) as f32 + 0.5 + ox,
                    (p / view.w) as f32 + 0.5 + oy,
                );
                let px = ((x / side * PATCH_GRID as f32) as usize).min(PATCH_GRID - 1);
                let py = ((y / side * PATCH_GRID as f32) as usize).min(PATCH_GRID - 1);
                let s = sims.get(py * PATCH_GRID + px).copied().unwrap_or(hi);
                view.vote[p] = 1.0 + weight * (hi - s) / span;
            }
        }
    }

    /// The changes that let view `vi` show its own colours over patch `k`
    /// of its letterboxed grid: every first cell of its rays there whose
    /// colour differs takes the target's slot, unless it is a kept slot or
    /// shows an accent another view sees there.
    fn patch_changes(&self, vi: usize, k: usize, accent: &[bool]) -> Vec<(usize, u8)> {
        let view = &self.views[vi];
        if !view.color {
            return Vec::new();
        }
        let side = view.w.max(view.h) as f32;
        let (ox, oy) = ((side - view.w as f32) / 2.0, (side - view.h as f32) / 2.0);
        let mut out: Vec<(usize, u8)> = Vec::new();
        for p in 0..view.w * view.h {
            let (x, y) = (
                (p % view.w) as f32 + 0.5 + ox,
                (p / view.w) as f32 + 0.5 + oy,
            );
            let px = ((x / side * PATCH_GRID as f32) as usize).min(PATCH_GRID - 1);
            let py = ((y / side * PATCH_GRID as f32) as usize).min(PATCH_GRID - 1);
            if py * PATCH_GRID + px != k || self.matched(vi, p) {
                continue;
            }
            let (Some(t), Some(s)) = (view.target[p], view.first(&self.grid, p)) else {
                continue;
            };
            if t == PLAIN {
                continue;
            }
            let i = view.cell(&self.grid, p, s);
            let old = self.grid.occ[i];
            if self.keep[old as usize] {
                continue;
            }
            let shows_accent = self.views.iter().enumerate().any(|(oj, o)| {
                if oj == vi {
                    return false;
                }
                let q = o.pixel(&self.grid, i);
                o.first(&self.grid, q)
                    .is_some_and(|f| o.cell(&self.grid, q, f) == i)
                    && o.target[q].is_some_and(|ot| ot != PLAIN && accent[ot as usize])
                    && self.matched(oj, q)
            });
            if !shows_accent && !out.iter().any(|(j, _)| *j == i) {
                out.push((i, self.slot_of[t as usize]));
            }
        }
        out
    }

    /// Agreement of every view with the current grid.
    fn agreements(&self) -> Vec<Agreement> {
        self.views
            .iter()
            .map(|view| {
                let mut a = Agreement::default();
                let mut err = 0.0f64;
                let mut coloured = 0usize;
                let mut depth = 0.0f64;
                let mut depth_n = 0usize;
                for p in 0..view.w * view.h {
                    let first = view.first(&self.grid, p);
                    let t = view.target[p];
                    a.target += usize::from(t.is_some());
                    a.model += usize::from(first.is_some());
                    let (Some(t), Some(s)) = (t, first) else {
                        continue;
                    };
                    a.overlap += 1;
                    if let Some(f) = view.front[p] {
                        depth += f64::from((s as f32 - f).abs());
                        depth_n += 1;
                    }
                    if t == PLAIN || !view.color {
                        a.matched += 1;
                        continue;
                    }
                    let slot = self.grid.occ[view.cell(&self.grid, p, s)];
                    let d = colors::distance(self.palette[slot as usize], self.labs[t as usize]);
                    err += f64::from(d);
                    coloured += 1;
                    if d <= AGREE_DISTANCE {
                        a.matched += 1;
                    }
                }
                a.color_error = if coloured == 0 {
                    0.0
                } else {
                    (err / coloured as f64) as f32
                };
                a.depth_error = (depth_n > 0).then(|| (depth / depth_n as f64) as f32);
                a
            })
            .collect()
    }

    /// The outward normal at `i`: the mean direction of empty cells within
    /// two cells.
    fn normal(&self, i: usize) -> [f64; 3] {
        let c = self.grid.coords(i);
        let mut sum = [0.0f64; 3];
        for dz in -2i64..=2 {
            for dy in -2i64..=2 {
                for dx in -2i64..=2 {
                    let r2 = dx * dx + dy * dy + dz * dz;
                    if r2 == 0 || r2 > 6 {
                        continue;
                    }
                    let q = [c[0] as i64 + dx, c[1] as i64 + dy, c[2] as i64 + dz];
                    let inside = (0..3).all(|a| q[a] >= 0 && q[a] < self.grid.n[a] as i64);
                    if inside
                        && self.grid.solid(self.grid.index([
                            q[0] as usize,
                            q[1] as usize,
                            q[2] as usize,
                        ]))
                    {
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
            [0.0; 3]
        } else {
            sum.map(|s| s / len)
        }
    }

    /// The colour step: returns (sweeps, surface cells, unseen, accent
    /// cells).
    fn recolor(&mut self, accent: &[bool]) -> (u32, usize, usize, usize) {
        let surface: Vec<usize> = (0..self.grid.occ.len())
            .filter(|i| self.grid.surface(*i))
            .collect();
        let at: HashMap<usize, usize> = surface.iter().enumerate().map(|(k, i)| (*i, k)).collect();
        // Votes: (target colour, weight) per surface cell.
        let mut votes: Vec<Vec<(u16, f32)>> = vec![Vec::new(); surface.len()];
        for view in &self.views {
            if !view.color {
                continue;
            }
            let toward = view.toward();
            for p in 0..view.w * view.h {
                let Some(t) = view.target[p].filter(|t| *t != PLAIN) else {
                    continue;
                };
                let Some(s) = view.first(&self.grid, p) else {
                    continue;
                };
                let i = view.cell(&self.grid, p, s);
                let Some(k) = at.get(&i) else { continue };
                let n = self.normal(i);
                let facing = (n[0] * toward[0] + n[1] * toward[1] + n[2] * toward[2]).max(0.0);
                let w = (facing as f32).powf(self.spec.sharpness).max(0.05)
                    * view.weight
                    * view.vote[p];
                votes[*k].push((t, w));
            }
        }
        let mut candidates: Vec<u8> = self.slot_of.clone();
        candidates.sort_unstable();
        candidates.dedup();
        let accent_slot = |slot: u8| -> bool {
            self.slot_of
                .iter()
                .zip(accent)
                .any(|(s, a)| *s == slot && *a)
                && !self
                    .slot_of
                    .iter()
                    .zip(accent)
                    .any(|(s, a)| *s == slot && !*a)
        };
        let data = |k: usize, slot: u8| -> f32 {
            let total: f32 = votes[k].iter().map(|v| v.1).sum();
            if total <= 0.0 {
                return 0.0;
            }
            votes[k]
                .iter()
                .map(|(t, w)| {
                    let d = colors::distance(self.palette[slot as usize], self.labs[*t as usize]);
                    w * (d / 0.25).min(1.0)
                })
                .sum::<f32>()
                / total
        };
        let mut labels: Vec<u8> = surface.iter().map(|i| self.grid.occ[*i]).collect();
        let mut locked = vec![false; surface.len()];
        let mut options: Vec<Vec<u8>> = vec![Vec::new(); surface.len()];
        let mut accent_cells = 0;
        let mut unseen = 0;
        for k in 0..surface.len() {
            if self.keep[labels[k] as usize] {
                locked[k] = true;
                continue;
            }
            if votes[k].is_empty() {
                unseen += 1;
                continue;
            }
            let voted: Vec<u8> = votes[k]
                .iter()
                .map(|(t, _)| self.slot_of[*t as usize])
                .collect();
            options[k] = candidates
                .iter()
                .copied()
                .filter(|s| !accent_slot(*s) || voted.contains(s))
                .collect();
            let best = options[k]
                .iter()
                .copied()
                .min_by(|a, b| data(k, *a).total_cmp(&data(k, *b)).then(a.cmp(b)))
                .unwrap_or(labels[k]);
            labels[k] = best;
            if self.spec.accents.is_some() && accent_slot(best) {
                let strong = votes[k]
                    .iter()
                    .any(|(t, w)| self.slot_of[*t as usize] == best && *w >= 0.35);
                if strong {
                    locked[k] = true;
                    accent_cells += 1;
                }
            }
        }
        // Surface neighbours (26) with weights: faces 1, edges 0.5,
        // corners 0.25.
        let neighbours: Vec<Vec<(usize, f32)>> = surface
            .iter()
            .map(|i| {
                let c = self.grid.coords(*i);
                let mut out = Vec::new();
                for dz in -1i64..=1 {
                    for dy in -1i64..=1 {
                        for dx in -1i64..=1 {
                            let order = dx.abs() + dy.abs() + dz.abs();
                            if order == 0 {
                                continue;
                            }
                            let q = [c[0] as i64 + dx, c[1] as i64 + dy, c[2] as i64 + dz];
                            if (0..3).any(|a| q[a] < 0 || q[a] >= self.grid.n[a] as i64) {
                                continue;
                            }
                            let j = self
                                .grid
                                .index([q[0] as usize, q[1] as usize, q[2] as usize]);
                            if let Some(k) = at.get(&j) {
                                out.push((*k, [1.0, 0.5, 0.25][order as usize - 1]));
                            }
                        }
                    }
                }
                out
            })
            .collect();
        let smooth = self.spec.smooth;
        let mut sweeps = 0;
        for _ in 0..MAX_COLOR_SWEEPS {
            sweeps += 1;
            let mut changed = 0;
            for k in 0..surface.len() {
                if locked[k] {
                    continue;
                }
                let nb = &neighbours[k];
                let total: f32 = nb.iter().map(|(_, w)| w).sum();
                let seen = !votes[k].is_empty();
                let mut opts: Vec<u8> = if seen {
                    options[k].clone()
                } else {
                    let mut o: Vec<u8> = nb
                        .iter()
                        .map(|(j, _)| labels[*j])
                        .filter(|s| !accent_slot(*s))
                        .collect();
                    o.push(labels[k]);
                    o
                };
                opts.sort_unstable();
                opts.dedup();
                let cost = |slot: u8| -> f32 {
                    let d = if seen { data(k, slot) } else { 0.0 };
                    let disagree: f32 = nb
                        .iter()
                        .filter(|(j, _)| labels[*j] != slot)
                        .map(|(_, w)| w)
                        .sum();
                    d + smooth * if total > 0.0 { disagree / total } else { 0.0 }
                };
                let current = cost(labels[k]);
                let best = opts
                    .iter()
                    .copied()
                    .map(|s| (cost(s), s))
                    .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                if let Some((c, s)) = best {
                    if c < current - 1e-6 && s != labels[k] {
                        labels[k] = s;
                        changed += 1;
                    }
                }
            }
            if changed == 0 {
                break;
            }
        }
        for (k, i) in surface.iter().enumerate() {
            self.grid.occ[*i] = labels[k];
        }
        (sweeps, surface.len(), unseen, accent_cells)
    }
}

/// The views' colours and each view's targets at the box.
fn targets(
    spec: &OptimizeSpec,
    n: [usize; 3],
    palette: &[[u8; 4]; 256],
) -> Result<(Vec<View>, Vec<(Ink, usize)>), String> {
    type Image = (
        Side,
        Mask,
        Option<Depth>,
        Option<[u32; 4]>,
        (bool, f32),
        bool,
    );
    let mut images: Vec<Image> = spec
        .views
        .iter()
        .map(|v| {
            (
                v.side,
                v.mask.clone(),
                v.depth.clone(),
                v.source,
                (v.color, v.weight),
                false,
            )
        })
        .collect();
    if spec.mirror {
        for (have, want) in [(Side::Left, Side::Right), (Side::Right, Side::Left)] {
            if images.iter().any(|v| v.0 == want) {
                continue;
            }
            if let Some(v) = images.iter().find(|v| v.0 == have && !v.5) {
                let mask = v.1.flipped();
                let depth = v.2.clone().map(|mut d| {
                    d.map = d.map.flipped();
                    d
                });
                let source = v.3.map(|[x, y, w, h]| {
                    let width = depth.as_ref().map_or(x + w, |d| d.map.width);
                    [width.saturating_sub(x + w), y, w, h]
                });
                let color = v.4;
                images.push((want, mask, depth, source, color, true));
            }
        }
    }
    let mut colours: Vec<(Ink, usize)> = Vec::new();
    let mut views = Vec::new();
    // A view listed earlier wins a tie (a cell on an edge two views see
    // equally squarely): the first listed is the hero view.
    let count = images.len();
    for (k, image) in images.iter_mut().enumerate() {
        image.4 .1 *= 1.0 + 0.01 * (count - k) as f32;
    }
    let mut depths = Vec::new();
    for (side, mask, depth, source, (color, weight), mirrored) in images {
        let axes = side.axes();
        let (w, h, steps) = (n[axes[0].0], n[axes[1].0], n[axes[2].0]);
        let [x0, y0, x1, y1] = if spec.fit_ink {
            mask.bounds()
                .ok_or_else(|| format!("view {} has no ink", side.name()))?
        } else {
            [0, 0, mask.width - 1, mask.height - 1]
        };
        let (rw, rh) = (f64::from(x1 - x0 + 1), f64::from(y1 - y0 + 1));
        let mut target = vec![None; w * h];
        for row in 0..h {
            for col in 0..w {
                let mx = f64::from(x0) + (col as f64 + 0.5) / w as f64 * rw;
                let my = f64::from(y0) + (row as f64 + 0.5) / h as f64 * rh;
                let (mx, my) = (
                    (mx.floor() as u32).min(mask.width - 1),
                    (my.floor() as u32).min(mask.height - 1),
                );
                let Some(ink) = mask.get(mx, my) else {
                    continue;
                };
                if ink == Ink::Plain {
                    target[row * w + col] = Some(PLAIN);
                    continue;
                }
                let k = match colours.iter().position(|(c, _)| *c == ink) {
                    Some(k) => k,
                    None => {
                        colours.push((ink, 0));
                        colours.len() - 1
                    }
                };
                colours[k].1 += 1;
                target[row * w + col] = Some(k as u16);
            }
        }
        // Each ray's footprint on the depth map, in shares of its image.
        let rects: Vec<Option<[f64; 4]>> = match &depth {
            None => Vec::new(),
            Some(d) => {
                let (mw, mh) = (f64::from(d.map.width), f64::from(d.map.height));
                let [sx, sy, sw, sh] = match source {
                    Some([x, y, w, h]) => [x, y, w, h].map(f64::from),
                    None => {
                        // The mask and the map share the crop.
                        let kx = mw / f64::from(mask.width);
                        let ky = mh / f64::from(mask.height);
                        [f64::from(x0) * kx, f64::from(y0) * ky, rw * kx, rh * ky]
                    }
                };
                (0..w * h)
                    .map(|p| {
                        target[p]?;
                        let (col, row) = ((p % w) as f64, (p / w) as f64);
                        Some([
                            (sx + col / w as f64 * sw) / mw,
                            (sx + (col + 1.0) / w as f64 * sw) / mw,
                            (sy + row / h as f64 * sh) / mh,
                            (sy + (row + 1.0) / h as f64 * sh) / mh,
                        ])
                    })
                    .collect()
            }
        };
        depths.push((depth, rects));
        views.push(View {
            side,
            mirrored,
            color,
            weight,
            vote: vec![1.0; w * h],
            w,
            h,
            steps,
            axes,
            target,
            front: vec![None; w * h],
            calibration: None,
        });
    }
    if colours.len() > MAX_OPTIMIZE_COLORS {
        return Err(format!(
            "the views hold {} colours (at most {MAX_OPTIMIZE_COLORS}): pass pixel art, the rows of pixelate_views, or downscale",
            colours.len()
        ));
    }
    let _ = palette;
    // Depth maps, calibrated against the other views' silhouette hull.
    let sides: Vec<Side> = views.iter().map(|v| v.side).collect();
    let grid = Grid {
        lo: [0; 3],
        n,
        occ: Vec::new(),
    };
    for vi in 0..views.len() {
        let (Some(map), rects) = &depths[vi] else {
            continue;
        };
        let v = &views[vi];
        let inside_others = |i: usize| {
            views
                .iter()
                .enumerate()
                .all(|(oj, o)| oj == vi || o.target[o.pixel(&grid, i)].is_some())
        };
        let mut hull = vec![None; v.w * v.h];
        let mut hull_back = vec![None; v.w * v.h];
        let (mut lo, mut hi) = (usize::MAX, 0usize);
        for p in 0..v.w * v.h {
            if v.target[p].is_none() {
                continue;
            }
            let on = |s: &usize| inside_others(v.cell(&grid, p, *s));
            hull[p] = (0..v.steps).find(on).map(|s| s as f32);
            hull_back[p] = (0..v.steps).rev().find(on).map(|s| s as f32);
            if let (Some(a), Some(b)) = (hull[p], hull_back[p]) {
                lo = lo.min(a as usize);
                hi = hi.max(b as usize);
            }
        }
        let [(u, _), (vv, _), (d, _)] = v.axes;
        let rays = depth::Rays {
            w: v.w,
            h: v.h,
            steps: v.steps,
            lines: [
                depth::measured(&sides, vv, d),
                depth::measured(&sides, u, d),
            ],
            middle: if lo == usize::MAX {
                v.steps as f32 / 2.0
            } else {
                (lo + hi + 1) as f32 / 2.0
            },
            hull: &hull,
            hull_back: &hull_back,
            rects,
        };
        let (surface, cal) = depth::surfaces(map, &rays);
        let last = v.steps as f32 - 1.0;
        let front: Vec<Option<f32>> = surface
            .iter()
            .map(|s| s.map(|s| (s - 0.5).ceil().clamp(0.0, last)))
            .collect();
        views[vi].front = front;
        views[vi].calibration = Some(cal);
    }
    Ok((views, colours))
}

/// Optimise `cells` toward `spec.views`. New slots for the views' colours
/// go into `shades` (its `colors` are the document's palette); the caller
/// writes `shades.created` in the same undo step.
pub fn optimize_to_views(
    cells: &HashMap<Cell, u8>,
    spec: &OptimizeSpec,
    shades: &mut Shades,
) -> Result<Optimized, String> {
    optimize_with(cells, spec, shades, None)
}

/// [`optimize_to_views`] with the semantic term's scorer, which
/// `spec.semantic` needs.
pub fn optimize_with(
    cells: &HashMap<Cell, u8>,
    spec: &OptimizeSpec,
    shades: &mut Shades,
    mut scorer: Option<&mut dyn PatchScorer>,
) -> Result<Optimized, String> {
    if let Some(sem) = spec.semantic {
        if !(0.0..=16.0).contains(&sem.weight) {
            return Err("semantic weight must be 0..16".into());
        }
        if sem.patches > 64 {
            return Err("semantic patches must be 0..=64".into());
        }
        if sem.rounds > MAX_SEMANTIC_ROUNDS {
            return Err(format!("semantic rounds must be 0..={MAX_SEMANTIC_ROUNDS}"));
        }
        if scorer.is_none() {
            return Err("semantic needs a scorer".into());
        }
    }
    if spec.views.is_empty() || spec.views.len() > 6 {
        return Err("views must hold 1 to 6 views".into());
    }
    for (i, view) in spec.views.iter().enumerate() {
        if spec.views[..i].iter().any(|v| v.side == view.side) {
            return Err(format!("view {} is given twice", view.side.name()));
        }
        if view.mask.count() == 0 {
            return Err(format!("view {} has no ink", view.side.name()));
        }
    }
    if spec.iterations > MAX_OPTIMIZE_ITERATIONS {
        return Err(format!("iterations must be 0..={MAX_OPTIMIZE_ITERATIONS}"));
    }
    if spec.band > MAX_OPTIMIZE_BAND {
        return Err(format!("band must be 0..={MAX_OPTIMIZE_BAND} cells"));
    }
    if spec.min_feature == 0 || spec.min_feature > super::MAX_MIN_FEATURE {
        return Err(format!(
            "minFeature must be 1..={} cells",
            super::MAX_MIN_FEATURE
        ));
    }
    if !(0.0..=4.0).contains(&spec.smooth) {
        return Err("smooth must be 0..4".into());
    }
    if !(0.0..=16.0).contains(&spec.sharpness) {
        return Err("sharpness must be 0..16".into());
    }
    if let Some(share) = spec.accents {
        if !(0.0..=0.5).contains(&share) {
            return Err("accentShare must be 0..0.5".into());
        }
    }
    if !(0.0..=16.0).contains(&spec.depth_weight) || !(0.0..=16.0).contains(&spec.depth_tolerance) {
        return Err("depthWeight and depthTolerance must be 0..16".into());
    }
    if !(0.0..=4.0).contains(&spec.surface_weight) {
        return Err("surfaceWeight must be 0..4".into());
    }
    if let Some(slots) = &spec.slots {
        if slots.is_empty() || slots.contains(&0) {
            return Err("slots must be palette indices 1..255".into());
        }
    }
    if cells.is_empty() {
        return Err("the object has no cells to optimise".into());
    }
    let mut lo = [i32::MAX; 3];
    let mut hi = [i32::MIN; 3];
    for &(x, y, z) in cells.keys() {
        for (a, c) in [x, y, z].into_iter().enumerate() {
            lo[a] = lo[a].min(c);
            hi[a] = hi[a].max(c);
        }
    }
    let n: [usize; 3] = std::array::from_fn(|a| (hi[a] - lo[a] + 1) as usize);
    let volume: i64 = n.iter().map(|v| *v as i64).product();
    if volume > MAX_OPTIMIZE_CELLS {
        return Err(format!(
            "the object's box holds {volume} cells (at most {MAX_OPTIMIZE_CELLS}): optimize_to_views is for small creatures; use fit_to_views"
        ));
    }
    let mut grid = Grid {
        lo,
        n,
        occ: vec![0; volume as usize],
    };
    for (&(x, y, z), slot) in cells {
        let i = grid.index([
            (x - lo[0]) as usize,
            (y - lo[1]) as usize,
            (z - lo[2]) as usize,
        ]);
        grid.occ[i] = *slot;
    }
    let (views, colours) = targets(spec, n, &shades.colors)?;
    // Each view colour's slot.
    let mut counts = [0usize; 256];
    for slot in cells.values() {
        counts[*slot as usize] += 1;
    }
    let from = (1..=255usize)
        .max_by_key(|s| (counts[*s], 255 - s))
        .unwrap_or(1) as u8;
    let lab_of = |rgba: [u8; 4]| colors::oklab([rgba[0], rgba[1], rgba[2]]);
    let mut slot_of = Vec::with_capacity(colours.len());
    for (ink, _) in &colours {
        let slot = match ink {
            Ink::Slot(s) => *s,
            Ink::Rgb(rgb) => match &spec.slots {
                Some(slots) => {
                    let lab = colors::oklab(*rgb);
                    let centres: Vec<Lab> = slots
                        .iter()
                        .map(|s| lab_of(shades.colors[*s as usize]))
                        .collect();
                    slots[colors::nearest(lab, &centres)]
                }
                None => shades.colour(*rgb, from)?,
            },
            Ink::Plain => unreachable!("plain ink is not a colour"),
        };
        slot_of.push(slot);
    }
    let labs: Vec<Lab> = colours
        .iter()
        .map(|(ink, _)| match ink {
            Ink::Rgb(rgb) => colors::oklab(*rgb),
            Ink::Slot(s) => lab_of(shades.colors[*s as usize]),
            Ink::Plain => [0.0; 3],
        })
        .collect();
    let palette: [Lab; 256] = std::array::from_fn(|s| lab_of(shades.colors[s]));
    let total: usize = colours.iter().map(|c| c.1).sum();
    let accent: Vec<bool> = colours
        .iter()
        .map(|(_, px)| {
            spec.accents
                .is_some_and(|share| (*px as f32) < share * total as f32)
        })
        .collect();
    let mut keep = [false; 256];
    for s in &spec.keep {
        keep[*s as usize] = true;
    }
    let start: Vec<u8> = grid.occ.clone();
    let cap = spec.band + 1;
    let out_dist = distances(&grid, &|i| start[i] != 0, false, cap);
    let in_dist = distances(&grid, &|i| start[i] == 0, true, cap);
    let mut solver = Solver {
        grid,
        views,
        labs,
        slot_of,
        palette,
        rgb: std::array::from_fn(|s| {
            let c = shades.colors[s];
            [c[0], c[1], c[2]]
        }),
        colour_rgb: colours
            .iter()
            .map(|(ink, _)| match ink {
                Ink::Rgb(rgb) => *rgb,
                Ink::Slot(s) => {
                    let c = shades.colors[*s as usize];
                    [c[0], c[1], c[2]]
                }
                Ink::Plain => [128, 128, 128],
            })
            .collect(),
        spec,
        out_dist,
        in_dist,
        keep,
        blocked: Blocked::default(),
    };
    let before = solver.agreements();
    let energy_before = solver.energy();
    let semantic_start = match (spec.semantic, scorer.as_deref_mut()) {
        (Some(_), Some(scorer)) => Some(solver.semantic_score(scorer)?.0),
        _ => None,
    };
    let mut iterations = 0;
    let mut moves = 0;
    if spec.shape {
        for _ in 0..spec.iterations {
            iterations += 1;
            let kept = solver.shape_pass();
            moves += kept;
            if kept == 0 {
                break;
            }
        }
    }
    let energy_after = solver.energy();
    let (mut color_sweeps, mut surface, mut unseen, mut accent_cells) = if spec.recolor {
        solver.recolor(&accent)
    } else {
        (0, 0, 0, 0)
    };
    let mut color_carved = 0;
    if spec.recolor && spec.shape && spec.color_carve {
        color_carved = solver.carve_conflicts(&accent, spec.iterations.max(1));
        if color_carved > 0 {
            let again = solver.recolor(&accent);
            color_sweeps += again.0;
            (surface, unseen, accent_cells) = (again.1, again.2, again.3);
        }
    }
    let mut semantic = None;
    if let (Some(sem), Some(scorer), Some(start)) =
        (spec.semantic, scorer.as_deref_mut(), semantic_start)
    {
        let (mut means, mut patches) = solver.semantic_score(scorer)?;
        let mut report = SemanticReport {
            scorer: scorer.name(),
            start,
            before: means.clone(),
            after: Vec::new(),
            rounds: 0,
            kept: 0,
            tried: 0,
            patches: 0,
            recolored: 0,
        };
        if spec.recolor {
            for _ in 0..sem.rounds {
                report.rounds += 1;
                let saved = solver.grid.occ.clone();
                solver.semantic_votes(&patches, sem.weight);
                let again = solver.recolor(&accent);
                color_sweeps += again.0;
                let (next, next_patches) = solver.semantic_score(scorer)?;
                let mean = |m: &[f32]| m.iter().sum::<f32>() / m.len().max(1) as f32;
                let mut improved = false;
                if mean(&next) > mean(&means) + 1e-5 {
                    report.kept += 1;
                    (surface, unseen, accent_cells) = (again.1, again.2, again.3);
                    means = next;
                    patches = next_patches;
                    improved = true;
                } else {
                    solver.grid.occ = saved.clone();
                }
                // The patches that agree least, worst first: each view in
                // turn shows its own colours there, kept when the views
                // read more alike.
                let mut worst: Vec<(f32, usize, usize)> = patches
                    .iter()
                    .enumerate()
                    .flat_map(|(vi, p)| p.iter().enumerate().map(move |(k, s)| (*s, vi, k)))
                    .collect();
                worst.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
                for &(_, vi, k) in worst.iter().take(sem.patches) {
                    let changes = solver.patch_changes(vi, k, &accent);
                    if changes.is_empty() {
                        continue;
                    }
                    let old: Vec<(usize, u8)> = changes
                        .iter()
                        .map(|(i, _)| (*i, solver.grid.occ[*i]))
                        .collect();
                    for (i, slot) in &changes {
                        solver.grid.occ[*i] = *slot;
                    }
                    report.tried += 1;
                    let (next, next_patches) = solver.semantic_score(scorer)?;
                    if mean(&next) > mean(&means) + 1e-5 {
                        report.patches += 1;
                        means = next;
                        patches = next_patches;
                        improved = true;
                    } else {
                        for (i, slot) in old {
                            solver.grid.occ[i] = slot;
                        }
                    }
                }
                report.recolored += saved
                    .iter()
                    .zip(&solver.grid.occ)
                    .filter(|(a, b)| a != b)
                    .count();
                if !improved {
                    break;
                }
            }
        }
        report.after = means;
        semantic = Some(report);
    }
    let after = solver.agreements();
    let mut writes = Vec::new();
    let (mut added, mut removed, mut recolored) = (0, 0, 0);
    for (i, (old, new)) in start.iter().zip(&solver.grid.occ).enumerate() {
        if old == new {
            continue;
        }
        match (*old, *new) {
            (0, _) => added += 1,
            (_, 0) => removed += 1,
            _ => recolored += 1,
        }
        let c = solver.grid.coords(i);
        let lo = solver.grid.lo;
        writes.push((
            (
                lo[0] + c[0] as i32,
                lo[1] + c[1] as i32,
                lo[2] + c[2] as i32,
            ),
            *new,
        ));
    }
    let reports = solver
        .views
        .iter()
        .zip(before.iter().zip(&after))
        .map(|(v, (b, a))| OptimizeViewReport {
            side: v.side,
            grid: [v.w as u32, v.h as u32],
            mirrored: v.mirrored,
            before: *b,
            after: *a,
            depth: v.calibration,
        })
        .collect();
    let colours = colours
        .iter()
        .zip(&solver.slot_of)
        .zip(&accent)
        .map(|(((ink, px), slot), accent)| ViewColour {
            rgb: match ink {
                Ink::Rgb(rgb) => Some(*rgb),
                _ => None,
            },
            slot: *slot,
            accent: *accent,
            pixels: *px,
        })
        .collect();
    Ok(Optimized {
        writes,
        added,
        removed,
        recolored,
        iterations,
        moves,
        color_carved,
        color_sweeps,
        surface,
        unseen,
        accent_cells,
        blocked: solver.blocked,
        views: reports,
        colours,
        energy: [energy_before, energy_after],
        semantic,
        origin: (lo[0], lo[1], lo[2]),
        size: n.map(|v| v as u32),
    })
}

#[cfg(test)]
#[path = "../../tests/optimize.rs"]
mod tests;
