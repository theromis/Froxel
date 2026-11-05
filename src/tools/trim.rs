//! The cut behind MCP `trim_outline`: remove every solid cell outside a surface
//! given as a closed plan outline and an inset that depends on height, so a
//! finished hull narrows toward its keel in one call.
//!
//! The surface at height `z` is the outline moved inward by `inset(z)`. A
//! cell stays when its centre is at least that far inside the outline. The
//! distance depends on `x` and `y` alone, so it is found once per column. The
//! optional skin follows the new surface and seals a hollow the cut opened.

use std::collections::HashMap;

use super::sculpt::MAX_SCULPT_CELLS;
use super::surface::Region;
use super::sweep::{catmull, snap, Profile};
use crate::state::Cell;

/// Most outline points and most inset rows.
pub const MAX_TRIM_POINTS: usize = 1024;
/// Most skin thickness, in cells.
pub const MAX_SKIN_THICKNESS: f64 = 64.0;
/// Farthest outline point, inset row, or inset from the origin.
const MAX_TRIM_REACH: f64 = 4096.0;
/// Most tests: columns times outline segments, and columns times layers for
/// the skin, about 2 seconds of them.
const MAX_TRIM_WORK: i64 = 512_000_000;

/// The new surface's skin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrimSkin {
    pub palette: u8,
    /// Cells inward from the surface, 1..=64.
    pub thickness: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrimSpec {
    /// The closed plan outline, `[x, y]` in either winding. A whole number is
    /// a cell centre.
    pub outline: Vec<[f64; 2]>,
    /// Pass a closed Catmull-Rom curve through the outline's points.
    pub smooth: bool,
    /// `[z, inset]` with `z` ascending: linear between rows, and the end
    /// value beyond them. A negative inset stands outside the outline.
    pub inset: Vec<[f64; 2]>,
    /// Nothing outside this box is removed or written.
    pub region: Option<Region>,
    /// Only these slots are removed or skinned; empty is every slot.
    pub only: Vec<u8>,
    pub skin: Option<TrimSkin>,
}

