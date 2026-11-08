//! One placement of many `.vox` assets baked together: each is shaded by the
//! others and by ground, into one merged mesh with one colour atlas.
//!
//! A placement is the frame of a glb that `export_mesh` wrote, as a game
//! places it: `world = translation + Lean × scale × Ry(yaw) × (x, z, −y)` of
//! a cell corner, Y up, right-handed. `Lean` is the identity unless the
//! placement has a `lean`: then it tilts by `degrees` about the horizontal
//! axis square to the yaw `toward`, so the top moves that way, pivoting on
//! `translation`. An instance's `sway` multiplies the sway of its moving
//! slots in the written mesh, so kinds of plant sway apart. Cells stay on their own lattice, so a yaw or a lean of any
//! angle costs nothing: a ray enters an instance through the inverse
//! transform and walks that source's one grid. A BVH over the instances and
//! the occluder triangles finds which to enter.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};

use super::bake::{self, Bake, Grid, Occluders, Patch, Rays};
use super::quads::{self, axis_of, GreedyQuad};
use super::*;

pub const MAX_INSTANCES: usize = 4096;
/// Larger than most GPUs sample, so a level this size has to be split.
pub const MAX_ATLAS: u32 = 16384;
/// Off the surface, in cells.
const LIFT: f64 = 1e-6;
const LEAF: usize = 4;

/// The rigid part of a placement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub translation: [f64; 3],
    /// Degrees about +Y, as Godot's `rotation.y`.
    pub yaw: f64,
    pub scale: f64,
    /// A tilt after yaw and scale; `None` is upright.
    pub lean: Option<Lean>,
    /// Multiplies the sway (`TEXCOORD_1` x) of an instance's moving slots,
    /// so one kind of plant sways more than another in one baked mesh; 1
    /// keeps the asset's own. Bob is left alone. Not used on occluders.
    pub sway: f64,
}

/// A tilt of a placement, pivoting on its translation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lean {
    /// How far the top tilts, in degrees.
    pub degrees: f64,
    /// Which way the top moves: a yaw about +Y, as `Place::yaw`, where 0 is
    /// toward −Z.
    pub toward: f64,
}

impl Lean {
    /// `p` turned by `sign × degrees` about the axis square to `toward`.
    fn turn(&self, p: [f64; 3], sign: f64) -> [f64; 3] {
        let (sin_t, cos_t) = self.toward.to_radians().sin_cos();
        // Toward is (−sin, 0, −cos); the axis is +Y × toward.
        let axis = [-cos_t, 0.0, sin_t];
        let (sin, cos) = (sign * self.degrees).to_radians().sin_cos();
        let dot = axis[0] * p[0] + axis[2] * p[2];
        let cross = [
            axis[1] * p[2] - axis[2] * p[1],
            axis[2] * p[0] - axis[0] * p[2],
            axis[0] * p[1] - axis[1] * p[0],
        ];
        [0, 1, 2].map(|i| p[i] * cos + cross[i] * sin + axis[i] * dot * (1.0 - cos))
    }

    fn json(&self) -> Value {
        json!({"degrees": self.degrees, "toward": self.toward})
    }
}

impl Place {
    fn rotate(&self, p: [f64; 3]) -> [f64; 3] {
        let (sin, cos) = self.yaw.to_radians().sin_cos();
        let turned = [p[0] * cos + p[2] * sin, p[1], -p[0] * sin + p[2] * cos];
        match &self.lean {
            None => turned,
            Some(lean) => lean.turn(turned, 1.0),
        }
    }

    fn point(&self, p: [f64; 3]) -> [f64; 3] {
        let turned = self.rotate(p);
        [0, 1, 2].map(|axis| self.translation[axis] + self.scale * turned[axis])
    }

    /// A point in the frame of the file, from the world.
    fn local_point(&self, p: [f64; 3]) -> [f64; 3] {
        let q = [0, 1, 2].map(|axis| (p[axis] - self.translation[axis]) / self.scale);
        self.local_vector(q)
    }

    fn local_vector(&self, p: [f64; 3]) -> [f64; 3] {
        let p = match &self.lean {
            None => p,
            Some(lean) => lean.turn(p, -1.0),
        };
        let (sin, cos) = self.yaw.to_radians().sin_cos();
        [p[0] * cos - p[2] * sin, p[1], p[0] * sin + p[2] * cos]
    }
}

