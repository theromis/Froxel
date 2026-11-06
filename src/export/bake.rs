//! Light baked into the colour atlas, one texel per cell face, by real rays.
//!
//! A texel is `albedo × (ambient × energy × (1 − occlusion × blocked) + sun ×
//! energy × max(n·l, 0) × open)` in linear light, clamped and encoded to sRGB.
//! `blocked` is the cosine-weighted share of hemisphere rays that hit a solid
//! within `distance`, a hit at `t` counting `1 − (t / distance)²`, so the edge
//! of the reach leaves no band. `open` is the share of sun rays that get out.
//!
//! The rays of a face follow one fixed pattern, so the result is a pure
//! function of the geometry and the options: no seed, no noise, and the same
//! bytes however the threads share the work.

use glam::Vec3;
use serde_json::{json, Value};

use super::quads::face_axes;
use super::*;

pub const MAX_RAYS: u32 = 1024;
pub const MAX_SUN_RAYS: u32 = 256;
pub const MAX_DISTANCE: f32 = 4096.0;
pub const MAX_SOFTNESS: f32 = 45.0;
pub const DEFAULT_RAYS: u32 = 64;
pub const DEFAULT_DISTANCE: f32 = 16.0;
/// Toward the sun, Z up, when the options name no direction: high, from the
/// front right.
pub const DEFAULT_SUN: [f32; 3] = [0.35, -0.45, 0.82];
/// Off the face plane, in cells. Far below a cell and far above `f64` error.
const LIFT: f64 = 1e-6;

const KEYS: [&str; 14] = [
    "occlusion",
    "distance",
    "rays",
    "sunSoftness",
    "sunRays",
    "preset",
    "lightingPresets",
    "description",
    "sunDirection",
    "godotSunBasis",
    "sunColor",
    "sunEnergy",
    "ambientColor",
    "ambientEnergy",
];

/// What the `bake` option holds, resolved from presets. Colours are sRGB
/// floats as an engine's picker stores them, as [`crate::lighting::Lighting`].
#[derive(Clone, Debug, PartialEq)]
pub struct Bake {
    /// How much ambient a fully blocked face loses, 0..1.
    pub occlusion: f32,
    /// Reach of the occlusion rays: cells, or scene units in a scene bake.
    pub distance: f32,
    /// Occlusion rays per cell face.
    pub rays: u32,
    /// Toward the sun, Z up, unit length.
    pub sun_direction: [f32; 3],
    pub sun_color: [f32; 3],
    pub sun_energy: f32,
    pub ambient_color: [f32; 3],
    pub ambient_energy: f32,
    /// Half angle of the sun's disc in degrees. 0 is a hard shadow.
    pub sun_softness: f32,
    /// Shadow rays per cell face.
    pub sun_rays: u32,
}

impl Default for Bake {
    /// Occlusion alone, under a white ambient of 1.
    fn default() -> Self {
        Bake {
            occlusion: 1.0,
            distance: DEFAULT_DISTANCE,
            rays: DEFAULT_RAYS,
            sun_direction: Vec3::from_array(DEFAULT_SUN).normalize().to_array(),
            sun_color: [1.0; 3],
            sun_energy: 0.0,
            ambient_color: [1.0; 3],
            ambient_energy: 1.0,
            sun_softness: 0.0,
            sun_rays: 1,
        }
    }
}

fn number(
    map: &serde_json::Map<String, Value>,
    key: &str,
    low: f32,
    high: f32,
) -> Result<Option<f32>, String> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(found) => found
            .as_f64()
            .map(|value| value as f32)
            .filter(|value| (low..=high).contains(value))
            .map(Some)
            .ok_or_else(|| format!("bake {key} must be a number from {low} to {high}")),
    }
}

fn whole(
    map: &serde_json::Map<String, Value>,
    key: &str,
    low: u32,
    high: u32,
) -> Result<Option<u32>, String> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(found) => found
            .as_u64()
            .filter(|value| (low as u64..=high as u64).contains(value))
            .map(|value| Some(value as u32))
            .ok_or_else(|| format!("bake {key} must be a whole number from {low} to {high}")),
    }
}

