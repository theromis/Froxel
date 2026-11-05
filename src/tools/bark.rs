//! Bark behind MCP `bark`: long plates of varied width along a direction
//! (an axis, or the curve a tube or sweep follows), separated by fissures
//! cut into the wood with a dirt slot at their bottom.
//!
//! The surface is unrolled around the direction: `around` is the angle
//! about the axis or curve, `along` the distance down it. Columns of random
//! width tile the circumference and wander a little; each column is split
//! into plates of random length whose ends sit at other heights than the
//! next column's, so the plates are offset as on a real trunk. The
//! fissures between columns are cut 1 to 2 cells deep (`depth`), the ends
//! of plates one, and now and then a crack runs part way across a plate.
//! Each plate takes one of the plate slots, so the tone changes plate by
//! plate rather than in stripes. End grain (faces looking along the
//! direction, such as the cut end of a log) and the inside of a hollow are
//! left alone; a cell next to a hollow is never cut, so a thin wall keeps
//! its inner layer.
//!
//! A flat or gently curved plate has no way round: the cells of its faces
//! look away from the middle, not out of an axis, and angles about that axis
//! bunch up over its width. So when the section is at least three times as
//! wide as it is deep and most of its surface fails the round test, the
//! columns run across the width instead (`Flat`), from rim to rim without a
//! seam. A plate too thin to cut from both faces is cut from one, and never
//! through: its back layer stays.

use std::collections::{HashMap, HashSet};
use std::f64::consts::TAU;

use super::debris::Rng;
use super::sculpt::{path_runs, PathRun, MAX_TUBE_POINTS};
use super::surface::{
    allowed, check_slot, check_slots, check_unit, depths_from, noise3, step, surface_normal,
    Exclude, Region, FACES,
};
use crate::state::Cell;

/// The way the grain runs.
#[derive(Clone, Debug, PartialEq)]
pub enum BarkDirection {
    /// A straight axis through the middle of the solid cells of the region.
    Axis([f64; 3]),
    /// The local direction of a polyline or a Catmull-Rom curve through the
    /// points, as `fill_tube` and `sweep` take them.
    Path { points: Vec<[f64; 3]>, curve: bool },
}

#[derive(Clone, Debug, PartialEq)]
pub struct BarkSpec {
    pub region: Region,
    pub direction: BarkDirection,
    /// Plate slots; each plate takes one.
    pub palettes: Vec<u8>,
    /// Slot of the cell at the bottom of a fissure. `None` keeps its colour.
    pub dirt: Option<u8>,
    /// Plate width range in cells around the wood or across a plate, 1..=64.
    pub width: (f64, f64),
    /// Plate length range in cells along the grain, 2..=256.
    pub length: (f64, f64),
    /// Fissure depth range in cells, 1..=4.
    pub depth: (u32, u32),
    /// Share of plates with a crack part way across, 0..=1.
    pub cracks: f64,
    /// Act only on cells of these slots. Empty acts on every slot.
    pub only: Vec<u8>,
    /// Boxes left alone.
    pub exclude: Exclude,
    pub seed: u64,
}

#[derive(Debug, PartialEq)]
pub struct Barked {
    pub writes: Vec<(i32, i32, i32, i32)>,
    /// Plates that show on the surface.
    pub plates: usize,
    pub removed: usize,
    pub dirtied: usize,
    pub painted: usize,
    pub cracks: usize,
    /// Why little or nothing could be cut, when so.
    pub warning: Option<String>,
}

impl Barked {
    fn nothing(why: &str) -> Barked {
        Barked {
            writes: Vec::new(),
            plates: 0,
            removed: 0,
            dirtied: 0,
            painted: 0,
            cracks: 0,
            warning: Some(why.into()),
        }
    }
}

/// Where a cell sits on the unrolled wood.
#[derive(Clone, Copy, Debug)]
struct Place {
    along: f64,
    /// Radians about the direction.
    around: f64,
    /// Distance from the axis or curve.
    radius: f64,
    tangent: [f64; 3],
    /// Away from the axis or curve, unit length (zero on it).
    out: [f64; 3],
    /// The offset from the axis or curve in its frame, and that frame.
    section: [f64; 2],
    normal: [f64; 3],
    binormal: [f64; 3],
}

