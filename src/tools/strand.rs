//! Strands: chains of cubes along a curve, each cube turned along the path.
//!
//! A strand is stored as its definition (control points, section, step,
//! colours) on its object, in the object's local cell frame, and its boxes
//! are derived from it whenever they are drawn, traced, exported, or saved.
//! Nothing is rasterized into the grid: the viewport, `preview`,
//! `model_view`, Render, and mesh export all see the same turned boxes.
//!
//! Points follow `sweep`: a whole number is a cell centre, `twist` turns the
//! section counter-clockwise looking along the path, and `scale` multiplies
//! the section. The section's `width` runs to the path's left (seen from
//! above, walking along it) and its `thickness` up. The path is cut into
//! cubes of equal length, the nearest to `step` that fits the length
//! exactly, so a strand always ends at its last point.

use super::sweep::catmull;

/// Most control points in one strand.
pub const MAX_STRAND_POINTS: usize = 256;
/// Most strands on one object.
pub const MAX_STRANDS: usize = 4096;
/// Most boxes of one object's strands together, after merging.
pub const MAX_STRAND_BOXES: usize = 20_000;
/// Section and step range, in cells.
pub const MIN_STRAND_SIZE: f32 = 0.125;
pub const MAX_STRAND_SIZE: f32 = 64.0;
/// Most cubes along one strand before merging.
const MAX_CUBES: usize = 8192;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrandPoint {
    /// Local cell coordinates; a whole number is a cell centre.
    pub at: [f32; 3],
    /// Degrees counter-clockwise looking along the path.
    pub twist: f32,
    /// Multiplies the section, 0.05..=16.
    pub scale: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrandPattern {
    /// Slots spread evenly from the first point to the last.
    Gradient,
    /// Slots in turn, `period` cells of path each.
    Rings,
    /// One slot per cube, chosen by `seed`.
    Random,
}

impl StrandPattern {
    pub fn name(self) -> &'static str {
        match self {
            StrandPattern::Gradient => "gradient",
            StrandPattern::Rings => "rings",
            StrandPattern::Random => "random",
        }
    }

    pub fn parse(name: &str) -> Option<StrandPattern> {
        match name {
            "gradient" => Some(StrandPattern::Gradient),
            "rings" => Some(StrandPattern::Rings),
            "random" => Some(StrandPattern::Random),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Strand {
    pub points: Vec<StrandPoint>,
    /// A Catmull-Rom curve through the points instead of straight spans.
    pub smooth: bool,
    pub width: f32,
    pub thickness: f32,
    pub step: f32,
    /// One slot, or the slots `pattern` lays out.
    pub slots: Vec<u8>,
    pub pattern: StrandPattern,
    pub period: f32,
    pub seed: u32,
}

// Every float is checked finite by `Strand::check`, so equality is total.
impl Eq for Strand {}

/// One derived box: a centre, three unit axes (along the path, its left, its
/// up), the half extents along them, and a palette slot. Geometric
/// coordinates: cell `(i, j, k)` spans `i..i + 1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrandBox {
    pub center: [f32; 3],
    pub axes: [[f32; 3]; 3],
    pub half: [f32; 3],
    pub slot: u8,
}

impl StrandBox {
    /// The map from the unit cube `-1..1` to this box.
    pub fn matrix(&self) -> glam::Mat4 {
        let a = |i: usize| glam::Vec3::from_array(self.axes[i]) * self.half[i];
        glam::Mat4::from_cols(
            a(0).extend(0.0),
            a(1).extend(0.0),
            a(2).extend(0.0),
            glam::Vec3::from_array(self.center).extend(1.0),
        )
    }

    /// The eight corners, in the order bit 0 along, bit 1 left, bit 2 up.
    pub fn corners(&self) -> [[f32; 3]; 8] {
        let mut out = [[0.0; 3]; 8];
        for (index, corner) in out.iter_mut().enumerate() {
            for (axis, half) in self.half.iter().enumerate() {
                let sign = if index >> axis & 1 == 1 { 1.0 } else { -1.0 };
                for (k, value) in corner.iter_mut().enumerate() {
                    *value += self.axes[axis][k] * half * sign;
                }
            }
            for (k, value) in corner.iter_mut().enumerate() {
                *value += self.center[k];
            }
        }
        out
    }
}

impl Default for Strand {
    fn default() -> Self {
        Strand {
            points: Vec::new(),
            smooth: true,
            width: 1.0,
            thickness: 1.0,
            step: 1.0,
            slots: vec![1],
            pattern: StrandPattern::Gradient,
            period: 2.0,
            seed: 0,
        }
    }
}

impl Strand {
    /// Refuses a strand that cannot be drawn: too few or too many points,
    /// sizes outside their range, slot 0, or a value that is not finite.
    pub fn check(&self) -> Result<(), String> {
        if self.points.len() < 2 || self.points.len() > MAX_STRAND_POINTS {
            return Err(format!(
                "a strand needs 2..={MAX_STRAND_POINTS} points, not {}",
                self.points.len()
            ));
        }
        for (index, point) in self.points.iter().enumerate() {
            if !point.at.iter().all(|v| v.is_finite() && v.abs() <= 1.0e5) {
                return Err(format!("point {index} must be finite, within 100000 cells"));
            }
            if !point.twist.is_finite() {
                return Err(format!("point {index} twist must be a number"));
            }
            if !(0.05..=16.0).contains(&point.scale) {
                return Err(format!("point {index} scale must be 0.05..=16"));
            }
        }
        for (name, value) in [
            ("width", self.width),
            ("thickness", self.thickness),
            ("step", self.step),
        ] {
            if !(MIN_STRAND_SIZE..=MAX_STRAND_SIZE).contains(&value) {
                return Err(format!(
                    "{name} must be {MIN_STRAND_SIZE}..={MAX_STRAND_SIZE} cells, not {value}"
                ));
            }
        }
        if self.slots.is_empty() || self.slots.len() > 64 {
            return Err("a strand needs 1..=64 palette slots".into());
        }
        if self.slots.contains(&0) {
            return Err("strand palette slots must be 1..=255".into());
        }
        if !(0.25..=512.0).contains(&self.period) {
            return Err("period must be 0.25..=512 cells".into());
        }
        Ok(())
    }

    /// The path length in cells, along the curve the boxes follow.
    pub fn length(&self) -> f32 {
        samples(self).last().map_or(0.0, |s| s.along)
    }

    /// The same strand moved by `delta` cells.
    pub fn shifted(&self, delta: [f32; 3]) -> Strand {
        let mut out = self.clone();
        for point in &mut out.points {
            for (value, d) in point.at.iter_mut().zip(delta) {
                *value += d;
            }
        }
        out
    }

    /// The same strand mirrored across the plane `axis = centre` of local
    /// cell coordinates. The twist turns the other way, so a leaf that leans
    /// left leans right.
    pub fn mirrored(&self, axis: usize, centre: f32) -> Strand {
        let mut out = self.clone();
        for point in &mut out.points {
            point.at[axis] = 2.0 * centre - point.at[axis];
            point.twist = -point.twist;
        }
        out
    }

    /// The boxes this strand is drawn with, in the object's local geometric
    /// frame, consecutive cubes of the same frame and slot merged.
    pub fn boxes(&self) -> Vec<StrandBox> {
        let samples = samples(self);
        let Some(total) = samples.last().map(|s| s.along) else {
            return Vec::new();
        };
        if total <= 1.0e-6 {
            return Vec::new();
        }
        let count = ((total / self.step).round() as usize).clamp(1, MAX_CUBES);
        let pitch = total / count as f32;
        let mut cubes: Vec<StrandBox> = Vec::with_capacity(count);
        let mut cursor = 0usize;
        let mut left_before: Option<glam::Vec3> = None;
        for index in 0..count {
            let along = (index as f32 + 0.5) * pitch;
            while cursor + 2 < samples.len() && samples[cursor + 1].along < along {
                cursor += 1;
            }
            let (a, b) = (
                &samples[cursor],
                &samples[(cursor + 1).min(samples.len() - 1)],
            );
            let span = (b.along - a.along).max(1.0e-9);
            let t = ((along - a.along) / span).clamp(0.0, 1.0);
            let at = a.at.lerp(b.at, t);
            let tangent = a.tangent.lerp(b.tangent, t).normalize_or(a.tangent);
            let twist = a.twist + (b.twist - a.twist) * t;
            let scale = a.scale + (b.scale - a.scale) * t;
            let left = frame_left(tangent, left_before);
            left_before = Some(left);
            let up = tangent.cross(left);
            let (sin, cos) = twist.to_radians().sin_cos();
            let left_t = left * cos + up * sin;
            let up_t = up * cos - left * sin;
            cubes.push(StrandBox {
                center: (at + glam::Vec3::splat(0.5)).to_array(),
                axes: [tangent.to_array(), left_t.to_array(), up_t.to_array()],
                half: [
                    pitch / 2.0,
                    self.width * scale / 2.0,
                    self.thickness * scale / 2.0,
                ],
                slot: self.slot_at(index, count, along),
            });
        }
        merge(cubes)
    }

    fn slot_at(&self, index: usize, count: usize, along: f32) -> u8 {
        let n = self.slots.len();
        if n == 1 {
            return self.slots[0];
        }
        let pick = match self.pattern {
            StrandPattern::Gradient => (index * n / count.max(1)).min(n - 1),
            StrandPattern::Rings => (along / self.period).floor() as usize % n,
            StrandPattern::Random => {
                let mut h = (index as u32)
                    .wrapping_mul(0x9E37_79B9)
                    .wrapping_add(self.seed.wrapping_mul(0x85EB_CA6B));
                h ^= h >> 16;
                h = h.wrapping_mul(0x7FEB_352D);
                h ^= h >> 15;
                h as usize % n
            }
        };
        self.slots[pick]
    }
}

/// The path's left at a tangent: horizontal while the path is not vertical,
/// and otherwise the previous left made square to the tangent, as `sweep`
/// keeps its frame.
fn frame_left(tangent: glam::Vec3, before: Option<glam::Vec3>) -> glam::Vec3 {
    if tangent.z.abs() < 0.995 {
        if let Some(left) = glam::Vec3::Z.cross(tangent).try_normalize() {
            return left;
        }
    }
    before
        .and_then(|l| (l - tangent * l.dot(tangent)).try_normalize())
        .or_else(|| (glam::Vec3::Y - tangent * tangent.y).try_normalize())
        .unwrap_or(glam::Vec3::X)
}

struct Sample {
    at: glam::Vec3,
    tangent: glam::Vec3,
    twist: f32,
    scale: f32,
    along: f32,
}

/// Dense samples of the path (eight per cell of chord), with the length so far.
fn samples(strand: &Strand) -> Vec<Sample> {
    let points: Vec<[f64; 3]> = strand.points.iter().map(|p| p.at.map(f64::from)).collect();
    let mut out: Vec<Sample> = Vec::new();
    let last = points.len().saturating_sub(1);
    for span in 0..last {
        let (a, b) = (points[span], points[span + 1]);
        let chord = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt();
        if chord < 1.0e-6 {
            continue;
        }
        let steps = (chord * 8.0).ceil().clamp(1.0, 4096.0) as usize;
        let first = if out.is_empty() { 0 } else { 1 };
        let (pa, pb) = (strand.points[span], strand.points[span + 1]);
        for step in first..=steps {
            let t = step as f64 / steps as f64;
            let (at, tangent) = if strand.smooth {
                let p0 = points[span.saturating_sub(1)];
                let p3 = points[(span + 2).min(last)];
                catmull(p0, a, b, p3, t)
            } else {
                let at = [
                    a[0] + (b[0] - a[0]) * t,
                    a[1] + (b[1] - a[1]) * t,
                    a[2] + (b[2] - a[2]) * t,
                ];
                (at, [b[0] - a[0], b[1] - a[1], b[2] - a[2]])
            };
            let at = glam::Vec3::new(at[0] as f32, at[1] as f32, at[2] as f32);
            let chord_dir = glam::Vec3::new(
                (b[0] - a[0]) as f32,
                (b[1] - a[1]) as f32,
                (b[2] - a[2]) as f32,
            )
            .normalize_or(glam::Vec3::X);
            let tangent = glam::Vec3::new(tangent[0] as f32, tangent[1] as f32, tangent[2] as f32)
                .try_normalize()
                .unwrap_or(chord_dir);
            let along = out
                .last()
                .map_or(0.0, |s: &Sample| s.along + s.at.distance(at));
            let t = t as f32;
            out.push(Sample {
                at,
                tangent,
                twist: pa.twist + (pb.twist - pa.twist) * t,
                scale: pa.scale + (pb.scale - pa.scale) * t,
                along,
            });
        }
    }
    out
}

/// Joins consecutive cubes that share a frame, a section, and a slot and
/// touch end to end, so a straight run is one long box.
fn merge(cubes: Vec<StrandBox>) -> Vec<StrandBox> {
    let mut out: Vec<StrandBox> = Vec::with_capacity(cubes.len());
    for cube in cubes {
        if let Some(last) = out.last_mut() {
            if joins(last, &cube) {
                let a = glam::Vec3::from_array(last.center)
                    - glam::Vec3::from_array(last.axes[0]) * last.half[0];
                let b = glam::Vec3::from_array(cube.center)
                    + glam::Vec3::from_array(cube.axes[0]) * cube.half[0];
                last.center = ((a + b) / 2.0).to_array();
                last.half[0] = a.distance(b) / 2.0;
                continue;
            }
        }
        out.push(cube);
    }
    out
}

fn joins(a: &StrandBox, b: &StrandBox) -> bool {
    const EPS: f32 = 1.0e-4;
    if a.slot != b.slot
        || (a.half[1] - b.half[1]).abs() > EPS
        || (a.half[2] - b.half[2]).abs() > EPS
    {
        return false;
    }
    for axis in 0..3 {
        let (u, v) = (
            glam::Vec3::from_array(a.axes[axis]),
            glam::Vec3::from_array(b.axes[axis]),
        );
        if u.distance(v) > EPS {
            return false;
        }
    }
    // The far end of `a` meets the near end of `b` on the shared axis.
    let along = glam::Vec3::from_array(a.axes[0]);
    let end = glam::Vec3::from_array(a.center) + along * a.half[0];
    let start = glam::Vec3::from_array(b.center) - along * b.half[0];
    end.distance(start) < 1.0e-3
}

/// The boxes of every strand of an object, refused past [`MAX_STRAND_BOXES`].
pub fn object_boxes(strands: &[Strand]) -> Vec<StrandBox> {
    strands.iter().flat_map(Strand::boxes).collect()
}

/// Checks a whole strand list against the per-object limits.
pub fn check_object(strands: &[Strand]) -> Result<usize, String> {
    if strands.len() > MAX_STRANDS {
        return Err(format!("an object holds at most {MAX_STRANDS} strands"));
    }
    let boxes: usize = strands.iter().map(|s| s.boxes().len()).sum();
    if boxes > MAX_STRAND_BOXES {
        return Err(format!(
            "the object's strands would need {boxes} boxes, more than {MAX_STRAND_BOXES}; use a larger step or fewer strands"
        ));
    }
    Ok(boxes)
}

/// Where a ray meets a box: the distance along `dir` and the outward face
/// normal, or `None`. The ray starts outside or inside; from inside it hits
/// the far face.
pub fn ray_box(
    inverse: &glam::Mat4,
    matrix: &glam::Mat4,
    origin: glam::Vec3,
    dir: glam::Vec3,
) -> Option<(f32, glam::Vec3)> {
    let o = inverse.transform_point3(origin);
    let d = inverse.transform_vector3(dir);
    let mut near = f32::MIN;
    let mut far = f32::MAX;
    let mut near_axis = 0usize;
    let mut far_axis = 0usize;
    for axis in 0..3 {
        let (oa, da) = (o[axis], d[axis]);
        if da.abs() < 1.0e-12 {
            if oa.abs() > 1.0 {
                return None;
            }
            continue;
        }
        let (mut t0, mut t1) = ((-1.0 - oa) / da, (1.0 - oa) / da);
        if t0 > t1 {
            std::mem::swap(&mut t0, &mut t1);
        }
        if t0 > near {
            near = t0;
            near_axis = axis;
        }
        if t1 < far {
            far = t1;
            far_axis = axis;
        }
        if near > far {
            return None;
        }
    }
    if far < 0.0 {
        return None;
    }
    let (t, axis, outward) = if near >= 0.0 {
        (near, near_axis, -d[near_axis].signum())
    } else {
        (far, far_axis, d[far_axis].signum())
    };
    // The face normal in world space: the unit cube's normal through the
    // inverse transpose.
    let mut local = glam::Vec3::ZERO;
    local[axis] = outward;
    let normal = inverse
        .transpose()
        .transform_vector3(local)
        .try_normalize()
        .unwrap_or_else(|| matrix.transform_vector3(local).normalize_or_zero());
    Some((t, normal))
}

/// The cells the chosen strands' boxes cover (all of them when `which` is
/// empty): every cell whose centre lies inside a box, in local cells.
pub fn strand_cells(
    strands: &[Strand],
    which: &[usize],
) -> std::collections::HashMap<crate::state::Cell, u8> {
    let boxes = strands
        .iter()
        .enumerate()
        .filter(|(n, _)| which.is_empty() || which.contains(n))
        .flat_map(|(_, strand)| strand.boxes());
    box_cells(boxes)
}

/// Cells whose centre lies inside any of `boxes`, each with its box's slot
/// (a later box over an earlier one).
pub fn box_cells(
    boxes: impl IntoIterator<Item = StrandBox>,
) -> std::collections::HashMap<crate::state::Cell, u8> {
    let mut cells = std::collections::HashMap::new();
    for b in boxes {
        let inverse = b.matrix().inverse();
        let mut min = glam::Vec3::splat(f32::MAX);
        let mut max = glam::Vec3::splat(f32::MIN);
        for corner in b.corners() {
            let c = glam::Vec3::from_array(corner);
            min = min.min(c);
            max = max.max(c);
        }
        for z in min.z.floor() as i32..=max.z.ceil() as i32 {
            for y in min.y.floor() as i32..=max.y.ceil() as i32 {
                for x in min.x.floor() as i32..=max.x.ceil() as i32 {
                    let centre = glam::Vec3::new(x as f32, y as f32, z as f32) + 0.5;
                    let unit = inverse.transform_point3(centre);
                    if unit.abs().max_element() <= 1.0 + 1.0e-5 {
                        cells.insert((x, y, z), b.slot);
                    }
                }
            }
        }
    }
    cells
}

#[cfg(test)]
#[path = "../../tests/strand.rs"]
mod tests;