impl TrimSpec {
    fn listed(&self, slot: u8) -> bool {
        self.only.is_empty() || self.only.contains(&slot)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Trimmed {
    pub writes: Vec<(i32, i32, i32, i32)>,
    pub removed: usize,
    /// Solid cells the skin recoloured.
    pub painted: usize,
    /// Empty cells the skin filled.
    pub added: usize,
    /// Why nothing was removed.
    pub note: Option<String>,
}

/// The outline's field over a rectangle of columns: how far inside the outline
/// each column is, negative outside, and the way out from it.
struct Plan {
    min: (i32, i32),
    width: i32,
    height: i32,
    depth: Vec<f64>,
    out: Vec<[f64; 2]>,
}

impl Plan {
    fn new(path: &[[f64; 2]], min: (i32, i32), max: (i32, i32)) -> Plan {
        let (width, height) = (max.0 - min.0 + 1, max.1 - min.1 + 1);
        let area = twice_area(path);
        let polygon = Profile::Polygon(path.to_vec());
        let count = width as usize * height as usize;
        let (mut depth, mut out) = (Vec::with_capacity(count), Vec::with_capacity(count));
        for y in min.1..=max.1 {
            for x in min.0..=max.0 {
                let c = [f64::from(x), f64::from(y)];
                let (mut best, mut nearest, mut along) = (f64::MAX, c, [1.0, 0.0]);
                for i in 0..path.len() {
                    let (a, b) = (path[i], path[(i + 1) % path.len()]);
                    let d = [b[0] - a[0], b[1] - a[1]];
                    let t =
                        ((c[0] - a[0]) * d[0] + (c[1] - a[1]) * d[1]) / (d[0] * d[0] + d[1] * d[1]);
                    let t = t.clamp(0.0, 1.0);
                    let q = [a[0] + d[0] * t, a[1] + d[1] * t];
                    let far = (c[0] - q[0]).powi(2) + (c[1] - q[1]).powi(2);
                    if far < best {
                        (best, nearest, along) = (far, q, d);
                    }
                }
                let distance = snap(best.sqrt());
                let inside = polygon.contains(c[0], c[1]);
                depth.push(if inside { distance } else { -distance });
                out.push(if distance > 1.0e-6 {
                    let sign = if inside { 1.0 } else { -1.0 } / distance;
                    [(nearest[0] - c[0]) * sign, (nearest[1] - c[1]) * sign]
                } else {
                    // On the outline: the edge's normal, away from the inside.
                    let l = (along[0] * along[0] + along[1] * along[1]).sqrt();
                    let wind = if area > 0.0 { 1.0 } else { -1.0 };
                    [along[1] / l * wind, -along[0] / l * wind]
                });
            }
        }
        Plan {
            min,
            width,
            height,
            depth,
            out,
        }
    }

    fn index(&self, x: i32, y: i32) -> usize {
        ((y - self.min.1) * self.width + (x - self.min.0)) as usize
    }

    fn depth(&self, x: i32, y: i32) -> f64 {
        self.depth[self.index(x, y)]
    }

    fn has(&self, x: i32, y: i32) -> bool {
        (0..self.width).contains(&(x - self.min.0)) && (0..self.height).contains(&(y - self.min.1))
    }

    /// Whether the way out from a column crosses a cell of `layer`. A step
    /// across a corner tests both cells beside it, so the ray does not slip
    /// through a diagonal wall one cell thick.
    fn reaches(&self, layer: &[bool], x: i32, y: i32) -> bool {
        let out = self.out[self.index(x, y)];
        let hit = |x: i32, y: i32| self.has(x, y) && layer[self.index(x, y)];
        let (mut px, mut py) = (f64::from(x), f64::from(y));
        let (mut cx, mut cy) = (x, y);
        loop {
            px += out[0] * 0.5;
            py += out[1] * 0.5;
            let (nx, ny) = (px.round() as i32, py.round() as i32);
            if (nx, ny) == (cx, cy) {
                continue;
            }
            if !self.has(nx, ny) {
                return false;
            }
            if hit(nx, ny) || (nx != cx && ny != cy && (hit(nx, cy) || hit(cx, ny))) {
                return true;
            }
            (cx, cy) = (nx, ny);
        }
    }
}

/// Twice the signed area of a closed path, positive when counter-clockwise.
fn twice_area(path: &[[f64; 2]]) -> f64 {
    (0..path.len())
        .map(|i| {
            let (a, b) = (path[i], path[(i + 1) % path.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum()
}

/// The closed path: the outline's points with repeats dropped (a path that
/// returns to its start is the same outline), or a closed curve through them.
fn closed_path(spec: &TrimSpec) -> Result<Vec<[f64; 2]>, String> {
    if spec.outline.len() > MAX_TRIM_POINTS {
        return Err(format!(
            "outline takes at most {MAX_TRIM_POINTS} points; simplify it"
        ));
    }
    for (index, point) in spec.outline.iter().enumerate() {
        if point
            .iter()
            .any(|v| !v.is_finite() || v.abs() > MAX_TRIM_REACH)
        {
            return Err(format!(
                "outline point {index} must be finite and within {MAX_TRIM_REACH} cells"
            ));
        }
    }
    let mut points: Vec<[f64; 2]> = Vec::with_capacity(spec.outline.len());
    for point in &spec.outline {
        if points.last() != Some(point) {
            points.push(*point);
        }
    }
    if points.len() > 1 && points.first() == points.last() {
        points.pop();
    }
    if points.len() < 3 {
        return Err("outline needs at least 3 different [x, y] points".into());
    }
    let n = points.len();
    let path: Vec<[f64; 2]> = if spec.smooth {
        let lift = |p: [f64; 2]| [p[0], p[1], 0.0];
        let mut path = Vec::new();
        for i in 0..n {
            let (p0, p1) = (points[(i + n - 1) % n], points[i]);
            let (p2, p3) = (points[(i + 1) % n], points[(i + 2) % n]);
            let steps = ((p2[0] - p1[0]).hypot(p2[1] - p1[1]).ceil() as usize).max(1);
            for step in 0..steps {
                let (at, _) = catmull(
                    lift(p0),
                    lift(p1),
                    lift(p2),
                    lift(p3),
                    step as f64 / steps as f64,
                );
                path.push([at[0], at[1]]);
            }
        }
        path
    } else {
        points
    };
    if twice_area(&path).abs() < 1.0e-6 {
        return Err("the outline encloses no area".into());
    }
    Ok(path)
}

fn check_inset(rows: &[[f64; 2]]) -> Result<(), String> {
    if rows.is_empty() || rows.len() > MAX_TRIM_POINTS {
        return Err(format!(
            "inset needs 1 to {MAX_TRIM_POINTS} [z, inset] pairs"
        ));
    }
    if rows
        .iter()
        .flatten()
        .any(|v| !v.is_finite() || v.abs() > MAX_TRIM_REACH)
    {
        return Err(format!(
            "inset values must be finite and within {MAX_TRIM_REACH} cells"
        ));
    }
    if rows.windows(2).any(|pair| pair[1][0] <= pair[0][0]) {
        return Err("inset [z, inset] pairs must go up in z, with no z twice".into());
    }
    Ok(())
}

/// How far inward the surface stands at `z`.
fn inset_at(rows: &[[f64; 2]], z: f64) -> f64 {
    let after = rows.partition_point(|row| row[0] <= z);
    match (after.checked_sub(1).map(|i| rows[i]), rows.get(after)) {
        (Some(a), Some(b)) => a[1] + (b[1] - a[1]) * (z - a[0]) / (b[0] - a[0]),
        (Some(a), None) => a[1],
        (None, Some(b)) => b[1],
        (None, None) => 0.0,
    }
}

/// Remove the solid cells outside the surface and lay the skin. Nothing changes
/// on `cells`: the writes are for the caller to apply as one step.
pub fn trim_outline(cells: &HashMap<Cell, u8>, spec: &TrimSpec) -> Result<Trimmed, String> {
    let path = closed_path(spec)?;
    check_inset(&spec.inset)?;
    if let Some(skin) = spec.skin {
        if !(1.0..=MAX_SKIN_THICKNESS).contains(&skin.thickness) {
            return Err(format!(
                "thickness must be 1..={MAX_SKIN_THICKNESS}, not {}",
                skin.thickness
            ));
        }
    }
    let region = spec.region.unwrap_or_else(|| {
        Region::spanning(
            (i32::MIN, i32::MIN, i32::MIN),
            (i32::MAX, i32::MAX, i32::MAX),
        )
    });
    let solid = region.solid(cells);
    if solid.len() as i64 > MAX_SCULPT_CELLS {
        return Err(format!(
            "more than {MAX_SCULPT_CELLS} solid cells to test; pass a smaller box"
        ));
    }
    let Some(&first) = solid.first() else {
        return Ok(Trimmed {
            note: Some("no solid cell in the box".into()),
            ..Trimmed::default()
        });
    };
    let (mut low, mut high) = (first, first);
    for c in &solid {
        low = (low.0.min(c.0), low.1.min(c.1), low.2.min(c.2));
        high = (high.0.max(c.0), high.1.max(c.1), high.2.max(c.2));
    }
    // One column of margin, so a column at the edge has neighbours to test.
    let columns = (i64::from(high.0 - low.0) + 3) * (i64::from(high.1 - low.1) + 3);
    let layers = i64::from(high.2 - low.2) + 1;
    let work = columns * path.len() as i64 + spec.skin.map_or(0, |_| columns * layers);
    if work > MAX_TRIM_WORK {
        return Err(format!(
            "the trim is too large to test ({work} tests, limit {MAX_TRIM_WORK}); pass a smaller box or use fewer outline points"
        ));
    }
    let plan = Plan::new(&path, (low.0 - 1, low.1 - 1), (high.0 + 1, high.1 + 1));
    // The skin looks this many layers up and down, so the insets reach as far.
    let reach = spec.skin.map_or(0, |skin| skin.thickness.floor() as i32);
    let insets: Vec<f64> = (low.2 - reach..=high.2 + reach)
        .map(|z| inset_at(&spec.inset, f64::from(z)))
        .collect();
    let inset = |z: i32| insets[(z - low.2 + reach) as usize];
    let mut done = Trimmed::default();
    // The removed cells, one column rectangle per layer that has any.
    let mut removed: HashMap<i32, Vec<bool>> = HashMap::new();
    let plan_cells = plan.depth.len();
    for &(x, y, z) in &solid {
        if snap(plan.depth(x, y) - inset(z)) >= 0.0 || !spec.listed(cells[&(x, y, z)]) {
            continue;
        }
        done.writes.push((x, y, z, 0));
        done.removed += 1;
        removed.entry(z).or_insert_with(|| vec![false; plan_cells])[plan.index(x, y)] = true;
    }
    if done.removed == 0 {
        done.note = Some(format!(
            "none of the {} solid cells{} lies outside the surface; the outline is in the object's local cells",
            solid.len(),
            if spec.only.is_empty() { "" } else { " of the only slots" },
        ));
        return Ok(done);
    }
    if let Some(skin) = spec.skin {
        lay_skin(
            cells,
            spec,
            &skin,
            &plan,
            &removed,
            (low, high),
            &inset,
            &mut done,
        );
    }
    Ok(done)
}

/// Lay the skin in the kept cells next to the surface: those within the
/// thickness of it across a layer, or by the sphere of that radius into the
/// layers above and below on a slope, and any cell touching a cut cell by a
/// face, so a slanted skin has no gaps. Only where the cut reached: an empty
/// cell is filled when the way out from it crosses a removed cell of its own
/// layer, so a hollow is sealed but a skin never rises past a hull's rim or
/// closes a gun port, and a solid cell is recoloured when that way crosses one
/// in the layers the thickness reaches.
#[allow(clippy::too_many_arguments)]
fn lay_skin(
    cells: &HashMap<Cell, u8>,
    spec: &TrimSpec,
    skin: &TrimSkin,
    plan: &Plan,
    removed: &HashMap<i32, Vec<bool>>,
    (low, high): (Cell, Cell),
    inset: &dyn Fn(i32) -> f64,
    done: &mut Trimmed,
) {
    let thick = skin.thickness;
    let reach = thick.floor() as i32;
    let spheres: Vec<(i32, f64)> = (1..=reach)
        .map(|dz| (dz, (thick * thick - f64::from(dz * dz)).max(0.0).sqrt()))
        .collect();
    let lowest = removed.keys().copied().min().unwrap_or(low.2) - reach;
    let highest = removed.keys().copied().max().unwrap_or(high.2) + reach;
    for z in lowest.max(low.2)..=highest.min(high.2) {
        let at = inset(z);
        for y in low.1..=high.1 {
            for x in low.0..=high.0 {
                let depth = plan.depth(x, y);
                let over = snap(depth - at);
                if over < 0.0 {
                    continue;
                }
                let near = over < thick
                    || [(1, 0), (-1, 0), (0, 1), (0, -1)]
                        .iter()
                        .any(|&(dx, dy)| snap(plan.depth(x + dx, y + dy) - at) < 0.0)
                    || spheres.iter().any(|&(dz, across)| {
                        snap(depth - inset(z + dz) - across) < 0.0
                            || snap(depth - inset(z - dz) - across) < 0.0
                    });
                if !near {
                    continue;
                }
                let crossed = |layer: i32| {
                    removed
                        .get(&layer)
                        .is_some_and(|cut| plan.reaches(cut, x, y))
                };
                match cells.get(&(x, y, z)) {
                    Some(&slot) => {
                        if slot != skin.palette
                            && spec.listed(slot)
                            && (z - reach..=z + reach).any(crossed)
                        {
                            done.writes.push((x, y, z, i32::from(skin.palette)));
                            done.painted += 1;
                        }
                    }
                    None => {
                        if crossed(z) {
                            done.writes.push((x, y, z, i32::from(skin.palette)));
                            done.added += 1;
                        }
                    }
                }
            }
        }
    }
}
