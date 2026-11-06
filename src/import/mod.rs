//! Mesh import: a `.glb`, `.gltf`, or `.obj` turned into coloured cells.
//!
//! [`load`] reads the file into one triangle soup in file space: every
//! primitive of every node of the default glTF scene under its node
//! transform, or every face of an OBJ, with the base-colour texture, factor,
//! and vertex colours each triangle needs. [`voxelize`] places the soup in
//! cell space (up axis, rotation, size, anchor), marks every cell a triangle
//! touches (the shell), fills the inside by a majority vote of ray parity
//! along X, Y, and Z when solid, and colours each shell cell at the nearest
//! point of its nearest triangle; inside cells take the colour of the nearest
//! shell cell. [`quantize`] maps those colours into the document palette.

use std::collections::{HashMap, VecDeque};

use glam::{DMat3, DVec3};

use crate::state::Cell;

mod gltf;
mod image;
mod obj;
mod palette;

pub(crate) use palette::{quantize, PaletteFit};

/// Most cells one import writes.
pub const MAX_CELLS: usize = 2_000_000;
/// Most triangles one import reads.
pub const MAX_TRIANGLES: usize = 4_000_000;
/// The longest axis of an import, in cells.
pub const MAX_SIDE: usize = 256;
/// A face that lands on a cell boundary (each face of a cube at `size`
/// cells) is moved half of this against its normal, into the cell behind
/// it, and a cell is tested a quarter of it smaller, so an edge that only
/// touches a cell's side does not take it.
const INSET: f64 = 1e-3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Wrap {
    Repeat,
    Clamp,
    Mirror,
}

/// A decoded texture, sRGB RGBA rows from the top.
#[derive(Clone, Debug)]
pub(crate) struct Texture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<[u8; 4]>,
    pub wrap: [Wrap; 2],
}

impl Texture {
    /// Bilinear sample at a glTF texture coordinate (origin at the top
    /// left), in linear RGBA.
    fn sample(&self, uv: [f64; 2]) -> [f64; 4] {
        let x = uv[0] * f64::from(self.width) - 0.5;
        let y = uv[1] * f64::from(self.height) - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let mut out = [0.0; 4];
        for (dx, dy, weight) in [
            (0, 0, (1.0 - fx) * (1.0 - fy)),
            (1, 0, fx * (1.0 - fy)),
            (0, 1, (1.0 - fx) * fy),
            (1, 1, fx * fy),
        ] {
            let px = wrap(x0 as i64 + dx, self.width, self.wrap[0]);
            let py = wrap(y0 as i64 + dy, self.height, self.wrap[1]);
            let texel = self.rgba[py * self.width as usize + px];
            for c in 0..3 {
                out[c] += weight * to_linear(texel[c]);
            }
            out[3] += weight * f64::from(texel[3]) / 255.0;
        }
        out
    }
}

fn wrap(i: i64, size: u32, mode: Wrap) -> usize {
    let n = i64::from(size);
    let at = match mode {
        Wrap::Clamp => i.clamp(0, n - 1),
        Wrap::Repeat => i.rem_euclid(n),
        Wrap::Mirror => {
            let period = i.rem_euclid(2 * n);
            if period < n {
                period
            } else {
                2 * n - 1 - period
            }
        }
    };
    at as usize
}

