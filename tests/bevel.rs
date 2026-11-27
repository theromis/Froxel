use std::collections::HashSet;

use super::*;

/// Whether `p` lies inside the bevelled solid, from the definition: in a
/// solid cell and on the kept side of each of its cut edges.
fn inside(cells: &HashSet<Cell>, bevel: Bevel, p: Vec3) -> bool {
    let cell = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
    if !cells.contains(&cell) {
        return false;
    }
    let solid = |c: Cell| cells.contains(&c);
    let local = p - Vec3::new(cell.0 as f32, cell.1 as f32, cell.2 as f32) - Vec3::splat(0.5);
    for (a, sa, b, sb) in edges() {
        if is_cut(&solid, bevel.mode, cell, (a, sa, b, sb))
            && sa * local[a] + sb * local[b] > 1.0 - bevel.size
        {
            return false;
        }
    }
    true
}

fn crossings(polys: &[Poly], origin: Vec3, dir: Vec3) -> usize {
    let mut count = 0;
    for poly in polys {
        let a = poly.points[0];
        for k in 1..poly.points.len() - 1 {
            let (b, c) = (poly.points[k], poly.points[k + 1]);
            let (e1, e2) = (b - a, c - a);
            let p = dir.cross(e2);
            let det = e1.dot(p);
            if det.abs() < 1.0e-9 {
                continue;
            }
            let s = origin - a;
            let u = s.dot(p) / det;
            let q = s.cross(e1);
            let v = dir.dot(q) / det;
            let t = e2.dot(q) / det;
            if u >= 0.0 && v >= 0.0 && u + v <= 1.0 && t > 0.0 {
                count += 1;
            }
        }
    }
    count
}

struct Rng(u32);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(747796405).wrapping_add(2891336453);
        (self.0 >> 8) as f32 / 16777216.0
    }
}

/// An L-shaped step with a hole and a lone diagonal cell: convex and
/// concave edges, walls, and a corner touching only along an edge.
fn shape() -> HashSet<Cell> {
    let mut cells = HashSet::new();
    for x in 0..4 {
        for y in 0..3 {
            cells.insert((x, y, 0));
        }
    }
    for y in 0..3 {
        cells.insert((0, y, 1));
        cells.insert((1, y, 1));
    }
    cells.insert((0, 0, 2));
    cells.remove(&(2, 1, 0));
    cells.insert((4, 3, 1));
    cells.insert((3, 3, 2));
    cells
}

fn check_closed(mode: BevelMode) {
    let cells = shape();
    let bevel = Bevel { size: 0.2, mode };
    let listed: Vec<(Cell, u8)> = cells.iter().map(|c| (*c, 1)).collect();
    let mut polys = Vec::new();
    cell_polys(|c| cells.contains(&c), &listed, bevel, &mut polys);
    assert!(!polys.is_empty());
    for poly in &polys {
        // Every polygon faces the way its normal says.
        let n = (poly.points[1] - poly.points[0]).cross(poly.points[2] - poly.points[0]);
        assert!(n.dot(poly.normal) > 0.0, "{poly:?}");
    }
    let mut rng = Rng(7);
    let mut checked = 0;
    for _ in 0..4000 {
        let p = Vec3::new(
            rng.next() * 7.0 - 1.5,
            rng.next() * 6.0 - 1.5,
            rng.next() * 5.0 - 1.5,
        );
        let dir = Vec3::new(rng.next() - 0.5, rng.next() - 0.5, rng.next() - 0.5).normalize();
        let parity = crossings(&polys, p, dir) % 2 == 1;
        assert_eq!(
            parity,
            inside(&cells, bevel, p),
            "{mode:?} at {p} along {dir}"
        );
        checked += 1;
    }
    assert_eq!(checked, 4000);
}

#[test]
fn the_cubes_bevel_is_closed() {
    check_closed(BevelMode::Cubes);
}

#[test]
fn the_edges_bevel_is_closed() {
    check_closed(BevelMode::Edges);
}