fn place(runs: &[PathRun], c: [f64; 3]) -> Place {
    let mut best = (
        f64::INFINITY,
        Place {
            along: 0.0,
            around: 0.0,
            radius: 0.0,
            tangent: [0.0, 0.0, 1.0],
            out: [0.0; 3],
            section: [0.0; 2],
            normal: [1.0, 0.0, 0.0],
            binormal: [0.0, 1.0, 0.0],
        },
    );
    for run in runs {
        let d = sub(run.b, run.a);
        let len2 = dot(d, d);
        let len = len2.sqrt();
        let t = if len2 > 0.0 {
            (dot(sub(c, run.a), d) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let q = [
            run.a[0] + d[0] * t,
            run.a[1] + d[1] * t,
            run.a[2] + d[2] * t,
        ];
        let off = sub(c, q);
        let dist2 = dot(off, off);
        if dist2 >= best.0 {
            continue;
        }
        let tangent = if len > 1.0e-9 {
            [d[0] / len, d[1] / len, d[2] / len]
        } else {
            best.1.tangent
        };
        let along_off = dot(off, tangent);
        let perp = [
            off[0] - tangent[0] * along_off,
            off[1] - tangent[1] * along_off,
            off[2] - tangent[2] * along_off,
        ];
        best = (
            dist2,
            Place {
                along: run.along + len * t + along_off,
                around: dot(perp, run.binormal).atan2(dot(perp, run.normal)),
                radius: dot(perp, perp).sqrt(),
                tangent,
                out: {
                    let r = dot(perp, perp).sqrt();
                    if r > 1.0e-9 {
                        [perp[0] / r, perp[1] / r, perp[2] / r]
                    } else {
                        [0.0; 3]
                    }
                },
                section: [dot(perp, run.normal), dot(perp, run.binormal)],
                normal: run.normal,
                binormal: run.binormal,
            },
        );
    }
    best.1
}

/// A section this many times as wide as it is deep can be a plate.
const FLAT_RATIO: f64 = 3.0;

/// A flat or gently curved plate, in the frame of the direction.
struct Flat {
    /// Unit directions of the section's width and depth.
    across: [f64; 2],
    deep: [f64; 2],
    /// Where the width starts, and the width in cells.
    from: f64,
    width: f64,
    /// The usual count of solid cells through the plate.
    thickness: u32,
    /// When too thin to cut from both faces: the side of `deep` to cut.
    front: Option<f64>,
}

impl Flat {
    /// A plate's layout if the surface cells of `surface` make one. `round`
    /// of them pass the round test.
    fn detect(
        cells: &HashMap<Cell, u8>,
        surface: &[(Cell, Place)],
        round: usize,
        max_depth: u32,
    ) -> Option<Flat> {
        if surface.len() < 4 || round * 4 >= surface.len() * 3 {
            return None;
        }
        let n = surface.len() as f64;
        let mean = surface.iter().fold([0.0; 2], |m, (_, p)| {
            [m[0] + p.section[0] / n, m[1] + p.section[1] / n]
        });
        let (mut aa, mut bb, mut ab) = (0.0, 0.0, 0.0);
        for (_, p) in surface {
            let (a, b) = (p.section[0] - mean[0], p.section[1] - mean[1]);
            aa += a * a;
            bb += b * b;
            ab += a * b;
        }
        let angle = 0.5 * (2.0 * ab).atan2(aa - bb);
        let mut across = [angle.cos(), angle.sin()];
        let first = &surface[0].1;
        // A notch or a rail must not tilt the boards off the grid: snap
        // to a face of the cells when one is that close.
        for f in FACES {
            let f = [f64::from(f.0), f64::from(f.1), f64::from(f.2)];
            let v = [dot(f, first.normal), dot(f, first.binormal)];
            let cos = v[0] * across[0] + v[1] * across[1];
            if cos > 0.99 * (v[0] * v[0] + v[1] * v[1]).sqrt() {
                let len = (v[0] * v[0] + v[1] * v[1]).sqrt();
                across = [v[0] / len, v[1] / len];
                break;
            }
        }
        let deep = [-across[1], across[0]];
        let project = |to: [f64; 2]| {
            let mut list: Vec<f64> = surface
                .iter()
                .map(|(_, p)| p.section[0] * to[0] + p.section[1] * to[1])
                .collect();
            list.sort_by(f64::total_cmp);
            list
        };
        // Extents without the last few cells, so a branch or rail does not
        // count as the wood's shape.
        let extent = |list: &[f64]| {
            let trim = (list.len() - 1) / 50;
            list[list.len() - 1 - trim] - list[trim] + 1.0
        };
        let wide = project(across);
        if extent(&wide) < FLAT_RATIO * extent(&project(deep)) {
            return None;
        }
        // Thickness through the plate, cell by cell, and which face is free.
        let limit = 2 * max_depth + 2;
        let (mut plus, mut minus) = (0usize, 0usize);
        let mut through: Vec<u32> = Vec::with_capacity(surface.len());
        for (cell, p) in surface {
            let face = nearest_face(world(deep, p));
            let back = (-face.0, -face.1, -face.2);
            through.push(
                solid_run(cells, *cell, face, limit) + solid_run(cells, *cell, back, limit) - 1,
            );
            plus += usize::from(!cells.contains_key(&step(*cell, face)));
            minus += usize::from(!cells.contains_key(&step(*cell, back)));
        }
        through.sort_unstable();
        let thickness = through[through.len() / 2];
        let front = (thickness < 2 * max_depth + 1).then(|| {
            if plus * 10 > minus * 11 {
                1.0
            } else if minus * 10 > plus * 11 {
                -1.0
            } else {
                // Both faces are free: the upper one, or on a standing plate
                // the one toward the positive axis.
                let w = world(deep, first);
                let lead = if w[2].abs() >= 0.5 {
                    w[2]
                } else if w[1].abs() > w[0].abs() {
                    w[1]
                } else {
                    w[0]
                };
                lead.signum()
            }
        });
        let from = wide[0] - 0.5;
        Some(Flat {
            across,
            deep,
            from,
            width: wide[wide.len() - 1] + 0.5 - from,
            thickness,
            front,
        })
    }

    /// Lay a cell on the plate. Returns its depth from the face it is cut
    /// from, if only one is, and the deepest cut that leaves a layer behind.
    fn lay(
        &self,
        cells: &HashMap<Cell, u8>,
        cell: Cell,
        p: &mut Place,
        limit: u32,
    ) -> (Option<u32>, u32) {
        let u = p.section[0] * self.across[0] + p.section[1] * self.across[1] - self.from;
        p.around = u / self.width * TAU;
        p.radius = self.width / TAU;
        let out = world(self.deep, p);
        let face = nearest_face(out);
        let back = (-face.0, -face.1, -face.2);
        let (ahead, behind) = (
            solid_run(cells, cell, face, limit),
            solid_run(cells, cell, back, limit),
        );
        let sign = self
            .front
            .unwrap_or(if ahead <= behind { 1.0 } else { -1.0 });
        p.out = [out[0] * sign, out[1] * sign, out[2] * sign];
        match self.front {
            Some(_) => {
                let (d, rest) = if sign > 0.0 {
                    (ahead, behind)
                } else {
                    (behind, ahead)
                };
                (Some(d), d + rest - 2)
            }
            None => (None, u32::MAX),
        }
    }
}

/// A direction of a section in the world, at a place.
fn world(v: [f64; 2], p: &Place) -> [f64; 3] {
    [
        p.normal[0] * v[0] + p.binormal[0] * v[1],
        p.normal[1] * v[0] + p.binormal[1] * v[1],
        p.normal[2] * v[0] + p.binormal[2] * v[1],
    ]
}

/// The cell face closest to a direction.
fn nearest_face(v: [f64; 3]) -> Cell {
    let toward = |f: &Cell| f64::from(f.0) * v[0] + f64::from(f.1) * v[1] + f64::from(f.2) * v[2];
    FACES
        .iter()
        .copied()
        .max_by(|a, b| toward(a).total_cmp(&toward(b)))
        .unwrap_or((0, 0, 1))
}

/// Solid cells in a row from `cell` (counted) toward `face`, up to `limit`.
fn solid_run(cells: &HashMap<Cell, u8>, cell: Cell, face: Cell, limit: u32) -> u32 {
    let mut at = cell;
    let mut count = 1;
    while count < limit {
        at = step(at, face);
        if !cells.contains_key(&at) {
            break;
        }
        count += 1;
    }
    count
}

/// The plates of the unrolled wood.
struct Layout {
    /// Rim to rim across a plate, not round a trunk: no seam, and no fissure
    /// along the first and last edges.
    open: bool,
    circ: f64,
    r_ref: f64,
    /// Column boundaries from 0 to `circ`; the first and last are one seam.
    edges: Vec<f64>,
    /// Per column, plate ends `(along, slope)` in order. The end runs at
    /// `along + slope * across`, so plate ends are slanted a little.
    ends: Vec<Vec<(f64, f64)>>,
    /// Per column, each plate's index into the plate slots; a plate never
    /// takes the slot of the plate before it.
    tones: Vec<Vec<usize>>,
    wander: f64,
    wander_scale: f64,
    depth: (u32, u32),
    cracks: f64,
    seed: u64,
}

/// A cell's plate and how far it lies from the plate's edges, in cells.
struct Spot {
    column: usize,
    plate: usize,
    /// To the nearer fissure between columns.
    side: f64,
    /// To the nearer plate end.
    end: f64,
    /// Across the column, 0 at its left edge and 1 at its right.
    across: f64,
    along: f64,
}

impl Layout {
    fn new(spec: &BarkSpec, r_ref: f64, along: (f64, f64), open: bool) -> Layout {
        let mut rng = Rng::new(spec.seed ^ 0x6261_726b);
        let circ = TAU * r_ref;
        let (w0, w1) = spec.width;
        let mut widths = Vec::new();
        let mut sum = 0.0;
        while sum < circ - 0.5 * w0 {
            let w = w0 + (w1 - w0) * rng.unit();
            widths.push(w);
            sum += w;
        }
        if widths.is_empty() {
            widths.push(circ);
            sum = circ;
        }
        let fit = circ / sum;
        let mut edges = vec![0.0];
        for w in &widths {
            edges.push(edges.last().unwrap() + w * fit);
        }
        *edges.last_mut().unwrap() = circ;
        let (l0, l1) = spec.length;
        let slope_max = (l0 / (1.5 * w1)).min(0.4);
        let ends: Vec<Vec<(f64, f64)>> = widths
            .iter()
            .map(|_| {
                let mut list = Vec::new();
                let mut v = along.0 - l1 * rng.unit() - 0.5;
                while v <= along.1 + l1 {
                    list.push((v, (rng.unit() * 2.0 - 1.0) * slope_max));
                    v += l0 + (l1 - l0) * rng.unit();
                }
                list.push((v, 0.0));
                list
            })
            .collect();
        let n = spec.palettes.len();
        let tones = ends
            .iter()
            .map(|list| {
                let mut tone = (rng.next() % n as u64) as usize;
                (0..list.len())
                    .map(|_| {
                        let at = tone;
                        if n > 1 {
                            tone = (tone + 1 + (rng.next() % (n as u64 - 1)) as usize) % n;
                        }
                        at
                    })
                    .collect()
            })
            .collect();
        Layout {
            open,
            circ,
            r_ref,
            edges,
            ends,
            tones,
            wander: 0.25 * w0,
            wander_scale: (0.25 * (l0 + l1)).max(w0).max(3.0),
            depth: spec.depth,
            cracks: spec.cracks,
            seed: spec.seed,
        }
    }

    fn spot(&self, p: &Place) -> Spot {
        let u = if self.open {
            p.around / TAU * self.circ
        } else {
            (p.around / TAU).rem_euclid(1.0) * self.circ
        };
        let ring = [
            p.around.cos() * self.r_ref,
            p.around.sin() * self.r_ref,
            p.along,
        ];
        let wander = (noise3(ring, self.wander_scale, self.seed ^ 11) - 0.5) * 2.0 * self.wander;
        let uw = if self.open {
            (u + wander).clamp(0.0, self.circ)
        } else {
            (u + wander).rem_euclid(self.circ)
        };
        let column = self
            .edges
            .partition_point(|e| *e <= uw)
            .saturating_sub(1)
            .min(self.edges.len() - 2);
        let (left, right) = (self.edges[column], self.edges[column + 1]);
        // Around the wood a cell at radius r covers r / r_ref of a cell at
        // the reference radius.
        let scale = (p.radius / self.r_ref).max(0.25);
        let side = if self.open {
            let last = self.edges.len() - 2;
            let from_left = if column == 0 {
                f64::INFINITY
            } else {
                uw - left
            };
            let from_right = if column == last {
                f64::INFINITY
            } else {
                right - uw
            };
            from_left.min(from_right) * scale
        } else {
            (uw - left).min(right - uw) * scale
        };
        let across_cells = (uw - 0.5 * (left + right)) * scale;
        let ends = &self.ends[column];
        let at = |j: usize| ends[j].0 + ends[j].1 * across_cells;
        let plate = (0..ends.len())
            .take_while(|j| at(*j) <= p.along)
            .count()
            .saturating_sub(1)
            .min(ends.len() - 2);
        let end = (p.along - at(plate)).min(at(plate + 1) - p.along).abs();
        Spot {
            column,
            plate,
            side,
            end,
            across: ((uw - left) / (right - left).max(1.0e-9)).clamp(0.0, 1.0),
            along: p.along,
        }
    }

    /// The fissure depth at a spot, 0 on a plate, and whether it is a crack.
    fn fissure(&self, spot: &Spot) -> (u32, bool) {
        let ring = [spot.column as f64 * 7.3, spot.along, 0.5];
        // Plates narrow toward their ends, so the fissures between columns
        // open into the gaps between plate ends and the plates read as
        // long lenses, not bricks.
        let taper = (1.0 - spot.end / 3.0).max(0.0);
        let half = 0.45 + 0.3 * noise3(ring, 4.0, self.seed ^ 12) + 0.9 * taper * taper;
        if spot.side < half {
            let n = noise3(ring, 5.0, self.seed ^ 13);
            let span = self.depth.1 - self.depth.0 + 1;
            let deep = self.depth.0 + ((n * f64::from(span)) as u32).min(span - 1);
            return (deep, false);
        }
        if spot.end < 0.5 {
            return (self.depth.0, false);
        }
        if let Some((v, from_left, reach)) = self.crack(spot.column, spot.plate) {
            let part = if from_left {
                spot.across
            } else {
                1.0 - spot.across
            };
            if (spot.along - v).abs() < 0.5 && part < reach {
                return (1, true);
            }
        }
        (0, false)
    }

    /// A plate's crack: where along it, from which edge, and how far across.
    fn crack(&self, column: usize, plate: usize) -> Option<(f64, bool, f64)> {
        let key = (column as i64, plate as i64);
        if super::debris::hash01(key.0, key.1, 1, self.seed ^ 14) >= self.cracks {
            return None;
        }
        let ends = &self.ends[column];
        let (a, b) = (ends[plate].0, ends[plate + 1].0);
        if b - a < 4.0 {
            return None;
        }
        let t = 0.3 + 0.4 * super::debris::hash01(key.0, key.1, 2, self.seed ^ 14);
        let from_left = super::debris::hash01(key.0, key.1, 3, self.seed ^ 14) < 0.5;
        let reach = 0.35 + 0.4 * super::debris::hash01(key.0, key.1, 4, self.seed ^ 14);
        Some((a + (b - a) * t, from_left, reach))
    }

    fn slot(&self, palettes: &[u8], column: usize, plate: usize) -> u8 {
        palettes[self.tones[column][plate]]
    }
}

const NOTHING_IN_BOX: &str =
    "no cells to cut: the box holds no solid cells of the only slots outside exclude";

pub fn bark(cells: &HashMap<Cell, u8>, spec: &BarkSpec) -> Result<Barked, String> {
    if spec.palettes.is_empty() {
        return Err("palettes must hold at least one plate slot".into());
    }
    check_slots(&spec.palettes, "palettes")?;
    if let Some(dirt) = spec.dirt {
        check_slot(dirt, "dirt")?;
    }
    check_unit(spec.cracks, "cracks")?;
    let (w0, w1) = spec.width;
    if !(1.0..=64.0).contains(&w0) || !(w0..=64.0).contains(&w1) {
        return Err(format!(
            "width must be [min, max] with 1 <= min <= max <= 64, not [{w0}, {w1}]"
        ));
    }
    let (l0, l1) = spec.length;
    if !(2.0..=256.0).contains(&l0) || !(l0..=256.0).contains(&l1) {
        return Err(format!(
            "length must be [min, max] with 2 <= min <= max <= 256, not [{l0}, {l1}]"
        ));
    }
    let (d0, d1) = spec.depth;
    if !(1..=4).contains(&d0) || !(d0..=4).contains(&d1) {
        return Err(format!(
            "depth must be [min, max] with 1 <= min <= max <= 4, not [{d0}, {d1}]"
        ));
    }
    let region = spec.region;
    let runs = match &spec.direction {
        BarkDirection::Axis(axis) => {
            let len = dot(*axis, *axis).sqrt();
            if !len.is_finite() || len < 1.0e-6 {
                return Err("axis must be a non-zero direction".into());
            }
            let t = [axis[0] / len, axis[1] / len, axis[2] / len];
            let solid = region.solid(cells);
            if solid.is_empty() {
                return Ok(Barked::nothing(NOTHING_IN_BOX));
            }
            let mut mid = [0.0; 3];
            for c in &solid {
                mid[0] += f64::from(c.0);
                mid[1] += f64::from(c.1);
                mid[2] += f64::from(c.2);
            }
            let n = solid.len() as f64;
            let mid = [mid[0] / n, mid[1] / n, mid[2] / n];
            let reach = f64::from(
                (region.max.0 - region.min.0)
                    + (region.max.1 - region.min.1)
                    + (region.max.2 - region.min.2),
            ) + 2.0;
            let a = [
                mid[0] - t[0] * reach,
                mid[1] - t[1] * reach,
                mid[2] - t[2] * reach,
            ];
            let b = [
                mid[0] + t[0] * reach,
                mid[1] + t[1] * reach,
                mid[2] + t[2] * reach,
            ];
            path_runs(&[a, b], false)
        }
        BarkDirection::Path { points, curve } => {
            if points.len() < 2 || points.len() > MAX_TUBE_POINTS {
                return Err(format!(
                    "points must hold 2..={MAX_TUBE_POINTS} points along the wood"
                ));
            }
            if points
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 4096.0)
            {
                return Err("points must be finite and within 4096 cells".into());
            }
            path_runs(points, *curve)
        }
    };
    let usable = |c: &Cell| !spec.exclude.contains(*c) && allowed(&spec.only, cells[c]);
    let depth = depths_from(cells, &region, d1 + 1);
    let mut order: Vec<(Cell, u32, Cell)> = depth.iter().map(|(c, (d, o))| (*c, *d, *o)).collect();
    order.sort_unstable_by_key(|(c, _, _)| (c.2, c.1, c.0));
    let centre = |c: Cell| [f64::from(c.0), f64::from(c.1), f64::from(c.2)];

    // Bark covers the sides of the wood, not its end grain.
    let mut sides: Vec<(Cell, Place)> = Vec::new();
    let mut normals: HashMap<Cell, Option<[f64; 3]>> = HashMap::new();
    let mut in_reach = 0;
    let mut round = 0;
    for &(cell, d, _) in &order {
        if d != 1 {
            continue;
        }
        let normal = surface_normal(cells, cell);
        normals.insert(cell, normal);
        if !usable(&cell) {
            continue;
        }
        in_reach += 1;
        let p = place(&runs, centre(cell));
        // No normal: a layer one cell thick, open on both sides.
        if normal.is_none_or(|n| dot(n, p.tangent).abs() < 0.8) {
            // The outside of the wood, not the inside of a hollow.
            round += usize::from(normal.is_some_and(|n| p.radius < 1.0 || dot(n, p.out) > 0.2));
            sides.push((cell, p));
        }
    }
    if in_reach == 0 {
        return Ok(Barked::nothing(NOTHING_IN_BOX));
    }
    if sides.is_empty() {
        return Ok(Barked::nothing(
            "every surface in the box is end grain, looking along the grain, and is left alone; run axis or points along the surface to cut",
        ));
    }
    let flat = Flat::detect(cells, &sides, round, d1);
    let mut barked: HashMap<Cell, bool> = normals.keys().map(|c| (*c, false)).collect();
    let mut radii = Vec::new();
    let mut span = (f64::INFINITY, f64::NEG_INFINITY);
    for (cell, p) in &sides {
        let normal = normals[cell];
        let side = flat.is_some() || normal.is_some_and(|n| p.radius < 1.0 || dot(n, p.out) > 0.2);
        barked.insert(*cell, side);
        if side {
            radii.push(p.radius);
            span = (span.0.min(p.along), span.1.max(p.along));
        }
    }
    if radii.is_empty() {
        return Ok(Barked::nothing(
            "every surface in the box faces the grain's axis, the inside of a hollow, and is left alone; pass points along the wood",
        ));
    }
    radii.sort_by(f64::total_cmp);
    let r_ref = match &flat {
        Some(plate) => plate.width / TAU,
        None => radii[radii.len() / 2].max(1.0),
    };
    let layout = Layout::new(spec, r_ref, span, flat.is_some());

    let mut removed: HashSet<Cell> = HashSet::new();
    let mut bottoms: Vec<(Cell, u32, usize, usize)> = Vec::new();
    let mut plate_cells: Vec<(Cell, u32, usize, usize)> = Vec::new();
    let mut cracked: HashSet<(usize, usize)> = HashSet::new();
    for &(cell, d, origin) in &order {
        if !usable(&cell) || !barked.get(&origin).copied().unwrap_or(false) {
            continue;
        }
        let mut p = place(&runs, centre(cell));
        let (mut d, mut keep) = (d, u32::MAX);
        if let Some(plate) = &flat {
            let (from_front, most) = plate.lay(cells, cell, &mut p, 2 * d1 + 2);
            d = from_front.unwrap_or(d);
            keep = most;
        }
        let spot = layout.spot(&p);
        let (deep, crack) = layout.fissure(&spot);
        let deep = deep.min(keep);
        // The inner layer of a hollow's wall stays as it is, however thin
        // the wall; one cell thick, it only takes its plate's tone.
        let open = |inward: bool| {
            FACES.iter().any(|f| {
                let toward = f64::from(f.0) * p.out[0]
                    + f64::from(f.1) * p.out[1]
                    + f64::from(f.2) * p.out[2];
                (if inward { toward < -0.5 } else { toward > 0.5 })
                    && !cells.contains_key(&step(cell, *f))
            })
        };
        let inner = open(true);
        // The back of a thin plate only takes the dirt under a fissure.
        let bottom = flat.is_some() && deep > 0 && d == deep + 1;
        if inner && !open(false) && !bottom {
            continue;
        }
        if d <= deep && !inner {
            removed.insert(cell);
            if crack {
                cracked.insert((spot.column, spot.plate));
            }
        } else if deep > 0 && d == deep + 1 {
            bottoms.push((cell, d, spot.column, spot.plate));
        } else {
            plate_cells.push((cell, d, spot.column, spot.plate));
        }
    }
    let open = |c: Cell| FACES.iter().any(|f| removed.contains(&step(c, *f)));

    let mut writes: Vec<(i32, i32, i32, i32)> = Vec::new();
    let mut gone: Vec<Cell> = removed.iter().copied().collect();
    gone.sort_unstable_by_key(|c| (c.2, c.1, c.0));
    for c in &gone {
        writes.push((c.0, c.1, c.2, 0));
    }
    let mut dirtied = 0;
    for &(c, d, column, plate) in &bottoms {
        match spec.dirt {
            Some(slot) if open(c) => {
                if cells[&c] != slot {
                    writes.push((c.0, c.1, c.2, i32::from(slot)));
                }
                dirtied += 1;
            }
            _ => plate_cells.push((c, d, column, plate)),
        }
    }
    let mut plates: HashSet<(usize, usize)> = HashSet::new();
    let mut painted = 0;
    for &(c, d, column, plate) in &plate_cells {
        if d != 1 && !open(c) {
            continue;
        }
        plates.insert((column, plate));
        let slot = layout.slot(&spec.palettes, column, plate);
        if cells[&c] != slot {
            writes.push((c.0, c.1, c.2, i32::from(slot)));
            painted += 1;
        }
    }
    let warning = if sides.len() * 3 < in_reach {
        Some(format!(
            "only {} of {in_reach} surface cells run along the grain; the rest is end grain and is left alone, so run axis or points along the surface to cut",
            sides.len()
        ))
    } else if !gone.is_empty() {
        (flat.is_none() && round * 3 < sides.len()).then(|| {
            format!(
                "only {round} of {} surface cells face away from the grain's axis, so the rest were left alone; pass points along the wood or a smaller box",
                sides.len()
            )
        })
    } else if flat.as_ref().is_some_and(|plate| plate.thickness == 1) {
        Some(
            "one cell thick: the plates took their tones, but a fissure would cut through; thicken it to 2 cells to cut it"
                .into(),
        )
    } else {
        Some("no fissure fell on the wood; lower width and length".into())
    };
    Ok(Barked {
        writes,
        plates: plates.len(),
        removed: gone.len(),
        dirtied,
        painted,
        cracks: cracked.len(),
        warning,
    })
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