/// A material's base colour: a linear factor and an optional texture.
#[derive(Clone, Debug)]
pub(crate) struct Material {
    pub color: [f64; 4],
    /// Index into [`Mesh::textures`].
    pub texture: Option<usize>,
    /// `KHR_texture_transform` as `u' = a u + b v + e`, `v' = c u + d v + f`.
    pub transform: Option<[f64; 6]>,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            color: [1.0; 4],
            texture: None,
            transform: None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Triangle {
    pub v: [u32; 3],
    /// Index into [`Mesh::materials`].
    pub material: u32,
}

/// Triangles in file space. `uvs` and `colors` have one entry per position;
/// a vertex without them has `[0, 0]` and white.
#[derive(Clone, Debug, Default)]
pub(crate) struct Mesh {
    pub positions: Vec<[f64; 3]>,
    pub uvs: Vec<[f64; 2]>,
    /// Linear RGBA.
    pub colors: Vec<[f64; 4]>,
    pub triangles: Vec<Triangle>,
    pub materials: Vec<Material>,
    pub textures: Vec<Texture>,
    /// glTF primitives, or OBJ material groups, that gave triangles.
    pub primitives: usize,
    /// What the file asked for that import skips, such as a WebP texture.
    pub warnings: Vec<String>,
}

impl Mesh {
    /// Append a vertex.
    pub(crate) fn push_vertex(&mut self, position: [f64; 3], uv: [f64; 2], color: [f64; 4]) -> u32 {
        self.positions.push(position);
        self.uvs.push(uv);
        self.colors.push(color);
        (self.positions.len() - 1) as u32
    }

    pub(crate) fn push_triangle(&mut self, v: [u32; 3], material: u32) -> Result<(), String> {
        if self.triangles.len() >= MAX_TRIANGLES {
            return Err(format!(
                "the mesh has more than {MAX_TRIANGLES} triangles; decimate it first"
            ));
        }
        if v[0] != v[1] && v[1] != v[2] && v[0] != v[2] {
            self.triangles.push(Triangle { v, material });
        }
        Ok(())
    }

    /// The linear colour an unlit material draws: nearest texel times the
    /// base-colour factor and the vertex colours.
    pub(crate) fn unlit_color(&self, triangle: &Triangle, bary: [f64; 3]) -> [f64; 4] {
        let mut out = [0.0; 4];
        for c in 0..4 {
            out[c] = (0..3)
                .map(|k| bary[k] * self.colors[triangle.v[k] as usize][c])
                .sum::<f64>();
        }
        let Some(material) = self.materials.get(triangle.material as usize) else {
            return out;
        };
        for c in 0..4 {
            out[c] *= material.color[c];
        }
        if let Some(texture) = material.texture.and_then(|t| self.textures.get(t)) {
            let mut uv = [0.0; 2];
            for k in 0..3 {
                let at = self.uvs[triangle.v[k] as usize];
                uv[0] += bary[k] * at[0];
                uv[1] += bary[k] * at[1];
            }
            let x = (uv[0] * f64::from(texture.width)).floor() as i64;
            let y = (uv[1] * f64::from(texture.height)).floor() as i64;
            let px = wrap(x, texture.width, texture.wrap[0]);
            let py = wrap(y, texture.height, texture.wrap[1]);
            let texel = texture.rgba[py * texture.width as usize + px];
            for c in 0..3 {
                out[c] *= to_linear(texel[c]);
            }
            out[3] *= f64::from(texel[3]) / 255.0;
        }
        out
    }

    fn color_at(&self, triangle: &Triangle, bary: [f64; 3], colors: Colors) -> [f64; 4] {
        let mut out = [1.0; 4];
        let material = self.materials.get(triangle.material as usize);
        for c in 0..4 {
            out[c] = (0..3)
                .map(|k| bary[k] * self.colors[triangle.v[k] as usize][c])
                .sum::<f64>();
        }
        if colors == Colors::Vertex {
            return out;
        }
        let Some(material) = material else {
            return out;
        };
        for c in 0..4 {
            out[c] *= material.color[c];
        }
        if let Some(texture) = material.texture.and_then(|t| self.textures.get(t)) {
            let mut uv = [0.0; 2];
            for k in 0..3 {
                let at = self.uvs[triangle.v[k] as usize];
                uv[0] += bary[k] * at[0];
                uv[1] += bary[k] * at[1];
            }
            if let Some([a, b, c, d, e, f]) = material.transform {
                uv = [a * uv[0] + b * uv[1] + e, c * uv[0] + d * uv[1] + f];
            }
            let texel = texture.sample(uv);
            for c in 0..4 {
                out[c] *= texel[c];
            }
        }
        out
    }
}

/// Read a `.glb`, `.gltf`, or `.obj` (with its `.mtl` and textures).
pub(crate) fn load(path: &std::path::Path) -> Result<Mesh, String> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);
    let mesh = match extension.as_deref() {
        Some("glb" | "gltf") => gltf::load(path)?,
        Some("obj") => obj::load(path)?,
        _ => {
            return Err(format!(
                "{} is not a .glb, .gltf, or .obj mesh",
                path.display()
            ))
        }
    };
    if mesh.triangles.is_empty() {
        return Err(format!("{} has no triangles", path.display()));
    }
    Ok(mesh)
}

