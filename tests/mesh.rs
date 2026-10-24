use super::*;

fn white(_: u8) -> [u8; 3] {
    [255, 255, 255]
}

fn still(_: u8) -> [f32; 2] {
    [0.0, 0.0]
}

#[test]
fn selection_shell_drops_the_face_between_two_picked_cells() {
    let mut cells = HashMap::new();
    cells.insert((0, 0, 0), 1);
    cells.insert((1, 0, 0), 1);
    let mut picked = HashSet::new();
    picked.insert((0, 0, 0));
    picked.insert((1, 0, 0));
    assert_eq!(selection_shell(&cells, &picked).face_count(), 10);
}

#[test]
fn translate_mesh_shifts_vertices() {
    let mut cells = HashMap::new();
    cells.insert((0, 0, 0), 1);
    let mut mesh = mesh_chunk(&cells, (0, 0, 0), white, still);
    translate_mesh(&mut mesh, (3, 0, 0));
    assert!(mesh.verts.iter().all(|vert| vert.pos[0] >= 3.0));
}

#[test]
fn one_voxel_has_six_faces() {
    let mut cells = HashMap::new();
    cells.insert((0, 0, 0), 1);
    let mesh = mesh_chunk(&cells, (0, 0, 0), white, still);
    assert_eq!(mesh.face_count(), 6);
}

#[test]
fn neighbors_drop_the_shared_face_across_a_chunk_boundary() {
    let mut cells = HashMap::new();
    cells.insert((15, 0, 0), 1);
    cells.insert((16, 0, 0), 1);
    let a = mesh_chunk(&cells, chunk_key((15, 0, 0)), white, still);
    let b = mesh_chunk(&cells, chunk_key((16, 0, 0)), white, still);
    assert_eq!(a.face_count() + b.face_count(), 10);
}

#[test]
fn negative_coordinates_stay_in_their_own_chunk() {
    assert_eq!(chunk_key((-1, 0, 0)), (-1, 0, 0));
    assert_eq!(chunk_key((-16, -17, 3)), (-1, -2, 0));
}

#[test]
fn a_changed_chunk_dirties_its_neighbors() {
    let mut prev = HashMap::new();
    prev.insert((0, 0, 0), 1);
    let mut next = HashMap::new();
    next.insert((0, 0, 0), 2);
    let dirty = dirty_chunks(&prev, &next);
    assert!(dirty.contains(&(0, 0, 0)));
    assert!(dirty.contains(&(1, 0, 0)));
    assert!(dirty.contains(&(-1, 0, 0)));
    assert_eq!(dirty.len(), 7);
}

#[test]
fn hash_ignores_insertion_order() {
    let a = occupancy_hash(&[((1, 2, 3), [9, 8, 7]), ((4, 5, 6), [1, 2, 3])]);
    let b = occupancy_hash(&[((4, 5, 6), [1, 2, 3]), ((1, 2, 3), [9, 8, 7])]);
    assert_eq!(a, b);
    let c = occupancy_hash(&[((1, 2, 3), [9, 8, 8]), ((4, 5, 6), [1, 2, 3])]);
    assert_ne!(a, c);
}

#[test]
fn sway_amount_and_height_are_stored_on_the_vertex() {
    let mut cells = HashMap::new();
    cells.insert((0, 0, 0), 1);
    cells.insert((0, 3, 0), 1);
    let mesh = mesh_chunk(&cells, (0, 0, 0), white, |slot| {
        if slot == 1 {
            [1.5, 0.0]
        } else {
            [0.0, 0.0]
        }
    });
    assert!(mesh
        .verts
        .iter()
        .all(|vert| vert.motion[0] == 1.5 && vert.motion[1] == 0.0));
    assert!(mesh.verts.iter().any(|vert| vert.motion[2] == 0.0));
    assert!(mesh.verts.iter().any(|vert| vert.motion[2] == 4.0));
}
