//! MCP `project_paint`: recolour the visible surface of an object from one
//! image projected along a view direction.
//!
//! The image is resampled to a grid of cells on the view plane, as one view
//! of `carve_from_views` is, and each inked grid cell casts parallel rays
//! (one along an axis view, a 3 by 3 fan of sub-rays otherwise) into the
//! object. A ray paints the solid cells within `depth` cells of the first
//! solid cell it meets, so a pattern lands on the surface the view sees and
//! never on what is behind it. Cells are never added or removed.

use std::collections::HashMap;

use super::silhouette::{quantize, Grid};
use super::{Colour, Ink, Mask, Scale, Side};
use crate::state::Cell;

/// Most cells of the grid the image is resampled to.
pub const MAX_PAINT_GRID: i64 = 4_000_000;
/// Most cells a ray paints below the surface.
pub const MAX_PAINT_DEPTH: u32 = 64;

/// The direction a projection looks from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Toward {
    /// One of the six views of `carve_from_views` and `model_view`.
    Side(Side),
    /// Degrees, as MCP `preview` takes them: yaw 0 looks from +X, yaw −90
    /// from −Y (front), pitch 90 from above.
    Angles { yaw: f32, pitch: f32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaintSpec {
    pub mask: Mask,
    pub toward: Toward,
    pub scale: Scale,
    /// The box's minimum corner; defaults to the object's.
    pub origin: Option<Cell>,
    /// The box in cells; an axis left out reaches the object's far side.
    pub size: [Option<u32>; 3],
    pub colour: Colour,
    /// Share of a grid cell's footprint that must be inked, 0..1.
    pub coverage: f32,
    /// Solid cells painted per ray, from the first one it meets, 1..=64.
    pub depth: u32,
    /// Degrees: a surface whose normal is further than this from the view
    /// direction is left alone.
    pub normal_limit: Option<f32>,
    /// Slots that may be repainted; other cells keep theirs (and still hide
    /// what is behind them).
    pub only: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Painted {
    /// Every painted cell with its new slot, changed or not.
    pub cells: Vec<(Cell, u8)>,
    pub origin: Cell,
    pub size: [u32; 3],
    /// The image's grid in cells, across and down.
    pub grid: [u32; 2],
    /// Grid cells the image inks.
    pub covered: usize,
    /// Inked rays that met the object.
    pub rays: usize,
    /// Inked rays skipped because the surface they met was too steep.
    pub steep: usize,
    pub bounds: Option<(Cell, Cell)>,
}

type V3 = [f64; 3];

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(a: V3) -> V3 {
    let len = dot(a, a).sqrt();
    if len <= 0.0 {
        a
    } else {
        a.map(|c| c / len)
    }
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn axis_vector(axis: usize, sign: i32) -> V3 {
    let mut v = [0.0; 3];
    v[axis] = f64::from(sign);
    v
}

/// Image right, image down, and the direction toward the camera, with the
/// side when the direction is one of the six views.
fn basis(toward: Toward) -> (V3, V3, V3, Option<Side>) {
    let side = match toward {
        Toward::Side(side) => Some(side),
        Toward::Angles { yaw, pitch } => {
            let (yaw, pitch) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
            let eye = [
                yaw.cos() * pitch.cos(),
                yaw.sin() * pitch.cos(),
                pitch.sin(),
            ];
            let snapped = [
                Side::Front,
                Side::Back,
                Side::Left,
                Side::Right,
                Side::Top,
                Side::Bottom,
            ]
            .into_iter()
            .find(|side| {
                let (axis, sign) = side.axes()[2];
                dot(eye, axis_vector(axis, sign)) > 1.0 - 1e-9
            });
            if snapped.is_none() {
                // The basis of MCP `preview`, so an image matches its view.
                let forward = eye.map(|c| -c);
                let up = if forward[2].abs() > 0.95 {
                    [0.0, 1.0, 0.0]
                } else {
                    [0.0, 0.0, 1.0]
                };
                let right = normalize(cross(forward, up));
                let up = cross(right, forward);
                return (right, up.map(|c| -c), eye, None);
            }
            snapped
        }
    };
    let side = side.expect("a side or an oblique basis");
    let [(u, su), (v, sv), (d, sd)] = side.axes();
    (
        axis_vector(u, su),
        axis_vector(v, sv),
        axis_vector(d, sd),
        Some(side),
    )
}

/// Inclusive bounds of a set of cells.
fn cell_bounds(cells: &HashMap<Cell, u8>) -> Option<([i32; 3], [i32; 3])> {
    let mut found: Option<([i32; 3], [i32; 3])> = None;
    for &(x, y, z) in cells.keys() {
        let p = [x, y, z];
        found = Some(match found {
            None => (p, p),
            Some((lo, hi)) => (
                std::array::from_fn(|a| lo[a].min(p[a])),
                std::array::from_fn(|a| hi[a].max(p[a])),
            ),
        });
    }
    found
}

/// The outward normal of the surface at a cell: the mean direction of the
/// empty cells within two cells of it, or `fallback` when they cancel.
fn surface_normal(cells: &HashMap<Cell, u8>, cell: Cell, fallback: V3) -> V3 {
    let mut sum = [0.0f64; 3];
    for dz in -2i32..=2 {
        for dy in -2i32..=2 {
            for dx in -2i32..=2 {
                let r2 = dx * dx + dy * dy + dz * dz;
                if r2 == 0 || r2 > 6 {
                    continue;
                }
                if cells.contains_key(&(cell.0 + dx, cell.1 + dy, cell.2 + dz)) {
                    continue;
                }
                let len = f64::from(r2).sqrt();
                sum[0] += f64::from(dx) / len;
                sum[1] += f64::from(dy) / len;
                sum[2] += f64::from(dz) / len;
            }
        }
    }
    if dot(sum, sum) < 0.25 {
        fallback
    } else {
        normalize(sum)
    }
}

/// Paint `spec`'s image onto `cells`. `palette` quantises image colours.
pub fn project_paint(
    cells: &HashMap<Cell, u8>,
    spec: &PaintSpec,
    palette: &[[u8; 3]; 256],
) -> Result<Painted, String> {
    if !(0.0..=1.0).contains(&spec.coverage) {
        return Err("coverage must be 0..1".into());
    }
    if spec.depth == 0 || spec.depth > MAX_PAINT_DEPTH {
        return Err(format!("depth must be 1..={MAX_PAINT_DEPTH}"));
    }
    if let Some(limit) = spec.normal_limit {
        if !(0.0..=90.0).contains(&limit) {
            return Err("normalLimit must be 0..90 degrees".into());
        }
    }
    if let Toward::Angles { yaw, pitch } = spec.toward {
        if !yaw.is_finite() || !(-90.0..=90.0).contains(&pitch) {
            return Err("yaw must be finite and pitch -90..90".into());
        }
    }
    match &spec.colour {
        Colour::Views { slots } => {
            if slots.is_empty() || slots.contains(&0) {
                return Err("slots must be palette indices 1..255".into());
            }
            if spec.mask.pixels.contains(&Some(Ink::Plain)) {
                return Err(
                    "a rows character has no legend entry: add it to legend or pass palette".into(),
                );
            }
        }
        Colour::Slot(0) => return Err("palette must be 1..255".into()),
        Colour::Slot(_) => {}
    }
    if let Some(only) = &spec.only {
        if only.is_empty() || only.contains(&0) {
            return Err("only must be palette indices 1..255".into());
        }
    }
    let (lo, hi) = cell_bounds(cells).ok_or("the active object has no cells to paint")?;
    let origin = match spec.origin {
        Some(cell) => [cell.0, cell.1, cell.2],
        None => lo,
    };
    let mut size = [0u32; 3];
    for axis in 0..3 {
        size[axis] = match spec.size[axis] {
            Some(given) => given,
            None => (i64::from(hi[axis]) - i64::from(origin[axis]) + 1).max(0) as u32,
        };
        if size[axis] == 0 || size[axis] > 2048 {
            return Err(format!(
                "the box along {} must be 1..2048 cells: pass origin and size inside the object",
                ["x", "y", "z"][axis]
            ));
        }
    }
    let (right, down, eye, side) = basis(spec.toward);
    // The box's projection onto the view plane.
    let (mut u0, mut u1, mut v0, mut v1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for corner in 0..8 {
        let p: V3 = std::array::from_fn(|a| {
            f64::from(origin[a])
                + if corner >> a & 1 == 1 {
                    f64::from(size[a])
                } else {
                    0.0
                }
        });
        let (u, v) = (dot(p, right), dot(p, down));
        u0 = u0.min(u);
        u1 = u1.max(u);
        v0 = v0.min(v);
        v1 = v1.max(v);
    }
    let (across, tall) = (u1 - u0, v1 - v0);
    let [cols, rows] = match spec.scale {
        Scale::CellsPerPixel(c) => {
            if !(c > 0.0 && c <= 64.0) {
                return Err("cellsPerPixel must be above 0, at most 64".into());
            }
            let n = |pixels: u32| ((pixels as f32 * c).round() as u32).max(1);
            [n(spec.mask.width), n(spec.mask.height)]
        }
        Scale::Size([w, h]) => {
            if w == 0 || h == 0 || w > 2048 || h > 2048 {
                return Err("imageSize must be 1..2048 cells a side".into());
            }
            [w, h]
        }
        Scale::Fit => [(across.round() as u32).max(1), (tall.round() as u32).max(1)],
    };
    if i64::from(cols) * i64::from(rows) > MAX_PAINT_GRID {
        return Err(format!(
            "the image grid is {cols}x{rows} cells, over {MAX_PAINT_GRID}"
        ));
    }
    // Where the grid starts on the plane. An axis view places it as
    // `carve_from_views` places a view in its box: centred across, standing
    // on the bottom; an oblique one is centred both ways.
    let (u_start, v_start) = match side {
        Some(side) => {
            let [(ua, su), (va, sv), _] = side.axes();
            let place = |axis: usize, sign: i32, low: f64, n: u32| -> f64 {
                let box_n = i64::from(size[axis]);
                let offset = if axis == 2 {
                    0
                } else {
                    (box_n - i64::from(n)).div_euclid(2)
                };
                // `offset` counts from the box's low corner along the axis;
                // a mirrored axis counts the grid from its far side.
                let from_low = if sign > 0 {
                    offset
                } else {
                    box_n - i64::from(n) - offset
                };
                low + from_low as f64
            };
            (place(ua, su, u0, cols), place(va, sv, v0, rows))
        }
        None => (
            u0 + (across - f64::from(cols)) / 2.0,
            v0 + (tall - f64::from(rows)) / 2.0,
        ),
    };
    let grid = Grid::build(&spec.mask, cols, rows, spec.coverage);
    let mut lookup: HashMap<[u8; 3], u8> = HashMap::new();
    let slot_of = |ink: Ink, lookup: &mut HashMap<[u8; 3], u8>| -> u8 {
        match (&spec.colour, ink) {
            (Colour::Slot(slot), _) => *slot,
            (Colour::Views { .. }, Ink::Slot(slot)) => slot,
            (Colour::Views { slots }, Ink::Rgb(color)) => *lookup
                .entry(color)
                .or_insert_with(|| quantize(color, palette, slots)),
            (Colour::Views { slots }, Ink::Plain) => slots[0],
        }
    };
    let in_box = |c: Cell| {
        let p = [c.0, c.1, c.2];
        (0..3).all(|a| {
            i64::from(p[a]) >= i64::from(origin[a])
                && i64::from(p[a]) < i64::from(origin[a]) + i64::from(size[a])
        })
    };
    let may_paint = |slot: u8| spec.only.as_ref().is_none_or(|only| only.contains(&slot));
    // Rays run through the bounds of every cell, the box's or not, so a
    // cell outside the box still hides what is behind it.
    let world_lo: V3 = lo.map(f64::from);
    let world_hi: V3 = hi.map(|c| f64::from(c) + 1.0);
    let mut far = f64::MIN;
    for corner in 0..8 {
        let p: V3 = std::array::from_fn(|a| {
            if corner >> a & 1 == 1 {
                world_hi[a]
            } else {
                world_lo[a]
            }
        });
        far = far.max(dot(p, eye));
    }
    let dir: V3 = eye.map(|c| -c);
    let cos_limit = spec
        .normal_limit
        .map(|deg| f64::from(deg).to_radians().cos());
    let fan: &[f64] = if side.is_some() {
        &[0.5]
    } else {
        &[1.0 / 6.0, 0.5, 5.0 / 6.0]
    };
    let mut votes: HashMap<Cell, Vec<(u8, u32)>> = HashMap::new();
    let mut normals: HashMap<Cell, bool> = HashMap::new();
    let (mut rays, mut steep) = (0usize, 0usize);
    for row in 0..rows {
        for col in 0..cols {
            let index = (row * cols + col) as usize;
            if !grid.inside[index] {
                continue;
            }
            let Some(ink) = grid.ink[index] else {
                continue;
            };
            let slot = slot_of(ink, &mut lookup);
            for &fv in fan {
                for &fu in fan {
                    let u = u_start + f64::from(col) + fu;
                    let v = v_start + f64::from(row) + fv;
                    let start: V3 =
                        std::array::from_fn(|a| u * right[a] + v * down[a] + (far + 1.0) * eye[a]);
                    let Some(hits) = march(cells, start, dir, world_lo, world_hi, spec.depth)
                    else {
                        continue;
                    };
                    rays += 1;
                    let (first, face) = hits[0];
                    if let Some(cos_limit) = cos_limit {
                        let ok = *normals.entry(first).or_insert_with(|| {
                            dot(surface_normal(cells, first, face), eye) >= cos_limit - 1e-9
                        });
                        if !ok {
                            steep += 1;
                            continue;
                        }
                    }
                    for (cell, _) in hits {
                        if !in_box(cell) || !may_paint(cells[&cell]) {
                            continue;
                        }
                        let list = votes.entry(cell).or_default();
                        match list.iter_mut().find(|(s, _)| *s == slot) {
                            Some(found) => found.1 += 1,
                            None => list.push((slot, 1)),
                        }
                    }
                }
            }
        }
    }
    let mut painted: Vec<(Cell, u8)> = votes
        .into_iter()
        .map(|(cell, list)| {
            let slot = list
                .into_iter()
                .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
                .map(|(slot, _)| slot)
                .unwrap_or(1);
            (cell, slot)
        })
        .collect();
    painted.sort_unstable();
    let mut bounds: Option<(Cell, Cell)> = None;
    for &(cell, _) in &painted {
        bounds = Some(match bounds {
            None => (cell, cell),
            Some((a, b)) => (
                (a.0.min(cell.0), a.1.min(cell.1), a.2.min(cell.2)),
                (b.0.max(cell.0), b.1.max(cell.1), b.2.max(cell.2)),
            ),
        });
    }
    Ok(Painted {
        cells: painted,
        origin: (origin[0], origin[1], origin[2]),
        size,
        grid: [cols, rows],
        covered: grid.inside.iter().filter(|inside| **inside).count(),
        rays,
        steep,
        bounds,
    })
}

/// The solid cells a ray from `start` along `dir` meets within `depth`
/// cells of the first one, each with the outward normal of the face it
/// entered through. `None` when it meets none.
fn march(
    cells: &HashMap<Cell, u8>,
    start: V3,
    dir: V3,
    lo: V3,
    hi: V3,
    depth: u32,
) -> Option<Vec<(Cell, V3)>> {
    // Clip to the bounds.
    let (mut t0, mut t1) = (0.0f64, f64::MAX);
    for a in 0..3 {
        if dir[a].abs() < 1e-12 {
            if start[a] < lo[a] || start[a] > hi[a] {
                return None;
            }
            continue;
        }
        let (mut near, mut far) = ((lo[a] - start[a]) / dir[a], (hi[a] - start[a]) / dir[a]);
        if near > far {
            std::mem::swap(&mut near, &mut far);
        }
        t0 = t0.max(near);
        t1 = t1.min(far);
    }
    if t0 > t1 {
        return None;
    }
    let entry: V3 = std::array::from_fn(|a| start[a] + dir[a] * (t0 + 1e-9));
    let mut cell = [0i32; 3];
    let mut step = [0i32; 3];
    let mut next = [f64::MAX; 3];
    let mut delta = [f64::MAX; 3];
    for a in 0..3 {
        cell[a] = entry[a].floor() as i32;
        if dir[a] > 1e-12 {
            step[a] = 1;
            delta[a] = 1.0 / dir[a];
            next[a] = t0 + (f64::from(cell[a]) + 1.0 - entry[a]) / dir[a];
        } else if dir[a] < -1e-12 {
            step[a] = -1;
            delta[a] = -1.0 / dir[a];
            next[a] = t0 + (f64::from(cell[a]) - entry[a]) / dir[a];
        }
    }
    // The face the ray entered the bounds through.
    let mut face = [0.0f64; 3];
    let first_axis = (0..3)
        .max_by(|a, b| {
            let near = |a: usize| {
                if dir[a].abs() < 1e-12 {
                    f64::MIN
                } else {
                    ((if dir[a] > 0.0 { lo[a] } else { hi[a] }) - start[a]) / dir[a]
                }
            };
            near(*a).total_cmp(&near(*b))
        })
        .unwrap_or(2);
    face[first_axis] = -dir[first_axis].signum();
    let mut t = t0;
    let mut found: Vec<(Cell, V3)> = Vec::new();
    let mut hit_t: Option<f64> = None;
    let limit = f64::from(depth);
    loop {
        if t > t1 + 1e-9 {
            break;
        }
        if let Some(h) = hit_t {
            if t >= h + limit - 1e-6 {
                break;
            }
        }
        let key = (cell[0], cell[1], cell[2]);
        if cells.contains_key(&key) {
            if hit_t.is_none() {
                hit_t = Some(t);
            }
            found.push((key, face));
        }
        // Step to the nearest boundary.
        let a = if next[0] <= next[1] && next[0] <= next[2] {
            0
        } else if next[1] <= next[2] {
            1
        } else {
            2
        };
        if next[a] == f64::MAX {
            break;
        }
        t = next[a];
        next[a] += delta[a];
        cell[a] += step[a];
        face = [0.0; 3];
        face[a] = -f64::from(step[a]);
    }
    (!found.is_empty()).then_some(found)
}