/// Which way is up in the file. glTF and most OBJ exports are Y-up; cells
/// are Z-up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Up {
    Y,
    Z,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Fit {
    /// Cells along the longest axis.
    Cells(f64),
    /// Cells per file unit (1 / voxel size in metres for a glTF).
    Scale(f64),
}

/// Which point of the placed mesh's box lands on `position`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Anchor {
    /// The low corner.
    Min,
    /// The centre.
    Center,
    /// The centre of the bottom face.
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Colors {
    /// Base-colour texture times factor times vertex colours.
    Auto,
    /// Vertex colours only.
    Vertex,
    /// No colour; the caller paints one slot.
    None,
}

#[derive(Clone, Debug)]
pub(crate) struct Voxelize {
    pub fit: Fit,
    pub up: Up,
    /// Degrees about X, then Y, then Z (cell axes), after the up axis.
    pub rotation: [f64; 3],
    pub anchor: Anchor,
    /// Cell coordinates the anchor lands on (a cell corner).
    pub position: [f64; 3],
    pub solid: bool,
    pub colors: Colors,
}

pub(crate) struct Voxels {
    /// Every cell with its sRGB colour, sorted.
    pub cells: Vec<(Cell, [u8; 3])>,
    pub shell: usize,
    /// Cells per file unit.
    pub scale: f64,
    pub min: Cell,
    pub max: Cell,
}

struct Hit {
    dist2: f64,
    triangle: u32,
    bary: [f64; 3],
}

