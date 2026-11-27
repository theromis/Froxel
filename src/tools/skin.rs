//! Skin: an object's surface covered with turned bricks of uneven length in
//! staggered rows along a flow direction, each sitting on the smoothed
//! surface with its own relief, tilt, and colour, so a creature reads as
//! built from irregular blocks instead of a grid.
//!
//! The skin is stored as its settings on the object and its bricks are
//! derived from the object's cells, like strands, so every drawing, tracing,
//! and export path sees the same boxes and an edit to the cells reshapes it.
//!
//! Layout: the cells are blurred into a density field whose half level is
//! the smoothed surface. Each exposed cell face gives a sample, moved onto
//! that surface, with the field's normal. Around the flow axis every sample
//! has a ring position (its angle about the slice's centre times the
//! slice's radius) and a position along the axis. Rows run along the axis
//! `width` apart around the ring; each row is cut into bricks of random
//! length, offset from the row beside it by `stagger`. A brick is one box
//! over its samples: along the flow projected onto the surface, across the
//! row, and out along the normal.

use std::collections::HashMap;

use glam::Vec3;

use super::StrandBox;
use crate::state::Cell;

/// Most cells an object may have for its skin to be built.
pub const MAX_SKIN_CELLS: usize = 2_000_000;
/// Most bricks on one object; a skin past it keeps the first ones.
pub const MAX_SKIN_BRICKS: usize = 50_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SkinAxis {
    /// The longest axis of the cells' spread.
    Auto,
    X,
    Y,
    Z,
}

impl SkinAxis {
    pub const NAMES: &'static str = "auto, x, y, or z";

    pub fn name(self) -> &'static str {
        match self {
            SkinAxis::Auto => "auto",
            SkinAxis::X => "x",
            SkinAxis::Y => "y",
            SkinAxis::Z => "z",
        }
    }

    pub fn parse(name: &str) -> Option<SkinAxis> {
        match name {
            "auto" => Some(SkinAxis::Auto),
            "x" => Some(SkinAxis::X),
            "y" => Some(SkinAxis::Y),
            "z" => Some(SkinAxis::Z),
            _ => None,
        }
    }

    pub fn code(self) -> u8 {
        match self {
            SkinAxis::Auto => 0,
            SkinAxis::X => 1,
            SkinAxis::Y => 2,
            SkinAxis::Z => 3,
        }
    }

    pub fn from_code(code: u8) -> Option<SkinAxis> {
        [SkinAxis::Auto, SkinAxis::X, SkinAxis::Y, SkinAxis::Z]
            .get(code as usize)
            .copied()
    }
}

#[derive(Clone, Debug)]
pub struct Skin {
    /// The flow the rows run along.
    pub axis: SkinAxis,
    /// Shortest and longest brick, in cells along the flow.
    pub length: [f32; 2],
    /// Row spacing around the body, in cells.
    pub width: f32,
    /// How far a brick stands out past the smoothed surface, in cells.
    pub height: f32,
    /// How deep a brick reaches below it, in cells.
    pub depth: f32,
    /// Fraction of a brick that one row is offset from the next, 0..1.
    pub stagger: f32,
    /// Random change of each brick's height, in cells.
    pub relief: f32,
    /// Random tilt and turn of each brick, in degrees.
    pub tilt: f32,
    /// How far a brick turns from the nearest axis toward the smoothed
    /// surface, 0..1: 0 keeps every face square to the grid, so a curve
    /// reads as steps of rows; 1 lays bricks on the surface like shingles.
    pub follow: f32,
    /// Groove left between bricks, in cells.
    pub gap: f32,
    /// Blur radius of the smoothed surface, in cells.
    pub smooth: f32,
    /// Parts thinner than about this many cells keep their cells bare:
    /// fins, whiskers, thin plates. 0 covers everything.
    pub thin: f32,
    /// Palette slots never covered: eyes, teeth, markings to keep.
    pub bare: Vec<u8>,
    pub seed: u32,
}

