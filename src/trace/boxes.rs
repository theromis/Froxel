//! Strand boxes in the tracer: each box as the map from the world into its
//! unit cube `-1..1`, under a bounding volume hierarchy. `trace.wgsl` packs
//! the same rows and nodes and walks them the same way, so the CPU tracer
//! stays the tested reference for both.

use glam::Vec3;

use crate::state::WorldBox;

/// Most boxes in one leaf.
const LEAF: usize = 4;
/// Box hits closer than this along the ray are the surface the ray left.
pub(super) const MIN_T: f32 = 1.0e-4;
/// How near a box's face, in unit-cube units, a ray's start counts as on it.
const SURFACE: f32 = 1.0e-3;

/// One box: `unit = rows * [p, 1]`, a palette slot.
#[derive(Clone, Copy, Debug)]
pub(super) struct TraceBox {
    pub rows: [[f32; 4]; 3],
    pub slot: u8,
}

/// A node: its bounds, and either two children (`count` 0, `first` the left
/// child, the right one next to it) or `count` boxes from `first`.
#[derive(Clone, Copy, Debug)]
pub(super) struct Node {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub first: u32,
    pub count: u32,
}

#[derive(Default)]
pub(crate) struct Boxes {
    pub(super) boxes: Vec<TraceBox>,
    pub(super) nodes: Vec<Node>,
}

impl Boxes {
    pub(crate) fn new(world: &[WorldBox]) -> Boxes {
        if world.is_empty() {
            return Boxes::default();
        }
        let mut items: Vec<(TraceBox, Vec3, Vec3)> = world
            .iter()
            .map(|b| {
                let m = b.inverse;
                let rows = [
                    [m.x_axis.x, m.y_axis.x, m.z_axis.x, m.w_axis.x],
                    [m.x_axis.y, m.y_axis.y, m.z_axis.y, m.w_axis.y],
                    [m.x_axis.z, m.y_axis.z, m.z_axis.z, m.w_axis.z],
                ];
                let (lo, hi) = b.aabb();
                (TraceBox { rows, slot: b.slot }, lo, hi)
            })
            .collect();
        let mut nodes = Vec::with_capacity(items.len() * 2);
        nodes.push(Node {
            min: [0.0; 3],
            max: [0.0; 3],
            first: 0,
            count: 0,
        });
        build(&mut items, 0, 0, &mut nodes);
        Boxes {
            boxes: items.into_iter().map(|(b, _, _)| b).collect(),
            nodes,
        }
    }

    /// The nearest box hit closer than `limit`: distance in `dir` units, the
    /// outward normal, and the slot.
    pub(crate) fn hit(&self, origin: Vec3, dir: Vec3, limit: f32) -> Option<(f32, Vec3, u8)> {
        self.hit_with(origin, dir, limit, true)
    }

    /// The bounds of every box, or `None` without boxes.
    pub(crate) fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let root = self.nodes.first()?;
        Some((Vec3::from_array(root.min), Vec3::from_array(root.max)))
    }

    /// [`Boxes::hit`] for occlusion rays leaving a surface: a ray that starts
    /// on a box's own face (within a rounding error inside it) is leaving that
    /// box, not inside it, so a ray grazing its face does not shade itself.
    /// A ray that starts deeper inside a box (an overlapping neighbour) is
    /// still stopped by it.
    pub(crate) fn hit_outside(
        &self,
        origin: Vec3,
        dir: Vec3,
        limit: f32,
    ) -> Option<(f32, Vec3, u8)> {
        self.hit_with(origin, dir, limit, false)
    }

    fn hit_with(
        &self,
        origin: Vec3,
        dir: Vec3,
        limit: f32,
        inside: bool,
    ) -> Option<(f32, Vec3, u8)> {
        if self.boxes.is_empty() {
            return None;
        }
        let inv = Vec3::new(safe_inv(dir.x), safe_inv(dir.y), safe_inv(dir.z));
        let mut best: Option<(f32, Vec3, u8)> = None;
        let mut nearest = limit;
        let mut stack = [0u32; 64];
        let mut top = 1usize;
        while top > 0 {
            top -= 1;
            let node = self.nodes[stack[top] as usize];
            if !slab(origin, inv, node.min, node.max, nearest) {
                continue;
            }
            if node.count == 0 {
                if top + 2 > stack.len() {
                    continue;
                }
                stack[top] = node.first;
                stack[top + 1] = node.first + 1;
                top += 2;
                continue;
            }
            for b in &self.boxes[node.first as usize..(node.first + node.count) as usize] {
                if let Some((t, normal, depth)) = box_hit(b, origin, dir) {
                    if (inside || depth < -SURFACE || depth > SURFACE) && t >= MIN_T && t < nearest
                    {
                        nearest = t;
                        best = Some((t, normal, b.slot));
                    }
                }
            }
        }
        best
    }
}