/// Froxel's Z-up cell axes to the glb's Y up.
fn y_up(p: [f64; 3]) -> [f64; 3] {
    [p[0], p[2], -p[1]]
}

/// The glb's Y up back to Froxel's cell axes.
fn z_up(p: [f64; 3]) -> [f64; 3] {
    [p[0], -p[2], p[1]]
}

#[derive(Clone, Debug, PartialEq)]
pub enum Occluder {
    /// A `.glb`, `.gltf`, or `.obj`, placed like an instance.
    Mesh { path: PathBuf, place: Place },
    /// Solid below this height: rays stop at it, and a face under it is dark.
    Plane { height: f64 },
    /// Heights at grid points: `rows[z][x]` at `origin + cell × (x, z)`.
    Height {
        origin: [f64; 2],
        cell: f64,
        rows: Vec<Vec<f64>>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneBake {
    pub instances: Vec<(PathBuf, Place)>,
    pub occluders: Vec<Occluder>,
    pub bake: Bake,
    pub export_hidden: bool,
}

fn numbers<const N: usize>(value: &Value, what: &str) -> Result<[f64; N], String> {
    let list: Option<Vec<f64>> = value
        .as_array()
        .filter(|list| list.len() == N)
        .and_then(|list| list.iter().map(Value::as_f64).collect());
    list.and_then(|list| list.try_into().ok())
        .ok_or_else(|| format!("{what} must be {N} numbers"))
}

fn place_of(item: &Value, what: &str) -> Result<Place, String> {
    let translation = match item.get("translation") {
        None | Some(Value::Null) => [0.0; 3],
        Some(value) => numbers(value, &format!("{what} translation"))?,
    };
    let number = |key: &str, default: f64| match item.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(value) => value
            .as_f64()
            .ok_or_else(|| format!("{what} {key} must be a number")),
    };
    let scale = number("scale", 1.0)?;
    if scale <= 0.0 {
        return Err(format!("{what} scale must be above 0"));
    }
    let sway = number("sway", 1.0)?;
    if !(0.0..=4.0).contains(&sway) {
        return Err(format!("{what} sway must be within 0..4"));
    }
    Ok(Place {
        translation,
        yaw: number("yaw", 0.0)?,
        scale,
        lean: lean_of(item.get("lean"), what)?,
        sway,
    })
}

fn lean_of(value: Option<&Value>, what: &str) -> Result<Option<Lean>, String> {
    let value = match value {
        None | Some(Value::Null) => return Ok(None),
        Some(value) => value,
    };
    let what = format!("{what} lean");
    only(value, &["degrees", "toward"], &what)?;
    let number = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_f64)
            .ok_or_else(|| format!("{what} needs {key}, a number of degrees"))
    };
    let degrees = number("degrees")?;
    if degrees.abs() > 90.0 {
        return Err(format!("{what} degrees must be within -90..90"));
    }
    Ok(Some(Lean {
        degrees,
        toward: number("toward")?,
    }))
}

fn only(item: &Value, keys: &[&str], what: &str) -> Result<(), String> {
    match item.as_object() {
        None => Err(format!("{what} must be an object")),
        Some(map) => match map.keys().find(|key| !keys.contains(&key.as_str())) {
            Some(key) => Err(format!("{what} takes {}, not `{key}`", keys.join(", "))),
            None => Ok(()),
        },
    }
}