/// Place `mesh` in cell space and fill its cells. Refused before anything is
/// written when it would be more than [`MAX_SIDE`] cells on an axis or more
/// than [`MAX_CELLS`] cells.
pub(crate) fn voxelize(mesh: &Mesh, options: &Voxelize) -> Result<Voxels, String> {
    let [rx, ry, rz] = options.rotation.map(f64::to_radians);
    let rotate =
        DMat3::from_rotation_z(rz) * DMat3::from_rotation_y(ry) * DMat3::from_rotation_x(rx);
    let placed: Vec<DVec3> = mesh
        .positions
        .iter()
        .map(|&[x, y, z]| {
            let p = match options.up {
                Up::Y => DVec3::new(x, -z, y),
                Up::Z => DVec3::new(x, y, z),
            };
            rotate * p
        })
        .collect();
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for triangle in &mesh.triangles {
        for &v in &triangle.v {
            let p = placed[v as usize];
            if !p.is_finite() {
                return Err("the mesh has a vertex that is not a finite number".into());
            }
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    let extent = hi - lo;
    let longest = extent.max_element();
    if !(longest > 0.0) {
        return Err("the mesh has no extent".into());
    }
    let nominal = match options.fit {
        Fit::Cells(cells) => {
            if !(1.0..=MAX_SIDE as f64).contains(&cells) {
                return Err(format!("size must be 1..={MAX_SIDE} cells"));
            }
            cells / longest
        }
        Fit::Scale(scale) => {
            if !(scale > 0.0 && scale.is_finite()) {
                return Err("scale must be a positive number".into());
            }
            scale
        }
    };
    let scale = nominal;
    let span = extent * scale;
    let position = DVec3::from(options.position);
    // A flat axis sits a little above the cell boundary at its anchor.
    let flat = DVec3::select(
        span.cmplt(DVec3::splat(INSET)),
        DVec3::splat(INSET),
        DVec3::ZERO,
    );
    let base = flat
        + match options.anchor {
            Anchor::Min => position,
            Anchor::Center => position - span / 2.0,
            Anchor::Bottom => DVec3::new(
                position.x - span.x / 2.0,
                position.y - span.y / 2.0,
                position.z,
            ),
        };
    let cell_of =
        |p: DVec3| -> Cell { (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32) };
    let low = cell_of(base);
    let high = cell_of((base + span - DVec3::splat(INSET)).max(base));
    let dims = [
        (high.0 - low.0 + 1) as usize,
        (high.1 - low.1 + 1) as usize,
        (high.2 - low.2 + 1) as usize,
    ];
    if dims.iter().any(|&d| d > MAX_SIDE) {
        return Err(format!(
            "the mesh would be {}x{}x{} cells; an object is at most {MAX_SIDE} per axis, so lower size or scale",
            dims[0], dims[1], dims[2]
        ));
    }
    let at: Vec<DVec3> = placed.iter().map(|p| (*p - lo) * scale + base).collect();

    // Faces wound inward (a negative volume) move the other way.
    let volume: f64 = mesh
        .triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.v.map(|v| at[v as usize] - base);
            a.dot(b.cross(c))
        })
        .sum();
    let behind = if volume < 0.0 {
        -INSET / 2.0
    } else {
        INSET / 2.0
    };

    // The shell: every cell a triangle overlaps, with its nearest triangle.
    let mut shell: HashMap<Cell, Hit> = HashMap::new();
    for (index, triangle) in mesh.triangles.iter().enumerate() {
        let [a, b, c] = triangle.v.map(|v| at[v as usize]);
        // A face that lands on a cell boundary inside the mesh (two parts
        // meeting at a whole cell) goes to the cell behind it.
        let inward = -(b - a).cross(c - a).normalize_or_zero() * behind;
        let [a, b, c] = [a + inward, b + inward, c + inward];
        let min = cell_of(a.min(b).min(c));
        let max = cell_of(a.max(b).max(c));
        for x in min.0..=max.0 {
            for y in min.1..=max.1 {
                for z in min.2..=max.2 {
                    let centre =
                        DVec3::new(f64::from(x) + 0.5, f64::from(y) + 0.5, f64::from(z) + 0.5);
                    if !triangle_box(a - centre, b - centre, c - centre, 0.5 - INSET / 4.0) {
                        continue;
                    }
                    let (point, bary) = closest_point(centre, a, b, c);
                    let dist2 = (point - centre).length_squared();
                    let hit = Hit {
                        dist2,
                        triangle: index as u32,
                        bary,
                    };
                    match shell.entry((x, y, z)) {
                        std::collections::hash_map::Entry::Occupied(mut old) => {
                            if dist2 < old.get().dist2 {
                                old.insert(hit);
                            }
                        }
                        std::collections::hash_map::Entry::Vacant(slot) => {
                            slot.insert(hit);
                        }
                    }
                }
            }
        }
        if shell.len() > MAX_CELLS {
            return Err(format!(
                "the mesh would fill more than {MAX_CELLS} cells; lower size or scale"
            ));
        }
    }

    let inside = if options.solid {
        inside_cells(mesh, &at, low, dims, &shell)
    } else {
        Vec::new()
    };
    if shell.len() + inside.len() > MAX_CELLS {
        return Err(format!(
            "the mesh would fill {} cells; at most {MAX_CELLS}, so lower size or scale or pass fill shell",
            shell.len() + inside.len()
        ));
    }

    let mut colors: HashMap<Cell, [u8; 3]> = shell
        .iter()
        .map(|(&cell, hit)| {
            let rgb = if options.colors == Colors::None {
                [255; 3]
            } else {
                let triangle = &mesh.triangles[hit.triangle as usize];
                let linear = mesh.color_at(triangle, hit.bary, options.colors);
                [srgb(linear[0]), srgb(linear[1]), srgb(linear[2])]
            };
            (cell, rgb)
        })
        .collect();
    let shell_count = colors.len();
    if !inside.is_empty() {
        // Inside cells take the colour of the nearest shell cell.
        let pending: std::collections::HashSet<Cell> = inside.iter().copied().collect();
        let mut queue: VecDeque<Cell> = shell.keys().copied().collect();
        while let Some(cell) = queue.pop_front() {
            let rgb = colors[&cell];
            for (dx, dy, dz) in [
                (1, 0, 0),
                (-1, 0, 0),
                (0, 1, 0),
                (0, -1, 0),
                (0, 0, 1),
                (0, 0, -1),
            ] {
                let next = (cell.0 + dx, cell.1 + dy, cell.2 + dz);
                if pending.contains(&next) && !colors.contains_key(&next) {
                    colors.insert(next, rgb);
                    queue.push_back(next);
                }
            }
        }
        let fallback = colors.values().next().copied().unwrap_or([255; 3]);
        for cell in inside {
            colors.entry(cell).or_insert(fallback);
        }
    }
    let mut cells: Vec<(Cell, [u8; 3])> = colors.into_iter().collect();
    cells.sort_unstable_by_key(|(cell, _)| *cell);
    let min = cells
        .iter()
        .fold((i32::MAX, i32::MAX, i32::MAX), |m, (c, _)| {
            (m.0.min(c.0), m.1.min(c.1), m.2.min(c.2))
        });
    let max = cells
        .iter()
        .fold((i32::MIN, i32::MIN, i32::MIN), |m, (c, _)| {
            (m.0.max(c.0), m.1.max(c.1), m.2.max(c.2))
        });
    Ok(Voxels {
        cells,
        shell: shell_count,
        scale,
        min,
        max,
    })
}