fn safe_inv(v: f32) -> f32 {
    if v.abs() < 1.0e-12 {
        if v.is_sign_negative() {
            -1.0e12
        } else {
            1.0e12
        }
    } else {
        1.0 / v
    }
}

/// Whether the ray meets the bounds before `limit`.
fn slab(origin: Vec3, inv: Vec3, min: [f32; 3], max: [f32; 3], limit: f32) -> bool {
    let mut t0 = 0.0f32;
    let mut t1 = limit;
    for axis in 0..3 {
        let a = (min[axis] - origin[axis]) * inv[axis];
        let b = (max[axis] - origin[axis]) * inv[axis];
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
    }
    t0 <= t1
}

/// The ray against one box: the entry distance from outside (or the exit
/// from inside), the outward normal of that face, and how deep inside the
/// box the ray starts, in unit-cube units (negative outside).
pub(super) fn box_hit(b: &TraceBox, origin: Vec3, dir: Vec3) -> Option<(f32, Vec3, f32)> {
    let row = |i: usize| Vec3::new(b.rows[i][0], b.rows[i][1], b.rows[i][2]);
    let mut near = f32::MIN;
    let mut far = f32::MAX;
    let mut near_axis = 0usize;
    let mut far_axis = 0usize;
    let mut near_sign = 1.0f32;
    let mut far_sign = 1.0f32;
    let mut outmost = 0.0f32;
    for axis in 0..3 {
        let r = row(axis);
        let o = r.dot(origin) + b.rows[axis][3];
        outmost = outmost.max(o.abs());
        let d = r.dot(dir);
        if d.abs() < 1.0e-12 {
            if o.abs() > 1.0 {
                return None;
            }
            continue;
        }
        let t0 = (-1.0 - o) / d;
        let t1 = (1.0 - o) / d;
        // Entering through -1 when moving up the axis, through +1 when down.
        let (enter, leave, enter_sign) = if t0 < t1 {
            (t0, t1, -1.0)
        } else {
            (t1, t0, 1.0)
        };
        if enter > near {
            near = enter;
            near_axis = axis;
            near_sign = enter_sign;
        }
        if leave < far {
            far = leave;
            far_axis = axis;
            far_sign = -enter_sign;
        }
        if near > far {
            return None;
        }
    }
    if far < 0.0 {
        return None;
    }
    let (t, axis, sign) = if near >= 0.0 {
        (near, near_axis, near_sign)
    } else {
        (far, far_axis, far_sign)
    };
    let normal = (row(axis) * sign).normalize_or_zero();
    Some((t, normal, 1.0 - outmost))
}

fn build(items: &mut [(TraceBox, Vec3, Vec3)], offset: usize, at: usize, nodes: &mut Vec<Node>) {
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for (_, lo, hi) in items.iter() {
        min = min.min(*lo);
        max = max.max(*hi);
    }
    nodes[at].min = min.to_array();
    nodes[at].max = max.to_array();
    if items.len() <= LEAF {
        nodes[at].first = offset as u32;
        nodes[at].count = items.len() as u32;
        return;
    }
    let size = max - min;
    let axis = if size.x >= size.y && size.x >= size.z {
        0
    } else if size.y >= size.z {
        1
    } else {
        2
    };
    items.sort_by(|a, b| {
        let ca = a.1[axis] + a.2[axis];
        let cb = b.1[axis] + b.2[axis];
        ca.total_cmp(&cb)
    });
    let half = items.len() / 2;
    let left = nodes.len();
    nodes[at].first = left as u32;
    nodes[at].count = 0;
    for _ in 0..2 {
        nodes.push(Node {
            min: [0.0; 3],
            max: [0.0; 3],
            first: 0,
            count: 0,
        });
    }
    let (a, b) = items.split_at_mut(half);
    build(a, offset, left, nodes);
    build(b, offset + half, left + 1, nodes);
}