impl SceneBake {
    /// `value` is the scene JSON; relative paths in it start at `dir`. `bake`
    /// is the caller's option, over the scene's own.
    pub fn parse(value: &Value, dir: &Path, bake: Option<&Value>) -> Result<SceneBake, String> {
        only(
            value,
            &[
                "instances",
                "occluders",
                "bake",
                "exportHidden",
                "description",
            ],
            "the scene",
        )?;
        let path = |text: &str| {
            let path = Path::new(text);
            if path.is_relative() {
                dir.join(path)
            } else {
                path.to_path_buf()
            }
        };
        let list = value
            .get("instances")
            .and_then(Value::as_array)
            .filter(|list| !list.is_empty())
            .ok_or(
                "the scene needs instances, a list of {source, translation, yaw, scale, lean, sway}",
            )?;
        if list.len() > MAX_INSTANCES {
            return Err(format!("a scene holds at most {MAX_INSTANCES} instances"));
        }
        let mut instances = Vec::with_capacity(list.len());
        for (index, item) in list.iter().enumerate() {
            let what = format!("instance {index}");
            only(
                item,
                &["source", "translation", "yaw", "scale", "lean", "sway"],
                &what,
            )?;
            let source = item
                .get("source")
                .and_then(Value::as_str)
                .filter(|source| source.ends_with(".vox"))
                .ok_or_else(|| format!("{what} source must be a .vox path"))?;
            instances.push((path(source), place_of(item, &what)?));
        }
        let mut occluders = Vec::new();
        let none = Vec::new();
        let given = match value.get("occluders") {
            None | Some(Value::Null) => &none,
            Some(found) => found.as_array().ok_or("occluders must be a list")?,
        };
        for (index, item) in given.iter().enumerate() {
            let what = format!("occluder {index}");
            only(
                item,
                &[
                    "mesh",
                    "plane",
                    "height",
                    "translation",
                    "yaw",
                    "scale",
                    "lean",
                ],
                &what,
            )?;
            if let Some(mesh) = item.get("mesh") {
                let mesh = mesh
                    .as_str()
                    .ok_or_else(|| format!("{what} mesh must be a path"))?;
                occluders.push(Occluder::Mesh {
                    path: path(mesh),
                    place: place_of(item, &what)?,
                });
            } else if let Some(height) = item.get("plane") {
                let height = height
                    .as_f64()
                    .ok_or_else(|| format!("{what} plane must be a height"))?;
                occluders.push(Occluder::Plane { height });
            } else if let Some(field) = item.get("height") {
                occluders.push(height_of(field, &what)?);
            } else {
                return Err(format!("{what} needs mesh, plane, or height"));
            }
        }
        let given = bake.or(value.get("bake")).filter(|value| !value.is_null());
        let bake = match given {
            None => Bake::default(),
            Some(given) => Bake::parse(Some(given))?
                .ok_or("a scene bake cannot be off: remove bake or set its fields")?,
        };
        let export_hidden = match value.get("exportHidden") {
            None | Some(Value::Null) => false,
            Some(found) => found
                .as_bool()
                .ok_or("exportHidden must be true or false")?,
        };
        Ok(SceneBake {
            instances,
            occluders,
            bake,
            export_hidden,
        })
    }
}

fn height_of(field: &Value, what: &str) -> Result<Occluder, String> {
    only(
        field,
        &["origin", "cell", "rows"],
        &format!("{what} height"),
    )?;
    let origin = match field.get("origin") {
        None | Some(Value::Null) => [0.0; 2],
        Some(value) => numbers(value, &format!("{what} height origin"))?,
    };
    let cell = match field.get("cell") {
        None | Some(Value::Null) => 1.0,
        Some(value) => value
            .as_f64()
            .filter(|cell| *cell > 0.0)
            .ok_or_else(|| format!("{what} height cell must be above 0"))?,
    };
    let bad = || format!("{what} height rows must be equal lists of 2 or more numbers");
    let list = field
        .get("rows")
        .and_then(Value::as_array)
        .filter(|rows| rows.len() >= 2)
        .ok_or_else(bad)?;
    let mut rows = Vec::with_capacity(list.len());
    for row in list {
        let row: Vec<f64> = row
            .as_array()
            .and_then(|row| row.iter().map(Value::as_f64).collect::<Option<_>>())
            .ok_or_else(bad)?;
        if row.len() < 2 || row.len() != list[0].as_array().map_or(0, Vec::len) {
            return Err(bad());
        }
        rows.push(row);
    }
    Ok(Occluder::Height { origin, cell, rows })
}

/// What rays can hit, leaf of the BVH.
#[derive(Clone, Copy)]
enum Primitive {
    Instance(u32),
    Triangle(u32),
}

struct Solid {
    grid: Arc<Grid>,
    place: Place,
}

/// Nodes of the BVH, a flat array. A leaf has `count > 0` primitives from
/// `start`; an inner node has its children at `start` and `start + 1`.
struct Node {
    low: [f64; 3],
    high: [f64; 3],
    start: u32,
    count: u32,
}

/// Everything in the scene that stops light, in the scene's Y-up frame.
pub(super) struct World {
    instances: Vec<Solid>,
    triangles: Vec<[[f64; 3]; 3]>,
    /// The highest solid half-space, if any.
    plane: Option<f64>,
    nodes: Vec<Node>,
    primitives: Vec<Primitive>,
}