impl Bake {
    /// MCP `bake`: absent, null or false is off, true is every default, and
    /// an object sets fields. Named lighting (`preset`, the lighting fields)
    /// starts from a white sun of 1 over a white ambient of 0.3; with none of
    /// it the bake is occlusion alone. A preset's exposure, background and
    /// LUT are not baked: a game's tonemapper and grade reach unshaded
    /// surfaces too.
    pub fn parse(value: Option<&Value>) -> Result<Option<Bake>, String> {
        let map = match value {
            None | Some(Value::Null) | Some(Value::Bool(false)) => return Ok(None),
            Some(Value::Bool(true)) => return Ok(Some(Bake::default())),
            Some(Value::Object(map)) => map,
            Some(_) => return Err("bake must be an object of bake options".into()),
        };
        if let Some(key) = map.keys().find(|key| !KEYS.contains(&key.as_str())) {
            return Err(format!("bake takes {}, not `{key}`", KEYS.join(", ")));
        }
        let mut bake = Bake::default();
        let lighting: serde_json::Map<String, Value> = map
            .iter()
            .filter(|(key, value)| {
                !value.is_null()
                    && matches!(
                        key.as_str(),
                        "preset"
                            | "sunDirection"
                            | "godotSunBasis"
                            | "sunColor"
                            | "sunEnergy"
                            | "ambientColor"
                            | "ambientEnergy"
                    )
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        if map.get("preset").and_then(Value::as_str) == Some(crate::lighting::VIEWPORT) {
            return Err("bake: viewport is the editor's light, name another preset".into());
        }
        if !lighting.is_empty() {
            let presets = crate::lighting::Presets::load(map.get("lightingPresets"))?;
            let Some(light) = presets.resolve(&Value::Object(lighting))? else {
                return Err("bake: viewport is the editor's light, name another preset".into());
            };
            if let Some(direction) = light.sun_direction {
                bake.sun_direction = direction.to_array();
            }
            bake.sun_color = light.sun_color;
            bake.sun_energy = light.sun_energy;
            bake.ambient_color = light.ambient_color;
            bake.ambient_energy = light.ambient_energy;
        } else if map.get("lightingPresets").is_some_and(|v| !v.is_null()) {
            return Err("bake lightingPresets needs preset to name one".into());
        }
        if let Some(value) = number(map, "occlusion", 0.0, 1.0)? {
            bake.occlusion = value;
        }
        if let Some(value) = number(map, "distance", f32::MIN_POSITIVE, MAX_DISTANCE)? {
            bake.distance = value;
        }
        if let Some(value) = whole(map, "rays", 1, MAX_RAYS)? {
            bake.rays = value;
        }
        if let Some(value) = number(map, "sunSoftness", 0.0, MAX_SOFTNESS)? {
            bake.sun_softness = value;
        }
        // One ray is a hard shadow; a disc needs a few to read as soft.
        bake.sun_rays = if bake.sun_softness > 0.0 { 16 } else { 1 };
        if let Some(value) = whole(map, "sunRays", 1, MAX_SUN_RAYS)? {
            bake.sun_rays = value;
        }
        Ok(Some(bake))
    }

    /// Every field, as [`Bake::parse`] takes it. Numbers to six places, so
    /// 1.45 reads 1.45 in a reply and in the glb.
    pub fn json(&self) -> Value {
        let short = |v: f32| (f64::from(v) * 1e6).round() / 1e6;
        json!({
            "occlusion": short(self.occlusion),
            "distance": short(self.distance),
            "rays": self.rays,
            "sunDirection": self.sun_direction.map(short),
            "sunColor": self.sun_color.map(short),
            "sunEnergy": short(self.sun_energy),
            "ambientColor": self.ambient_color.map(short),
            "ambientEnergy": short(self.ambient_energy),
            "sunSoftness": short(self.sun_softness),
            "sunRays": self.sun_rays,
        })
    }
}

/// Anything rays stop at. `origin` and `dir` share the caller's frame.
pub(super) trait Occluders: Sync {
    /// The distance to the nearest solid along the unit `dir`, up to `reach`.
    fn hit(&self, origin: [f64; 3], dir: [f64; 3], reach: f64) -> Option<f64>;
}

/// Opaque cells, as a sparse bit grid of 16³ bricks over their bounds.
pub(super) struct Grid {
    min: [i32; 3],
    /// Cells per axis.
    size: [i32; 3],
    /// Bricks per axis.
    top: [usize; 3],
    /// Brick number plus one per brick position; 0 is empty.
    index: Vec<u32>,
    bricks: Vec<[u64; 64]>,
}

impl Grid {
    pub(super) fn new(cells: &[Cell]) -> Grid {
        let mut min = [i32::MAX; 3];
        let mut max = [i32::MIN; 3];
        for cell in cells {
            for (axis, value) in [cell.0, cell.1, cell.2].into_iter().enumerate() {
                min[axis] = min[axis].min(value);
                max[axis] = max[axis].max(value);
            }
        }
        if cells.is_empty() {
            min = [0; 3];
            max = [-1; 3];
        }
        let size = [0, 1, 2].map(|axis| max[axis] - min[axis] + 1);
        let top = size.map(|cells| ((cells as usize) + 15) / 16);
        let mut grid = Grid {
            min,
            size,
            top,
            index: vec![0; top[0] * top[1] * top[2]],
            bricks: Vec::new(),
        };
        for cell in cells {
            let at = [cell.0 - min[0], cell.1 - min[1], cell.2 - min[2]];
            let slot = grid.brick_at(at);
            if grid.index[slot] == 0 {
                grid.bricks.push([0; 64]);
                grid.index[slot] = grid.bricks.len() as u32;
            }
            let brick = &mut grid.bricks[grid.index[slot] as usize - 1];
            let (word, bit) = Grid::bit_at(at);
            brick[word] |= 1 << bit;
        }
        grid
    }

    /// The opaque cells of the exported objects, in world space. Translucent
    /// slots let the light through.
    pub(super) fn of_session(session: &Session, export_hidden: bool) -> Grid {
        let mut cells = Vec::new();
        for object in &session.objects {
            if object.hidden && !export_hidden {
                continue;
            }
            for (cell, slot) in &object.cells {
                if *slot != 0 && session.palette[*slot as usize][3] == 255 {
                    cells.push((
                        cell.0 + object.translation.0,
                        cell.1 + object.translation.1,
                        cell.2 + object.translation.2,
                    ));
                }
            }
        }
        Grid::new(&cells)
    }

    fn brick_at(&self, at: [i32; 3]) -> usize {
        let brick = at.map(|value| value as usize / 16);
        (brick[2] * self.top[1] + brick[1]) * self.top[0] + brick[0]
    }

    fn bit_at(at: [i32; 3]) -> (usize, u32) {
        let local = at.map(|value| value as usize % 16);
        (
            local[2] * 4 + local[1] / 4,
            (local[1] % 4 * 16 + local[0]) as u32,
        )
    }

    pub(super) fn is_empty(&self) -> bool {
        self.bricks.is_empty()
    }

    /// Memory of the grid, in bytes.
    pub(super) fn bytes(&self) -> usize {
        self.index.len() * 4 + self.bricks.len() * 512
    }

    /// The box of the cells, on the lattice.
    pub(super) fn bounds(&self) -> ([f64; 3], [f64; 3]) {
        let low = self.min.map(f64::from);
        let high = [0, 1, 2].map(|axis| f64::from(self.min[axis] + self.size[axis]));
        (low, high)
    }

    /// `at` is from the grid's corner and inside it.
    fn solid(&self, at: [i32; 3]) -> bool {
        let slot = self.index[self.brick_at(at)];
        if slot == 0 {
            return false;
        }
        let (word, bit) = Grid::bit_at(at);
        self.bricks[slot as usize - 1][word] >> bit & 1 == 1
    }
}

impl Occluders for Grid {
    /// A 3D DDA through the cells, after clipping to the bounds.
    fn hit(&self, origin: [f64; 3], dir: [f64; 3], reach: f64) -> Option<f64> {
        if self.bricks.is_empty() {
            return None;
        }
        let (mut near, mut far) = (0f64, reach);
        for axis in 0..3 {
            let low = f64::from(self.min[axis]);
            let high = low + f64::from(self.size[axis]);
            if dir[axis] == 0.0 {
                if origin[axis] < low || origin[axis] > high {
                    return None;
                }
                continue;
            }
            let a = (low - origin[axis]) / dir[axis];
            let b = (high - origin[axis]) / dir[axis];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
        }
        if near > far {
            return None;
        }
        let mut at = [0i32; 3];
        let mut next = [f64::INFINITY; 3];
        let mut delta = [f64::INFINITY; 3];
        let mut step = [0i32; 3];
        for axis in 0..3 {
            let local = origin[axis] + dir[axis] * near - f64::from(self.min[axis]);
            at[axis] = (local.floor() as i32).clamp(0, self.size[axis] - 1);
            if dir[axis] > 0.0 {
                step[axis] = 1;
                delta[axis] = 1.0 / dir[axis];
                next[axis] = (f64::from(at[axis] + 1) + f64::from(self.min[axis]) - origin[axis])
                    / dir[axis];
            } else if dir[axis] < 0.0 {
                step[axis] = -1;
                delta[axis] = -1.0 / dir[axis];
                next[axis] =
                    (f64::from(at[axis]) + f64::from(self.min[axis]) - origin[axis]) / dir[axis];
            }
        }
        let mut t = near;
        loop {
            if self.solid(at) {
                return Some(t);
            }
            let axis = if next[0] <= next[1] && next[0] <= next[2] {
                0
            } else if next[1] <= next[2] {
                1
            } else {
                2
            };
            t = next[axis];
            if t > far {
                return None;
            }
            at[axis] += step[axis];
            if at[axis] < 0 || at[axis] >= self.size[axis] {
                return None;
            }
            next[axis] += delta[axis];
        }
    }
}

/// A patch of surface: a corner and two edges, in the tracer's frame.
#[derive(Clone, Copy)]
pub(super) struct Patch {
    pub corner: [f64; 3],
    pub u: [f64; 3],
    pub v: [f64; 3],
    pub normal: [f64; 3],
}

fn add(a: [f64; 3], b: [f64; 3], scale: f64) -> [f64; 3] {
    [0, 1, 2].map(|axis| a[axis] + b[axis] * scale)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(a: [f64; 3]) -> [f64; 3] {
    let length = dot(a, a).sqrt();
    a.map(|value| value / length)
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// A point of the fixed face pattern, inside the face's edge.
fn on_face(i: u32) -> (f64, f64) {
    // The R2 low-discrepancy sequence.
    const G: f64 = 1.324_717_957_244_746;
    let n = f64::from(i);
    let u = (0.5 + n / G).fract();
    let v = (0.5 + n / (G * G)).fract();
    (0.05 + 0.9 * u, 0.05 + 0.9 * v)
}

/// The fixed pattern of ray offsets, the same for every face.
pub(super) struct Rays {
    /// `(u, v)` on the face and `(tangent, bitangent, normal)` of a
    /// cosine-weighted hemisphere direction.
    hemisphere: Vec<((f64, f64), [f64; 3])>,
    /// `(u, v)` on the face, and an offset on the sun's disc.
    sun: Vec<((f64, f64), [f64; 2])>,
    /// The sun in the tracer's frame, and two axes across its disc, scaled
    /// by the tangent of the half angle.
    toward: [f64; 3],
    across: [[f64; 3]; 2],
}

impl Rays {
    /// `toward` is the unit direction to the sun in the tracer's frame.
    pub(super) fn new(bake: &Bake, toward: [f64; 3]) -> Rays {
        const TURN: f64 = std::f64::consts::TAU * 0.618_033_988_749_895;
        let hemisphere = (0..bake.rays)
            .map(|i| {
                let share = (f64::from(i) + 0.5) / f64::from(bake.rays);
                let (radius, angle) = (share.sqrt(), TURN * f64::from(i));
                let dir = [
                    radius * angle.cos(),
                    radius * angle.sin(),
                    (1.0 - share).sqrt(),
                ];
                (on_face(i), dir)
            })
            .collect();
        let sun = (0..bake.sun_rays)
            .map(|i| {
                if bake.sun_rays == 1 {
                    return ((0.5, 0.5), [0.0, 0.0]);
                }
                let share = (f64::from(i) + 0.5) / f64::from(bake.sun_rays);
                let (radius, angle) = (share.sqrt(), TURN * f64::from(i));
                (on_face(i), [radius * angle.cos(), radius * angle.sin()])
            })
            .collect();
        let helper = if toward[2].abs() < 0.9 {
            [0.0, 0.0, 1.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        let tangent = f64::from(bake.sun_softness).to_radians().tan();
        let first = unit(cross(helper, toward));
        let second = cross(toward, first);
        Rays {
            hemisphere,
            sun,
            toward,
            across: [first.map(|v| v * tangent), second.map(|v| v * tangent)],
        }
    }

    /// The light on a patch per colour channel, in linear light, unclamped.
    /// `lift` moves an origin off the surface, in the tracer's units.
    pub(super) fn light(
        &self,
        bake: &Bake,
        occluders: &impl Occluders,
        patch: &Patch,
        lift: f64,
    ) -> [f32; 3] {
        let reach = f64::from(bake.distance);
        let on = |(u, v): (f64, f64)| {
            add(
                add(add(patch.corner, patch.u, u), patch.v, v),
                patch.normal,
                lift,
            )
        };
        let mut blocked = 0.0;
        if bake.occlusion > 0.0 && bake.ambient_energy > 0.0 {
            let (tangent, bitangent) = (unit(patch.u), unit(patch.v));
            for (at, local) in &self.hemisphere {
                let dir = add(
                    add(tangent.map(|t| t * local[0]), bitangent, local[1]),
                    patch.normal,
                    local[2],
                );
                if let Some(t) = occluders.hit(on(*at), dir, reach) {
                    let t = t / reach;
                    blocked += 1.0 - t * t;
                }
            }
            blocked /= self.hemisphere.len() as f64;
        }
        let facing = dot(patch.normal, self.toward);
        let mut open = 0.0;
        if bake.sun_energy > 0.0 && facing > 0.0 {
            for (at, disc) in &self.sun {
                let dir = unit(add(
                    add(self.toward, self.across[0], disc[0]),
                    self.across[1],
                    disc[1],
                ));
                // A ray of the disc that dips under the face hits its own cell.
                if occluders.hit(on(*at), dir, f64::INFINITY).is_none() {
                    open += 1.0;
                }
            }
            open /= self.sun.len() as f64;
        }
        let visible = 1.0 - bake.occlusion * blocked as f32;
        let direct = (facing.max(0.0) * open) as f32;
        [0, 1, 2].map(|c| {
            crate::lighting::to_linear(bake.ambient_color[c]) * bake.ambient_energy * visible
                + crate::lighting::to_linear(bake.sun_color[c]) * bake.sun_energy * direct
        })
    }
}

/// `count` results of `work`, on every core. Each result is a pure function
/// of its index, so the output does not depend on how the work is shared.
pub(super) fn parallel_map(count: usize, work: impl Fn(usize) -> [f32; 3] + Sync) -> Vec<[f32; 3]> {
    let mut out = vec![[0.0f32; 3]; count];
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let chunks = std::sync::Mutex::new(out.chunks_mut(256).enumerate());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let next = chunks.lock().expect("chunks").next();
                let Some((chunk, slots)) = next else {
                    break;
                };
                for (i, slot) in slots.iter_mut().enumerate() {
                    *slot = work(chunk * 256 + i);
                }
            });
        }
    });
    out
}

/// The patch of a face of `cell`, in the lattice's own Z-up frame.
pub(super) fn cell_patch(face: usize, cell: Cell) -> Patch {
    let (normal, u_axis, v_axis) = face_axes(face);
    let mut corner = [f64::from(cell.0), f64::from(cell.1), f64::from(cell.2)];
    let mut out = [0.0; 3];
    if face % 2 == 0 {
        corner[normal] += 1.0;
        out[normal] = 1.0;
    } else {
        out[normal] = -1.0;
    }
    let (mut u, mut v) = ([0.0; 3], [0.0; 3]);
    u[u_axis] = 1.0;
    v[v_axis] = 1.0;
    Patch {
        corner,
        u,
        v,
        normal: out,
    }
}

/// The light on each of `faces` of a document, one per face.
pub(super) fn session_light(
    session: &Session,
    options: &MeshExport,
    bake: &Bake,
    faces: &[(usize, Cell)],
) -> Vec<[f32; 3]> {
    let grid = Grid::of_session(session, options.export_hidden);
    let rays = Rays::new(bake, bake.sun_direction.map(f64::from));
    parallel_map(faces.len(), |index| {
        let (face, cell) = faces[index];
        rays.light(bake, &grid, &cell_patch(face, cell), LIFT)
    })
}

#[cfg(test)]
#[path = "../../tests/bake.rs"]
mod tests;