impl Skin {
    fn floats(&self) -> [f32; 12] {
        [
            self.length[0],
            self.length[1],
            self.width,
            self.height,
            self.depth,
            self.stagger,
            self.relief,
            self.tilt,
            self.follow,
            self.gap,
            self.smooth,
            self.thin,
        ]
    }

    /// Each setting's name and allowed range, in `floats` order (length is
    /// both of its ends).
    pub const RANGES: [(&'static str, f32, f32); 12] = [
        ("length", 0.25, 64.0),
        ("length", 0.25, 64.0),
        ("width", 0.25, 64.0),
        ("height", 0.0, 8.0),
        ("depth", 0.0, 8.0),
        ("stagger", 0.0, 1.0),
        ("relief", 0.0, 8.0),
        ("tilt", 0.0, 45.0),
        ("follow", 0.0, 1.0),
        ("gap", 0.0, 1.0),
        ("smooth", 0.5, 8.0),
        ("thin", 0.0, 64.0),
    ];

    /// Refuses a setting outside its range, a length whose ends are
    /// reversed, a brick no thicker than nothing, or slot 0 in `bare`.
    pub fn check(&self) -> Result<(), String> {
        for (value, (name, lo, hi)) in self.floats().iter().zip(Skin::RANGES) {
            if !value.is_finite() || *value < lo || *value > hi {
                return Err(format!("skin {name} must be {lo}..={hi}, not {value}"));
            }
        }
        if self.length[0] > self.length[1] {
            return Err("skin length must be [shortest, longest]".into());
        }
        if self.height + self.depth <= 0.0 {
            return Err("skin height plus depth must be above 0".into());
        }
        if self.bare.contains(&0) {
            return Err("skin bare slots are 1..=255".into());
        }
        Ok(())
    }
}

impl PartialEq for Skin {
    fn eq(&self, other: &Skin) -> bool {
        self.axis == other.axis
            && self.bare == other.bare
            && self.seed == other.seed
            && self
                .floats()
                .iter()
                .zip(other.floats())
                .all(|(a, b)| a.to_bits() == b.to_bits())
    }
}

impl Eq for Skin {}

impl std::hash::Hash for Skin {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.axis.hash(state);
        self.bare.hash(state);
        self.seed.hash(state);
        for v in self.floats() {
            v.to_bits().hash(state);
        }
    }
}

impl Default for Skin {
    fn default() -> Self {
        Skin {
            axis: SkinAxis::Auto,
            length: [2.0, 4.0],
            width: 1.5,
            height: 0.4,
            depth: 0.75,
            stagger: 0.5,
            relief: 0.3,
            tilt: 2.0,
            follow: 0.0,
            gap: 0.12,
            smooth: 2.0,
            thin: 3.0,
            bare: Vec::new(),
            seed: 0,
        }
    }
}

fn hash(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^= h >> 16;
    h
}