/// Cells inside the mesh and not on its shell: a cell is inside when at
/// least two of the rays through its centre along X, Y, and Z have crossed
/// the surface an odd number of times. The vote keeps a small hole in an
/// open mesh from emptying a whole row.
fn inside_cells(
    mesh: &Mesh,
    at: &[DVec3],
    low: Cell,
    dims: [usize; 3],
    shell: &HashMap<Cell, Hit>,
) -> Vec<Cell> {
    let low = [low.0, low.1, low.2];
    let mut votes = vec![0u8; dims[0] * dims[1] * dims[2]];
    let index = |p: [usize; 3]| (p[2] * dims[1] + p[1]) * dims[0] + p[0];
    for axis in 0..3 {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let mut rays: HashMap<(i32, i32), Vec<f64>> = HashMap::new();
        for triangle in &mesh.triangles {
            let mut p = triangle.v.map(|i| at[i as usize]);
            let flat = |q: DVec3| [q[u], q[v]];
            let mut area = edge(flat(p[0]), flat(p[1]), flat(p[2]));
            if area.abs() < 1e-12 {
                continue;
            }
            if area < 0.0 {
                p.swap(1, 2);
                area = -area;
            }
            let q = p.map(flat);
            let (u0, u1) = (
                q[0][0].min(q[1][0]).min(q[2][0]),
                q[0][0].max(q[1][0]).max(q[2][0]),
            );
            let (v0, v1) = (
                q[0][1].min(q[1][1]).min(q[2][1]),
                q[0][1].max(q[1][1]).max(q[2][1]),
            );
            for j in (u0 - 0.5).ceil() as i32..=(u1 - 0.5).floor() as i32 {
                for k in (v0 - 0.5).ceil() as i32..=(v1 - 0.5).floor() as i32 {
                    let point = [f64::from(j) + 0.5, f64::from(k) + 0.5];
                    let w = [
                        edge(q[1], q[2], point),
                        edge(q[2], q[0], point),
                        edge(q[0], q[1], point),
                    ];
                    let owns = |weight: f64, from: [f64; 2], to: [f64; 2]| {
                        weight > 0.0 || (weight == 0.0 && top_left(from, to))
                    };
                    if !(owns(w[0], q[1], q[2]) && owns(w[1], q[2], q[0]) && owns(w[2], q[0], q[1]))
                    {
                        continue;
                    }
                    let depth = (w[0] * p[0][axis] + w[1] * p[1][axis] + w[2] * p[2][axis]) / area;
                    rays.entry((j, k)).or_default().push(depth);
                }
            }
        }
        for ((j, k), mut crossings) in rays {
            let (ju, kv) = (j - low[u], k - low[v]);
            if ju < 0 || kv < 0 || ju as usize >= dims[u] || kv as usize >= dims[v] {
                continue;
            }
            crossings.sort_by(f64::total_cmp);
            for pair in crossings.chunks_exact(2) {
                let first = ((pair[0] - 0.5).ceil() as i32 - low[axis]).max(0);
                let last = ((pair[1] - 0.5).floor() as i32 - low[axis]).min(dims[axis] as i32 - 1);
                for i in first..=last {
                    let mut p = [0usize; 3];
                    p[axis] = i as usize;
                    p[u] = ju as usize;
                    p[v] = kv as usize;
                    votes[index(p)] += 1;
                }
            }
        }
    }
    let mut inside = Vec::new();
    for z in 0..dims[2] {
        for y in 0..dims[1] {
            for x in 0..dims[0] {
                if votes[index([x, y, z])] >= 2 {
                    let cell = (x as i32 + low[0], y as i32 + low[1], z as i32 + low[2]);
                    if !shell.contains_key(&cell) {
                        inside.push(cell);
                    }
                }
            }
        }
    }
    inside
}