fn bounds_of(points: impl Iterator<Item = [f64; 3]>) -> ([f64; 3], [f64; 3]) {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for point in points {
        for axis in 0..3 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    (low, high)
}

impl World {
    /// The grids are one per source; `instances` says which and where.
    pub(super) fn new(
        instances: Vec<(Arc<Grid>, Place)>,
        occluders: &[Occluder],
    ) -> Result<World, String> {
        let mut triangles = Vec::new();
        let mut plane: Option<f64> = None;
        for occluder in occluders {
            match occluder {
                Occluder::Mesh { path, place } => {
                    let mesh = crate::import::load(path)?;
                    for triangle in &mesh.triangles {
                        triangles.push(triangle.v.map(|v| place.point(mesh.positions[v as usize])));
                    }
                }
                Occluder::Plane { height } => {
                    plane = Some(plane.map_or(*height, |known| known.max(*height)));
                }
                Occluder::Height { origin, cell, rows } => {
                    let at = |x: usize, z: usize| {
                        [
                            origin[0] + cell * x as f64,
                            rows[z][x],
                            origin[1] + cell * z as f64,
                        ]
                    };
                    for z in 0..rows.len() - 1 {
                        for x in 0..rows[0].len() - 1 {
                            triangles.push([at(x, z), at(x + 1, z), at(x + 1, z + 1)]);
                            triangles.push([at(x, z), at(x + 1, z + 1), at(x, z + 1)]);
                        }
                    }
                }
            }
        }
        let instances: Vec<Solid> = instances
            .into_iter()
            .map(|(grid, place)| Solid { grid, place })
            .collect();
        let mut world = World {
            instances,
            triangles,
            plane,
            nodes: Vec::new(),
            primitives: Vec::new(),
        };
        let mut items: Vec<(Primitive, [f64; 3], [f64; 3])> = Vec::new();
        for (index, solid) in world.instances.iter().enumerate() {
            if !solid.grid.is_empty() {
                let (low, high) = solid.bounds();
                items.push((Primitive::Instance(index as u32), low, high));
            }
        }
        for (index, triangle) in world.triangles.iter().enumerate() {
            let (low, high) = bounds_of(triangle.iter().copied());
            items.push((Primitive::Triangle(index as u32), low, high));
        }
        if !items.is_empty() {
            world.nodes.push(Node {
                low: [0.0; 3],
                high: [0.0; 3],
                start: 0,
                count: 0,
            });
            world.split(0, &mut items);
        }
        Ok(world)
    }

    /// Fill node `at` with `items`, median split on the longest axis.
    fn split(&mut self, at: usize, items: &mut [(Primitive, [f64; 3], [f64; 3])]) {
        let (low, _) = bounds_of(items.iter().map(|item| item.1));
        let (_, high) = bounds_of(items.iter().map(|item| item.2));
        self.nodes[at].low = low;
        self.nodes[at].high = high;
        if items.len() <= LEAF {
            self.nodes[at].start = self.primitives.len() as u32;
            self.nodes[at].count = items.len() as u32;
            self.primitives.extend(items.iter().map(|item| item.0));
            return;
        }
        let spread = [0, 1, 2].map(|axis| high[axis] - low[axis]);
        let axis = if spread[0] >= spread[1] && spread[0] >= spread[2] {
            0
        } else if spread[1] >= spread[2] {
            1
        } else {
            2
        };
        let middle = items.len() / 2;
        items.sort_by(|a, b| (a.1[axis] + a.2[axis]).total_cmp(&(b.1[axis] + b.2[axis])));
        let first = self.nodes.len();
        for _ in 0..2 {
            self.nodes.push(Node {
                low: [0.0; 3],
                high: [0.0; 3],
                start: 0,
                count: 0,
            });
        }
        self.nodes[at].start = first as u32;
        let (left, right) = items.split_at_mut(middle);
        self.split(first, left);
        self.split(first + 1, right);
    }

    pub(super) fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    /// Memory of the triangles and the tree, in bytes.
    pub(super) fn tree_bytes(&self) -> usize {
        self.triangles.len() * std::mem::size_of::<[[f64; 3]; 3]>()
            + self.nodes.len() * std::mem::size_of::<Node>()
            + self.primitives.len() * std::mem::size_of::<Primitive>()
    }
}

impl Solid {
    /// World bounds of the grid's box.
    fn bounds(&self) -> ([f64; 3], [f64; 3]) {
        let (low, high) = self.grid.bounds();
        let corners = (0..8).map(|corner| {
            let pick = |axis: usize| {
                if corner >> axis & 1 == 0 {
                    low[axis]
                } else {
                    high[axis]
                }
            };
            self.place.point(y_up([pick(0), pick(1), pick(2)]))
        });
        bounds_of(corners)
    }

    fn hit(&self, origin: [f64; 3], dir: [f64; 3], reach: f64) -> Option<f64> {
        let scale = self.place.scale;
        let local = z_up(self.place.local_point(origin));
        let direction = z_up(self.place.local_vector(dir));
        self.grid
            .hit(local, direction, reach / scale)
            .map(|t| t * scale)
    }
}

/// The ray's entry into a box, if it enters within `reach`.
fn enters(low: [f64; 3], high: [f64; 3], origin: [f64; 3], dir: [f64; 3], reach: f64) -> bool {
    let (mut near, mut far) = (0f64, reach);
    for axis in 0..3 {
        if dir[axis] == 0.0 {
            if origin[axis] < low[axis] || origin[axis] > high[axis] {
                return false;
            }
            continue;
        }
        let a = (low[axis] - origin[axis]) / dir[axis];
        let b = (high[axis] - origin[axis]) / dir[axis];
        near = near.max(a.min(b));
        far = far.min(a.max(b));
    }
    near <= far
}

/// Möller-Trumbore, both sides.
fn triangle_hit(triangle: &[[f64; 3]; 3], origin: [f64; 3], dir: [f64; 3]) -> Option<f64> {
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let (e1, e2) = (sub(triangle[1], triangle[0]), sub(triangle[2], triangle[0]));
    let p = cross(dir, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-14 {
        return None;
    }
    let inv = 1.0 / det;
    let s = sub(origin, triangle[0]);
    let u = dot(s, p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = cross(s, e1);
    let v = dot(dir, q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = dot(e2, q) * inv;
    (t > 0.0).then_some(t)
}

impl Occluders for World {
    fn hit(&self, origin: [f64; 3], dir: [f64; 3], reach: f64) -> Option<f64> {
        let mut best: Option<f64> = None;
        let mut reach = reach;
        if let Some(height) = self.plane {
            if origin[1] < height {
                return Some(0.0);
            }
            if dir[1] < 0.0 {
                let t = (height - origin[1]) / dir[1];
                if t <= reach {
                    best = Some(t);
                    reach = t;
                }
            }
        }
        if self.nodes.is_empty() {
            return best;
        }
        let mut stack = [0u32; 64];
        let mut depth = 1;
        while depth > 0 {
            depth -= 1;
            let node = &self.nodes[stack[depth] as usize];
            if !enters(node.low, node.high, origin, dir, reach) {
                continue;
            }
            if node.count == 0 {
                stack[depth] = node.start;
                stack[depth + 1] = node.start + 1;
                depth += 2;
                continue;
            }
            let (start, count) = (node.start as usize, node.count as usize);
            for primitive in &self.primitives[start..start + count] {
                let found = match *primitive {
                    Primitive::Instance(index) => {
                        self.instances[index as usize].hit(origin, dir, reach)
                    }
                    Primitive::Triangle(index) => {
                        triangle_hit(&self.triangles[index as usize], origin, dir)
                            .filter(|t| *t <= reach)
                    }
                };
                if let Some(t) = found {
                    best = Some(t);
                    reach = t;
                }
            }
        }
        best
    }
}

/// A face of a cell of a placed source, in the scene's frame.
fn placed_patch(place: &Place, face: usize, cell: Cell) -> Patch {
    let patch = bake::cell_patch(face, cell);
    let turned = |v: [f64; 3]| place.rotate(y_up(v));
    Patch {
        corner: place.point(y_up(patch.corner)),
        u: turned(patch.u).map(|v| v * place.scale),
        v: turned(patch.v).map(|v| v * place.scale),
        normal: turned(patch.normal),
    }
}

/// A loaded source: its document, grid, and atlas quads, once however many
/// times it is placed.
struct Source {
    session: Session,
    grid: Arc<Grid>,
    quads: Vec<GreedyQuad>,
    colors: std::collections::HashMap<Cell, u8>,
    classes: Vec<(u8, u8)>,
}

fn load_source(path: &Path, export_hidden: bool) -> Result<Source, String> {
    let mut session = Session::new();
    session.open_vox(path)?;
    if !session.skeleton.bones.is_empty() {
        return Err(format!(
            "{} has bones: a scene bake needs rigid models",
            path.display()
        ));
    }
    let grid = Arc::new(Grid::of_session(&session, export_hidden));
    let (quads, colors, classes) = quads::atlas_quads(&session, export_hidden);
    Ok(Source {
        session,
        grid,
        quads,
        colors,
        classes,
    })
}

/// The grids and the world of a scene, for tests and the bake.
fn world_of(scene: &SceneBake, sources: &[Source], which: &[usize]) -> Result<World, String> {
    let placed = scene
        .instances
        .iter()
        .zip(which)
        .map(|((_, place), source)| (sources[*source].grid.clone(), *place))
        .collect();
    World::new(placed, &scene.occluders)
}

/// The sun of a [`Bake`] in the scene's frame.
fn scene_rays(bake: &Bake) -> Rays {
    Rays::new(bake, y_up(bake.sun_direction.map(f64::from)))
}

/// Bake `scene` into one `.glb` at `output`, and describe it.
pub fn bake_scene(scene: &SceneBake, output: &Path) -> Result<Value, String> {
    if output.extension().is_none_or(|ext| ext != "glb") {
        return Err(format!("path must end in .glb ({})", output.display()));
    }
    let mut sources: Vec<Source> = Vec::new();
    let mut paths: Vec<&PathBuf> = Vec::new();
    let mut which = Vec::with_capacity(scene.instances.len());
    for (path, _) in &scene.instances {
        let at = match paths.iter().position(|known| *known == path) {
            Some(at) => at,
            None => {
                sources.push(load_source(path, scene.export_hidden)?);
                paths.push(path);
                paths.len() - 1
            }
        };
        which.push(at);
    }
    let world = world_of(scene, &sources, &which)?;

    // One atlas block per quad of every instance, in instance order.
    let all_quads: Vec<GreedyQuad> = which
        .iter()
        .flat_map(|source| sources[*source].quads.iter().copied())
        .collect();
    if all_quads.is_empty() {
        return Err("Nothing to export.".into());
    }
    let (width, height, blocks) = quads::pack(&all_quads);
    if width > MAX_ATLAS || height > MAX_ATLAS {
        return Err(format!(
            "the atlas would be {width}x{height}, over {MAX_ATLAS} a side: split the scene"
        ));
    }

    // The light on every texel, in the order the loop below paints them.
    let mut faces: Vec<(usize, usize, Cell)> = Vec::new();
    for (instance, source) in which.iter().enumerate() {
        for (face, cell) in quads::atlas_faces(&sources[*source].quads) {
            faces.push((instance, face, cell));
        }
    }
    let rays = scene_rays(&scene.bake);
    let light = bake::parallel_map(faces.len(), |index| {
        let (instance, face, cell) = faces[index];
        let place = &scene.instances[instance].1;
        let patch = placed_patch(place, face, cell);
        rays.light(&scene.bake, &world, &patch, LIFT * place.scale)
    });

    let any_motion = sources.iter().any(|source| source.session.has_motion());
    let mut built = Built {
        positions: Vec::new(),
        normals: Vec::new(),
        extra: Vec::new(),
        shade: Vec::new(),
        indices: Vec::new(),
        joints: Vec::new(),
        motion: Vec::new(),
        atlas: (width, height, vec![0u8; (width * height * 4) as usize]),
        min: [f32::MAX; 3],
        max: [f32::MIN; 3],
        blend: false,
    };
    let mut first_block = 0;
    let mut next_light = 0;
    let mut spans = Vec::with_capacity(which.len());
    for (instance, source) in which.iter().enumerate() {
        let source = &sources[*source];
        let place = &scene.instances[instance].1;
        let blocks = &blocks[first_block..first_block + source.quads.len()];
        first_block += source.quads.len();
        for (quad, (x0, y0)) in source.quads.iter().zip(blocks) {
            let (normal, u_axis, v_axis) = quads::face_axes(quad.face);
            let origin = quads::split_origin(quad);
            for j in 0..quad.h {
                for i in 0..quad.w {
                    let cell = quads::cell_on(
                        quad.face,
                        axis_of(quad.cell, normal),
                        origin[u_axis] + i,
                        origin[v_axis] + j,
                    );
                    let slot = source.colors.get(&cell).copied().unwrap_or(0);
                    let mut rgba = source.session.palette[slot as usize];
                    built.blend |= rgba[3] != 255;
                    for (channel, value) in rgba[..3].iter_mut().zip(light[next_light]) {
                        *channel = quads::srgb(quads::linear(*channel) * value);
                    }
                    next_light += 1;
                    let at = (((y0 + j as u32) * width + x0 + i as u32) * 4) as usize;
                    built.atlas.2[at..at + 4].copy_from_slice(&rgba);
                }
            }
        }
        let first_index = built.indices.len();
        append_instance(&mut built, source, blocks, place, any_motion);
        let mut span = json!({
            "source": scene.instances[instance].0.display().to_string(),
            "translation": place.translation,
            "yaw": place.yaw,
            "scale": place.scale,
            "firstIndex": first_index,
            "indexCount": built.indices.len() - first_index,
        });
        if let Some(lean) = &place.lean {
            span["lean"] = lean.json();
        }
        if place.sway != 1.0 {
            span["sway"] = json!(place.sway);
        }
        spans.push(span);
    }

    let extras = json!({"instances": spans});
    let bytes = super::encode_scene(&built, &scene.bake, extras)?;
    crate::state::create_parent(output)?;
    std::fs::write(output, &bytes)
        .map_err(|err| format!("Could not write {}: {err}", output.display()))?;
    let grids: usize = sources.iter().map(|source| source.grid.bytes()).sum();
    Ok(json!({
        "path": output.display().to_string(),
        "instances": scene.instances.len(),
        "sources": sources.len(),
        "occluderTriangles": world.triangle_count(),
        "triangles": super::glb_triangles(&bytes)?,
        "vertices": built.positions.len() / 3,
        "atlas": [width, height],
        "texels": faces.len(),
        "bytes": bytes.len(),
        "memory": {
            "grids": grids,
            "occluders": world.tree_bytes(),
            "atlas": width as usize * height as usize * 4,
        },
        "bake": scene.bake.json(),
        "structure": super::gltf_structure(&bytes)?,
    }))
}

/// One instance's faces into the merged mesh, in the scene's frame.
fn append_instance(
    built: &mut Built,
    source: &Source,
    blocks: &[(u32, u32)],
    place: &Place,
    any_motion: bool,
) {
    let mut own = Built {
        positions: Vec::new(),
        normals: Vec::new(),
        extra: Vec::new(),
        shade: Vec::new(),
        indices: Vec::new(),
        joints: Vec::new(),
        motion: Vec::new(),
        atlas: (built.atlas.0, built.atlas.1, Vec::new()),
        min: [f32::MAX; 3],
        max: [f32::MIN; 3],
        blend: false,
    };
    quads::emit_atlas(
        &mut own,
        &source.session,
        &source.quads,
        blocks,
        &source.classes,
        any_motion,
        None,
    );
    let first = (built.positions.len() / 3) as u32;
    for corner in own.positions.chunks_exact(3) {
        let local = [
            f64::from(corner[0]),
            f64::from(corner[1]),
            f64::from(corner[2]),
        ];
        let point = place.point(local).map(|value| value as f32);
        for axis in 0..3 {
            built.min[axis] = built.min[axis].min(point[axis]);
            built.max[axis] = built.max[axis].max(point[axis]);
            built.positions.push(point[axis]);
        }
    }
    for normal in own.normals.chunks_exact(3) {
        let local = [
            f64::from(normal[0]),
            f64::from(normal[1]),
            f64::from(normal[2]),
        ];
        built
            .normals
            .extend(place.rotate(local).map(|value| value as f32));
    }
    built.extra.extend_from_slice(&own.extra);
    if place.sway == 1.0 {
        built.motion.extend_from_slice(&own.motion);
    } else {
        let sway = place.sway as f32;
        for pair in own.motion.chunks_exact(2) {
            built.motion.extend_from_slice(&[pair[0] * sway, pair[1]]);
        }
    }
    built.indices.extend(own.indices.iter().map(|i| i + first));
}

#[cfg(test)]
#[path = "../../tests/bake_scene.rs"]
mod tests;
