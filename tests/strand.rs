use super::*;

fn point(x: f32, y: f32, z: f32) -> StrandPoint {
    StrandPoint {
        at: [x, y, z],
        twist: 0.0,
        scale: 1.0,
    }
}

fn line(points: &[[f32; 3]]) -> Strand {
    Strand {
        points: points.iter().map(|p| point(p[0], p[1], p[2])).collect(),
        smooth: false,
        ..Strand::default()
    }
}

#[test]
fn a_straight_strand_of_one_slot_is_one_long_box() {
    let strand = line(&[[0.0, 0.0, 0.0], [6.0, 0.0, 0.0]]);
    strand.check().unwrap();
    let boxes = strand.boxes();
    assert_eq!(boxes.len(), 1);
    let only = boxes[0];
    assert!((only.half[0] - 3.0).abs() < 1.0e-4, "{only:?}");
    // From the centre of cell 0 to the centre of cell 6.
    assert!((only.center[0] - 3.5).abs() < 1.0e-4);
    assert!((only.center[1] - 0.5).abs() < 1.0e-4);
    assert!((only.center[2] - 0.5).abs() < 1.0e-4);
}

#[test]
fn a_gradient_splits_a_straight_run_by_slot() {
    let mut strand = line(&[[0.0, 0.0, 0.0], [6.0, 0.0, 0.0]]);
    strand.slots = vec![3, 4, 5];
    let boxes = strand.boxes();
    let slots: Vec<u8> = boxes.iter().map(|b| b.slot).collect();
    assert_eq!(slots, vec![3, 4, 5]);
}

#[test]
fn cubes_follow_the_path_and_its_tangent() {
    let mut strand = line(&[[0.0, 0.0, 0.0], [0.0, 0.0, 4.0], [4.0, 0.0, 8.0]]);
    strand.slots = vec![1, 2];
    strand.pattern = StrandPattern::Random;
    strand.seed = 7;
    let boxes = strand.boxes();
    assert!(!boxes.is_empty());
    for b in &boxes {
        let t = glam::Vec3::from_array(b.axes[0]);
        let l = glam::Vec3::from_array(b.axes[1]);
        let u = glam::Vec3::from_array(b.axes[2]);
        assert!((t.length() - 1.0).abs() < 1.0e-4);
        assert!(t.dot(l).abs() < 1.0e-4 && t.dot(u).abs() < 1.0e-4 && l.dot(u).abs() < 1.0e-4);
    }
    // The last cube leans 45 degrees toward +x.
    let last = boxes.last().unwrap();
    let t = glam::Vec3::from_array(last.axes[0]);
    assert!((t.x - t.z).abs() < 1.0e-3 && t.x > 0.5, "{t:?}");
}

#[test]
fn the_cube_count_fits_the_length_exactly() {
    let mut strand = line(&[[0.0, 0.0, 0.0], [0.0, 5.0, 0.0]]);
    strand.step = 2.0;
    strand.slots = vec![1, 2];
    strand.pattern = StrandPattern::Rings;
    strand.period = 0.5;
    // 5 cells at step 2: two or three cubes, whatever fills 5 exactly.
    let boxes = strand.boxes();
    let total: f32 = boxes.iter().map(|b| b.half[0] * 2.0).sum();
    assert!((total - 5.0).abs() < 1.0e-3, "{total}");
}

#[test]
fn twist_turns_the_section_about_the_path() {
    let mut strand = line(&[[0.0, 0.0, 0.0], [4.0, 0.0, 0.0]]);
    strand.width = 2.0;
    strand.thickness = 0.5;
    for p in &mut strand.points {
        p.twist = 90.0;
    }
    let b = strand.boxes()[0];
    // The path runs along +x, its left is +y; turned 90 degrees, left is +z.
    let left = glam::Vec3::from_array(b.axes[1]);
    assert!((left - glam::Vec3::Z).length() < 1.0e-4, "{left:?}");
}

#[test]
fn a_ray_meets_a_turned_box_on_its_face() {
    let mut strand = line(&[[0.0, 0.0, 0.0], [4.0, 4.0, 0.0]]);
    strand.width = 1.0;
    let b = strand.boxes()[0];
    let m = b.matrix();
    let inv = m.inverse();
    let center = glam::Vec3::from_array(b.center);
    let origin = center + glam::Vec3::new(0.0, 0.0, 10.0);
    let (t, normal) = ray_box(&inv, &m, origin, -glam::Vec3::Z).unwrap();
    assert!((t - 9.5).abs() < 1.0e-3, "{t}");
    assert!((normal - glam::Vec3::Z).length() < 1.0e-3, "{normal:?}");
    assert!(ray_box(
        &inv,
        &m,
        origin + glam::Vec3::new(20.0, 0.0, 0.0),
        -glam::Vec3::Z
    )
    .is_none());
}

#[test]
fn bad_strands_are_refused() {
    let mut strand = line(&[[0.0, 0.0, 0.0]]);
    assert!(strand.check().is_err());
    strand = line(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
    strand.step = 0.0;
    assert!(strand.check().is_err());
    strand.step = 1.0;
    strand.slots = vec![0];
    assert!(strand.check().is_err());
}

#[test]
fn mirroring_twice_gives_the_strand_back() {
    let mut strand = line(&[[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]);
    strand.points[0].twist = 30.0;
    assert_eq!(strand.mirrored(0, 2.5).mirrored(0, 2.5), strand);
}