fn edge(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

/// The fill rule for a point exactly on an edge: of two triangles sharing
/// the edge, only one counts it, so a ray through it crosses once.
fn top_left(from: [f64; 2], to: [f64; 2]) -> bool {
    let (du, dv) = (to[0] - from[0], to[1] - from[1]);
    dv < 0.0 || (dv == 0.0 && du > 0.0)
}

/// Whether the triangle (relative to the box centre) overlaps the cube of
/// half size `h`: the separating axis test of Akenine-Möller.
fn triangle_box(v0: DVec3, v1: DVec3, v2: DVec3, h: f64) -> bool {
    let edges = [v1 - v0, v2 - v1, v0 - v2];
    let verts = [v0, v1, v2];
    for e in edges {
        for axis in [DVec3::X, DVec3::Y, DVec3::Z] {
            let a = axis.cross(e);
            if a.length_squared() < 1e-24 {
                continue;
            }
            let p = verts.map(|v| v.dot(a));
            let r = h * (a.x.abs() + a.y.abs() + a.z.abs());
            let (lo, hi) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
            if lo > r || hi < -r {
                return false;
            }
        }
    }
    for k in 0..3 {
        let lo = v0[k].min(v1[k]).min(v2[k]);
        let hi = v0[k].max(v1[k]).max(v2[k]);
        if lo > h || hi < -h {
            return false;
        }
    }
    let normal = edges[0].cross(edges[1]);
    let r = h * (normal.x.abs() + normal.y.abs() + normal.z.abs());
    normal.dot(v0).abs() <= r
}

/// The point of triangle `abc` nearest `p` and its barycentric weights
/// (Ericson, Real-Time Collision Detection 5.1.5).
fn closest_point(p: DVec3, a: DVec3, b: DVec3, c: DVec3) -> (DVec3, [f64; 3]) {
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return (a, [1.0, 0.0, 0.0]);
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return (b, [0.0, 1.0, 0.0]);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return (a + ab * v, [1.0 - v, v, 0.0]);
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return (c, [0.0, 0.0, 1.0]);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return (a + ac * w, [1.0 - w, 0.0, w]);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return (b + (c - b) * w, [0.0, 1.0 - w, w]);
    }
    let denom = va + vb + vc;
    if denom.abs() < 1e-300 {
        return (a, [1.0, 0.0, 0.0]);
    }
    let v = vb / denom;
    let w = vc / denom;
    (a + ab * v + ac * w, [1.0 - v - w, v, w])
}

pub(crate) fn to_linear(byte: u8) -> f64 {
    let c = f64::from(byte) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub(crate) fn srgb(value: f64) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let s = if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

#[cfg(test)]
#[path = "../../tests/import.rs"]
mod tests;