/// A number in 0..1 from three keys and the seed.
fn unit(seed: u32, a: i64, b: i64, salt: u32) -> f32 {
    let h = hash(
        seed.wrapping_mul(0x9E37_79B9)
            ^ hash(a as u32 ^ ((a >> 32) as u32).wrapping_mul(0x85EB_CA6B))
            ^ hash((b as u32).wrapping_add(0x27D4_EB2F) ^ salt.wrapping_mul(0x1656_67B1)),
    );
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// The density field: occupancy blurred by a separable Gaussian.
struct Field {
    min: [i32; 3],
    dims: [usize; 3],
    values: Vec<f32>,
}

impl Field {
    fn new(cells: &HashMap<Cell, u8>, sigma: f32) -> Field {
        let radius = (sigma * 2.5).ceil().max(1.0) as i32;
        let pad = radius + 2;
        let (mut lo, mut hi) = ([i32::MAX; 3], [i32::MIN; 3]);
        for c in cells.keys() {
            for (k, v) in [c.0, c.1, c.2].into_iter().enumerate() {
                lo[k] = lo[k].min(v);
                hi[k] = hi[k].max(v);
            }
        }
        let min = lo.map(|v| v - pad);
        let dims = [0, 1, 2].map(|k| (hi[k] - lo[k] + 1 + 2 * pad) as usize);
        let mut values = vec![0.0f32; dims[0] * dims[1] * dims[2]];
        let index = |x: usize, y: usize, z: usize| (z * dims[1] + y) * dims[0] + x;
        for c in cells.keys() {
            let p = [c.0 - min[0], c.1 - min[1], c.2 - min[2]].map(|v| v as usize);
            values[index(p[0], p[1], p[2])] = 1.0;
        }
        let weights: Vec<f32> = (-radius..=radius)
            .map(|d| (-(d * d) as f32 / (2.0 * sigma * sigma)).exp())
            .collect();
        let total: f32 = weights.iter().sum();
        let weights: Vec<f32> = weights.iter().map(|w| w / total).collect();
        let mut scratch = vec![0.0f32; values.len()];
        for axis in 0..3 {
            let stride = [1, dims[0], dims[0] * dims[1]][axis];
            let n = dims[axis] as i32;
            for (i, out) in scratch.iter_mut().enumerate() {
                let coord = (i / stride) as i32 % n;
                let mut sum = 0.0;
                for (k, w) in weights.iter().enumerate() {
                    let at = coord + k as i32 - radius;
                    if at >= 0 && at < n {
                        sum +=
                            w * values[(i as i64 + (at - coord) as i64 * stride as i64) as usize];
                    }
                }
                *out = sum;
            }
            std::mem::swap(&mut values, &mut scratch);
        }
        Field { min, dims, values }
    }

    /// Trilinear value at a geometric point (cell centres at `+0.5`).
    fn at(&self, p: Vec3) -> f32 {
        let q = p - Vec3::new(self.min[0] as f32, self.min[1] as f32, self.min[2] as f32) - 0.5;
        let base = q.floor();
        let f = q - base;
        let mut sum = 0.0;
        for corner in 0..8 {
            let o = [corner & 1, corner >> 1 & 1, corner >> 2 & 1];
            let c = [0, 1, 2].map(|k| base[k] as i64 + o[k] as i64);
            if (0..3).any(|k| c[k] < 0 || c[k] >= self.dims[k] as i64) {
                continue;
            }
            let w = (0..3)
                .map(|k| if o[k] == 1 { f[k] } else { 1.0 - f[k] })
                .product::<f32>();
            let index =
                (c[2] as usize * self.dims[1] + c[1] as usize) * self.dims[0] + c[0] as usize;
            sum += w * self.values[index];
        }
        sum
    }

    fn gradient(&self, p: Vec3) -> Vec3 {
        let h = 0.5;
        Vec3::new(
            self.at(p + Vec3::X * h) - self.at(p - Vec3::X * h),
            self.at(p + Vec3::Y * h) - self.at(p - Vec3::Y * h),
            self.at(p + Vec3::Z * h) - self.at(p - Vec3::Z * h),
        ) / (2.0 * h)
    }
}

struct Sample {
    at: Vec3,
    /// The cell face the sample came from.
    face: Vec3,
    normal: Vec3,
    cell: Cell,
}

fn axis_of(cells: &HashMap<Cell, u8>, axis: SkinAxis) -> Vec3 {
    match axis {
        SkinAxis::X => Vec3::X,
        SkinAxis::Y => Vec3::Y,
        SkinAxis::Z => Vec3::Z,
        SkinAxis::Auto => {
            let n = cells.len().max(1) as f32;
            let mean = cells
                .keys()
                .map(|c| Vec3::new(c.0 as f32, c.1 as f32, c.2 as f32))
                .sum::<Vec3>()
                / n;
            let mut m = glam::Mat3::ZERO;
            for c in cells.keys() {
                let d = Vec3::new(c.0 as f32, c.1 as f32, c.2 as f32) - mean;
                m += glam::Mat3::from_cols(d * d.x, d * d.y, d * d.z);
            }
            // Power iteration for the largest spread.
            let mut v = Vec3::new(1.0, 0.7, 0.3).normalize();
            for _ in 0..64 {
                v = (m * v).normalize_or(Vec3::X);
            }
            v
        }
    }
}

/// The bricks of a skin over `cells`, in the object's local geometric frame.
/// The layout is made at `origin` (the object's translation), so the rows of
/// rig parts that meet continue each other.
pub fn skin_boxes(cells: &HashMap<Cell, u8>, skin: &Skin, origin: Cell) -> Vec<StrandBox> {
    let solid: HashMap<Cell, u8> = cells
        .iter()
        .filter(|(_, s)| **s != 0)
        .map(|(c, s)| ((c.0 + origin.0, c.1 + origin.1, c.2 + origin.2), *s))
        .collect();
    let shift = Vec3::new(origin.0 as f32, origin.1 as f32, origin.2 as f32);
    if solid.is_empty() || solid.len() > MAX_SKIN_CELLS {
        return Vec::new();
    }
    let field = Field::new(&solid, skin.smooth.max(0.5));
    let mut order: Vec<Cell> = solid.keys().copied().collect();
    order.sort_unstable();
    let mut samples = Vec::new();
    for &c in &order {
        for (d, n) in [
            ((1, 0, 0), Vec3::X),
            ((-1, 0, 0), -Vec3::X),
            ((0, 1, 0), Vec3::Y),
            ((0, -1, 0), -Vec3::Y),
            ((0, 0, 1), Vec3::Z),
            ((0, 0, -1), -Vec3::Z),
        ] {
            if solid.contains_key(&(c.0 + d.0, c.1 + d.1, c.2 + d.2))
                || skin.bare.contains(&solid[&c])
            {
                continue;
            }
            let centre = Vec3::new(c.0 as f32, c.1 as f32, c.2 as f32) + 0.5;
            // A thin part never reaches the half level of the blurred
            // field halfway through its thickness.
            if skin.thin > 0.0 && field.at(centre - n * (skin.thin * 0.5 - 0.5).max(0.0)) < 0.5 {
                continue;
            }
            let mut p = centre + n * 0.5;
            // Onto the half level along the field's gradient.
            for _ in 0..4 {
                let g = field.gradient(p);
                let len2 = g.length_squared();
                if len2 < 1.0e-6 {
                    break;
                }
                let step = ((field.at(p) - 0.5) / len2).clamp(-0.5, 0.5);
                p -= g * step;
            }
            if p.distance(centre) > 2.0 {
                p = centre + n * 0.5;
            }
            let normal = (-field.gradient(p)).try_normalize().unwrap_or(n);
            samples.push(Sample {
                at: p,
                face: centre + n * 0.5,
                normal,
                cell: c,
            });
        }
    }
    let axis = axis_of(&solid, skin.axis);
    // Ring frame: angle 0 on top, the seam underneath.
    let side = axis
        .cross(Vec3::Z)
        .try_normalize()
        .unwrap_or_else(|| axis.any_orthonormal_vector());
    let top = side.cross(axis).normalize();
    // Slice centres and radii along the axis.
    let slice = |p: Vec3| p.dot(axis).floor() as i64;
    let mut centres: HashMap<i64, (Vec3, f32)> = HashMap::new();
    for c in solid.keys() {
        let p = Vec3::new(c.0 as f32, c.1 as f32, c.2 as f32) + 0.5;
        let e = centres.entry(slice(p)).or_insert((Vec3::ZERO, 0.0));
        e.0 += p;
        e.1 += 1.0;
    }
    let centres: HashMap<i64, Vec3> = centres.into_iter().map(|(k, (s, n))| (k, s / n)).collect();
    let centre_at = |p: Vec3| -> Vec3 {
        let k = slice(p);
        for d in [0, -1, 1, -2, 2] {
            if let Some(c) = centres.get(&(k + d)) {
                return *c;
            }
        }
        p
    };
    let mut radius: HashMap<i64, (f32, f32)> = HashMap::new();
    for s in &samples {
        let r = s.at - centre_at(s.at);
        let r = r - axis * r.dot(axis);
        let e = radius.entry(slice(s.at)).or_insert((0.0, 0.0));
        e.0 += r.length();
        e.1 += 1.0;
    }
    let radius: HashMap<i64, f32> = radius.into_iter().map(|(k, (s, n))| (k, s / n)).collect();
    // Group samples into bricks.
    let [lo, hi] = [
        skin.length[0].min(skin.length[1]).max(0.25),
        skin.length[0].max(skin.length[1]).max(0.25),
    ];
    let width = skin.width.max(0.25);
    // Each sample's place on the layout: ring position, along, and brick.
    struct Placed {
        v: f32,
        u: f32,
        span: (f32, f32),
    }
    let mut bricks: HashMap<(i64, i64), Vec<(usize, Placed)>> = HashMap::new();
    for (index, s) in samples.iter().enumerate() {
        let r = s.at - centre_at(s.at);
        let r = r - axis * r.dot(axis);
        let angle = r.dot(side).atan2(r.dot(top));
        let v = angle * radius.get(&slice(s.at)).copied().unwrap_or(1.0).max(0.5);
        let row = (v / width).round() as i64;
        let u = s.at.dot(axis);
        // Brick edges along this row: random lengths from a staggered start.
        let start = -(skin.stagger * (row.rem_euclid(2)) as f32 * 0.5 * (lo + hi))
            - unit(skin.seed, row, 0, 1) * hi * (1.0 - skin.stagger);
        // Bricks come in runs of `RUN` whose random lengths are scaled to
        // fill the run exactly, so every sample of a row finds the same edges.
        const RUN: i64 = 8;
        let mean = (lo + hi) * 0.5;
        let run = ((u - start) / (mean * RUN as f32)).floor() as i64;
        let lengths: Vec<f32> = (0..RUN)
            .map(|k| lo + unit(skin.seed, row, run * RUN + k, 2) * (hi - lo))
            .collect();
        let scale = mean * RUN as f32 / lengths.iter().sum::<f32>();
        let mut edge = start + run as f32 * mean * RUN as f32;
        let mut number = run * RUN;
        let mut len = lengths[0] * scale;
        for k in 0..RUN as usize {
            len = lengths[k] * scale;
            if edge + len > u || k + 1 == RUN as usize {
                break;
            }
            edge += len;
            number += 1;
        }
        bricks.entry((row, number)).or_default().push((
            index,
            Placed {
                v,
                u,
                span: (edge, edge + len),
            },
        ));
    }
    let mut keys: Vec<(i64, i64)> = bricks.keys().copied().collect();
    keys.sort_unstable();
    let mut out = Vec::with_capacity(keys.len());
    for key in keys {
        let members = &bricks[&key];
        let n = members.len() as f32;
        let mean_at = members.iter().map(|(i, _)| samples[*i].at).sum::<Vec3>() / n;
        let mean_u = members.iter().map(|(_, p)| p.u).sum::<f32>() / n;
        let mean_v = members.iter().map(|(_, p)| p.v).sum::<f32>() / n;
        let smooth_normal = members
            .iter()
            .map(|(i, _)| samples[*i].normal)
            .sum::<Vec3>()
            .try_normalize()
            .unwrap_or(Vec3::Z);
        let square = {
            let a = smooth_normal.abs();
            let k = if a.x >= a.y && a.x >= a.z {
                0
            } else if a.y >= a.z {
                1
            } else {
                2
            };
            let mut v = Vec3::ZERO;
            v[k] = smooth_normal[k].signum();
            v
        };
        let follow = skin.follow.clamp(0.0, 1.0);
        let normal = (square * (1.0 - follow) + smooth_normal * follow).normalize_or(square);
        let along = (axis - normal * axis.dot(normal))
            .try_normalize()
            .unwrap_or_else(|| {
                (top - normal * top.dot(normal)).normalize_or(normal.any_orthonormal_vector())
            });
        let across = normal.cross(along);
        // Which way the ring position grows on this brick.
        let r = (mean_at - centre_at(mean_at))
            .reject_from(axis)
            .normalize_or(top);
        let ring_dir = if across.dot(axis.cross(r)) >= 0.0 {
            1.0
        } else {
            -1.0
        };
        let (u0, u1) = members[0].1.span;
        let (lo_u, hi_u) = members.iter().fold((f32::MAX, f32::MIN), |(a, b), (_, p)| {
            (a.min(p.u), b.max(p.u))
        });
        // The nominal brick, cut back where its row has no surface.
        let (u0, u1) = (u0.max(lo_u - 0.5), u1.min(hi_u + 0.5));
        let length = (u1 - u0).max(0.5);
        let row_v = key.0 as f32 * width;
        let centre_ab = mean_at
            + along * (((u0 + u1) * 0.5 - mean_u) / along.dot(axis).abs().max(0.3))
            + across * ((row_v - mean_v) * ring_dir);
        let (r1, r2, r3, r4) = (
            unit(skin.seed, key.0, key.1, 3) * 2.0 - 1.0,
            unit(skin.seed, key.0, key.1, 4) * 2.0 - 1.0,
            unit(skin.seed, key.0, key.1, 5) * 2.0 - 1.0,
            unit(skin.seed, key.0, key.1, 6) * 2.0 - 1.0,
        );
        // The base is the mean level of the cell faces under the brick, or
        // the smoothed surface where that is further out: on a tight curve
        // the smoothed surface sinks below the cells.
        let base = (members
            .iter()
            .map(|(i, _)| (samples[*i].face - centre_ab).dot(normal))
            .sum::<f32>()
            / n)
            .clamp(0.0, 0.75);
        let out_h = (skin.height + r1 * skin.relief).max(0.02);
        let thick = base + out_h + skin.depth;
        let centre = centre_ab + normal * (base + out_h - thick * 0.5);
        let tilt = skin.tilt.to_radians();
        let turn = glam::Quat::from_axis_angle(normal, r2 * tilt * 0.5)
            * glam::Quat::from_axis_angle(along, r3 * tilt)
            * glam::Quat::from_axis_angle(across, r4 * tilt);
        let (t, b, u) = (turn * along, turn * across, turn * normal);
        // The commonest colour of the cells under the brick.
        let mut counts: Vec<(u8, usize)> = Vec::new();
        for (i, _) in members {
            let slot = solid[&samples[*i].cell];
            match counts.iter_mut().find(|(s, _)| *s == slot) {
                Some(entry) => entry.1 += 1,
                None => counts.push((slot, 1)),
            }
        }
        let slot = counts
            .iter()
            .max_by_key(|(s, n)| (*n, *s))
            .map(|e| e.0)
            .unwrap_or(1);
        if out.len() == MAX_SKIN_BRICKS {
            break;
        }
        out.push(StrandBox {
            center: (centre - shift).to_array(),
            axes: [t.to_array(), b.to_array(), u.to_array()],
            half: [
                ((length - skin.gap) * 0.5).max(0.05),
                ((width * 1.2 - skin.gap) * 0.5).max(0.05),
                thick * 0.5,
            ],
            slot,
        });
    }
    out
}

#[cfg(test)]
#[path = "../../tests/skin.rs"]
mod tests;