#[test]
fn edges_keep_a_flat_run_whole_and_cubes_groove_it() {
    let cells: HashSet<Cell> = (0..4).map(|x| (x, 0, 0)).collect();
    let listed: Vec<(Cell, u8)> = cells.iter().map(|c| (*c, 1)).collect();
    let top = |mode| {
        let mut polys = Vec::new();
        cell_polys(
            |c| cells.contains(&c),
            &listed,
            Bevel { size: 0.2, mode },
            &mut polys,
        );
        polys
            .iter()
            .filter(|p| p.normal == Vec3::Z)
            .map(|p| {
                let xs: Vec<f32> = p.points.iter().map(|q| q.x).collect();
                xs.iter().cloned().fold(f32::MIN, f32::max)
                    - xs.iter().cloned().fold(f32::MAX, f32::min)
            })
            .collect::<Vec<f32>>()
    };
    // Edges: each top face reaches its neighbours (width 1, the ends 0.8).
    let edges = top(BevelMode::Edges);
    assert_eq!(edges.len(), 4);
    assert!(edges.iter().filter(|w| (**w - 1.0).abs() < 1e-5).count() == 2);
    // Cubes: every top face is inset on both sides.
    assert!(top(BevelMode::Cubes).iter().all(|w| (w - 0.6).abs() < 1e-5));
}

#[test]
fn a_straight_view_never_looks_through_a_cubes_block() {
    let mut cells = HashSet::new();
    for x in 0..3 {
        for y in 0..3 {
            for z in 0..3 {
                cells.insert((x, y, z));
            }
        }
    }
    let listed: Vec<(Cell, u8)> = cells.iter().map(|c| (*c, 1)).collect();
    let mut polys = Vec::new();
    let bevel = Bevel {
        size: 0.3,
        mode: BevelMode::Cubes,
    };
    cell_polys(|c| cells.contains(&c), &listed, bevel, &mut polys);
    // Straight down through every inner lattice line, a hair off it.
    for (x, y) in [(1.0, 1.0), (2.0, 1.0), (1.0, 2.0), (2.0, 2.0)] {
        let origin = Vec3::new(x + 1.0e-3, y + 2.0e-3, 10.0);
        assert!(
            crossings(&polys, origin, -Vec3::Z) >= 2,
            "a hole at {x}, {y}"
        );
    }
}

#[test]
fn a_strand_box_is_bevelled_into_a_closed_solid() {
    let b = crate::tools::StrandBox {
        center: [1.0, 2.0, 3.0],
        axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        half: [2.0, 0.5, 0.2],
        slot: 3,
    };
    let mut polys = Vec::new();
    box_polys(&b, 0.3, &mut polys);
    // 6 faces and 12 cuts.
    assert_eq!(polys.len(), 18);
    let mut rng = Rng(3);
    for _ in 0..500 {
        let p = Vec3::new(1.0, 2.0, 3.0)
            + Vec3::new(
                rng.next() * 5.0 - 2.5,
                rng.next() * 1.4 - 0.7,
                rng.next() * 0.6 - 0.3,
            );
        let dir = Vec3::new(rng.next() - 0.5, rng.next() - 0.5, rng.next() - 0.5).normalize();
        let inside_box = (p - Vec3::new(1.0, 2.0, 3.0))
            .abs()
            .cmple(Vec3::new(2.0, 0.5, 0.2))
            .all();
        let parity = crossings(&polys, p, dir) % 2 == 1;
        if !inside_box {
            assert!(!parity);
        }
    }
}

#[test]
fn random_shapes_stay_closed_in_both_modes() {
    for seed in 1..9u32 {
        let mut rng = Rng(seed * 7919);
        let mut cells = HashSet::new();
        for x in 0..5 {
            for y in 0..5 {
                for z in 0..5 {
                    if rng.next() < 0.55 {
                        cells.insert((x, y, z));
                    }
                }
            }
        }
        let listed: Vec<(Cell, u8)> = cells.iter().map(|c| (*c, 1)).collect();
        for mode in [BevelMode::Cubes, BevelMode::Edges] {
            let bevel = Bevel { size: 0.25, mode };
            let mut polys = Vec::new();
            cell_polys(|c| cells.contains(&c), &listed, bevel, &mut polys);
            for _ in 0..1500 {
                let p = Vec3::new(
                    rng.next() * 7.0 - 1.0,
                    rng.next() * 7.0 - 1.0,
                    rng.next() * 7.0 - 1.0,
                );
                let dir =
                    Vec3::new(rng.next() - 0.5, rng.next() - 0.5, rng.next() - 0.5).normalize();
                assert_eq!(
                    crossings(&polys, p, dir) % 2 == 1,
                    inside(&cells, bevel, p),
                    "seed {seed} {mode:?} at {p} along {dir}"
                );
            }
        }
    }
}
