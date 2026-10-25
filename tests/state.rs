use std::path::Path;

use super::geom::*;
use super::vox::*;
use super::*;

fn chunk(id: &[u8; 4], content: &[u8]) -> Vec<u8> {
    let mut out = id.to_vec();
    out.extend_from_slice(&(content.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(content);
    out
}

#[test]
fn loads_one_voxel() {
    let mut size = Vec::new();
    size.extend_from_slice(&1u32.to_le_bytes());
    size.extend_from_slice(&1u32.to_le_bytes());
    size.extend_from_slice(&1u32.to_le_bytes());
    let mut xyzi = Vec::new();
    xyzi.extend_from_slice(&1u32.to_le_bytes());
    xyzi.extend_from_slice(&[0, 0, 0, 36]);
    let children = {
        let mut c = chunk(b"SIZE", &size);
        c.extend(chunk(b"XYZI", &xyzi));
        c
    };
    let mut file = b"VOX ".to_vec();
    file.extend_from_slice(&150u32.to_le_bytes());
    file.extend(b"MAIN");
    file.extend_from_slice(&0u32.to_le_bytes());
    file.extend_from_slice(&(children.len() as u32).to_le_bytes());
    file.extend(children);

    let loaded = load_vox(&file).expect("vox");
    assert_eq!(loaded.objects.len(), 1);
    assert_eq!(loaded.objects[0].cells.get(&(0, 0, 0)), Some(&36));
}

#[test]
fn flood_select_stops_at_a_different_color() {
    let mut cells = HashMap::new();
    cells.insert((0, 0, 0), 4);
    cells.insert((1, 0, 0), 4);
    cells.insert((2, 0, 0), 9);
    let picked = flood_same(&cells, (0, 0, 0), 4);
    assert_eq!(picked.len(), 2);
    assert!(!picked.contains(&(2, 0, 0)));
}

#[test]
fn box_select_keeps_occupied_cells_inside_the_aabb() {
    let mut cells = HashMap::new();
    cells.insert((0, 0, 0), 1);
    cells.insert((1, 0, 0), 1);
    cells.insert((3, 0, 0), 1);
    let picked = cells_in_aabb(&cells, (0, 0, 0), (1, 0, 0));
    assert!(picked.contains(&(0, 0, 0)));
    assert!(picked.contains(&(1, 0, 0)));
    assert!(!picked.contains(&(3, 0, 0)));
}

#[test]
fn rotate_selection_leaves_the_rest_of_the_volume() {
    let mut session = Session::new();
    session.active_mut().cells.clear();
    session.active_mut().cells.insert((0, 0, 0), 4);
    session.active_mut().cells.insert((1, 0, 0), 1);
    session.active_mut().cells.insert((2, 0, 0), 5);
    session.picked.insert((1, 0, 0));
    session.picked.insert((2, 0, 0));
    let before = session.voxel_count();
    session.rotate_selection(Axis::Z, true);
    assert_eq!(session.voxel_count(), before);
    assert_eq!(session.active().cells.get(&(0, 0, 0)), Some(&4));
    assert!(!session.picked.contains(&(0, 0, 0)));
    assert_eq!(session.active().cells.get(&(1, -1, 0)), Some(&5));
    assert!(session.picked.contains(&(1, -1, 0)));
}

#[test]
fn rotate_object_spins_around_its_center() {
    let mut session = Session::new();
    session.active_mut().cells.clear();
    session.active_mut().cells.insert((0, 0, 0), 1);
    session.active_mut().cells.insert((2, 0, 0), 2);
    session.active_mut().translation = (5, 0, 0);
    session.picked.insert((0, 0, 0));
    session.rotate_object(Axis::Z, false);
    assert_eq!(session.active().translation, (5, 0, 0));
    assert_eq!(session.active().cells.get(&(1, -1, 0)), Some(&1));
    assert_eq!(session.active().cells.get(&(1, 1, 0)), Some(&2));
    assert!(session.picked.is_empty());
    session.undo();
    assert_eq!(session.active().cells.get(&(0, 0, 0)), Some(&1));
    assert_eq!(session.active().cells.get(&(2, 0, 0)), Some(&2));
    assert_eq!(session.active().translation, (5, 0, 0));
}

#[test]
fn flip_object_mirrors_across_the_span_center() {
    let mut session = Session::new();
    session.active_mut().cells.clear();
    session.active_mut().cells.insert((0, 0, 0), 1);
    session.active_mut().cells.insert((1, 0, 0), 2);
    session.active_mut().translation = (4, 1, 0);
    session.flip_object(Axis::X);
    assert_eq!(session.active().translation, (4, 1, 0));
    assert_eq!(session.active().cells.get(&(2, 0, 0)), Some(&1));
    assert_eq!(session.active().cells.get(&(1, 0, 0)), Some(&2));
    session.undo();
    assert_eq!(session.active().cells.get(&(0, 0, 0)), Some(&1));
    assert_eq!(session.active().cells.get(&(1, 0, 0)), Some(&2));
}

#[test]
fn object_band_selects_the_hits_and_a_click_keeps_one() {
    let mut session = Session::new();
    session.objects.push(Volume {
        name: "B".into(),
        cells: HashMap::from([((0, 0, 0), 2)]),
        translation: (4, 0, 0),
        box_min: (0, 0, 0),
        dims: (0, 0, 0),
        rot: ROT_IDENTITY,
        pivot: (0, 0, 0),
        model_dims: (0, 0, 0),
        hidden: false,
        file: None,
        bone: None,
        script: Default::default(),
    });
    session.finish_object_band(
        &ObjectBand {
            click: false,
            hits: vec![0, 1],
        },
        false,
    );
    assert_eq!(session.object_selection, vec![0, 1]);
    assert_eq!(session.selected, 0);
    session.finish_object_band(
        &ObjectBand {
            click: true,
            hits: Vec::new(),
        },
        false,
    );
    assert_eq!(session.object_selection, vec![0]);
}

#[test]
fn object_drag_moves_every_chosen_object() {
    let mut session = Session::new();
    session.editor_mode = EditorMode::Object;
    session.active_mut().cells.clear();
    session.active_mut().cells.insert((0, 0, 0), 1);
    session.objects.push(Volume {
        name: "B".into(),
        cells: HashMap::from([((0, 0, 0), 2)]),
        translation: (5, 0, 0),
        box_min: (0, 0, 0),
        dims: (0, 0, 0),
        rot: ROT_IDENTITY,
        pivot: (0, 0, 0),
        model_dims: (0, 0, 0),
        hidden: false,
        file: None,
        bone: None,
        script: Default::default(),
    });
    session.object_selection = vec![0, 1];
    let down = (Vec3::new(0.5, 0.5, 5.0), Vec3::new(0.0, 0.0, -1.0));
    let over = (Vec3::new(2.5, 0.5, 5.0), Vec3::new(0.0, 0.0, -1.0));
    point(&mut session, true, true, false, down);
    point(&mut session, false, false, true, over);
    assert_eq!(session.objects[0].translation, (2, 0, 0));
    assert_eq!(session.objects[1].translation, (7, 0, 0));
    assert_eq!(session.object_selection, vec![0, 1]);
    session.undo();
    assert_eq!(session.objects[0].translation, (0, 0, 0));
    assert_eq!(session.objects[1].translation, (5, 0, 0));
}

#[test]
fn rotate_object_turns_the_whole_selection() {
    let mut session = Session::new();
    session.active_mut().cells.clear();
    session.active_mut().cells.insert((0, 0, 0), 1);
    session.objects.push(Volume {
        name: "B".into(),
        cells: HashMap::from([((0, 0, 0), 2)]),
        translation: (4, 0, 0),
        box_min: (0, 0, 0),
        dims: (0, 0, 0),
        rot: ROT_IDENTITY,
        pivot: (0, 0, 0),
        model_dims: (0, 0, 0),
        hidden: false,
        file: None,
        bone: None,
        script: Default::default(),
    });
    session.object_selection = vec![0, 1];
    session.rotate_object(Axis::Z, false);
    assert_eq!(session.objects[0].translation, (0, 0, 0));
    assert_eq!(session.objects[0].cells.get(&(2, -2, 0)), Some(&1));
    assert_eq!(session.objects[1].translation, (4, 0, 0));
    assert_eq!(session.objects[1].cells.get(&(-2, 2, 0)), Some(&2));
}

#[test]
fn copy_paste_keeps_unselected_neighbors() {
    let mut session = Session::new();
    session.active_mut().cells.clear();
    session.active_mut().cells.insert((0, 0, 0), 4);
    session.active_mut().cells.insert((1, 0, 0), 5);
    session.active_mut().cells.insert((2, 0, 0), 6);
    session.picked.insert((0, 0, 0));
    session.picked.insert((2, 0, 0));
    session.copy_picked();
    session.delete_picked();
    assert_eq!(session.active().cells.get(&(1, 0, 0)), Some(&5));
    assert!(!session.active().cells.contains_key(&(0, 0, 0)));
    session.paste_clipboard();
    assert_eq!(session.active().cells.get(&(0, 0, 0)), Some(&4));
    assert_eq!(session.active().cells.get(&(1, 0, 0)), Some(&5));
    assert_eq!(session.active().cells.get(&(2, 0, 0)), Some(&6));
    assert!(session.picked.contains(&(0, 0, 0)));
    assert!(session.picked.contains(&(2, 0, 0)));
    assert!(!session.picked.contains(&(1, 0, 0)));
}

#[test]
fn cut_is_one_undo_step() {
    let mut session = Session::new();
    session.active_mut().cells.clear();
    session.active_mut().cells.insert((0, 0, 0), 4);
    session.picked.insert((0, 0, 0));
    session.cut_picked();
    assert!(session.active().cells.is_empty());
    session.undo();
    assert_eq!(session.active().cells.get(&(0, 0, 0)), Some(&4));
    session.paste_clipboard();
    assert_eq!(session.active().cells.get(&(0, 0, 0)), Some(&4));
}

#[test]
fn duplicate_places_the_copy_beside_the_original() {
    let mut session = Session::new();
    session.active_mut().cells.clear();
    session.active_mut().cells.insert((0, 0, 0), 1);
    session.active_mut().cells.insert((2, 0, 0), 1);
    session.duplicate_object();
    assert_eq!(session.objects[0].translation, (0, 0, 0));
    assert_eq!(session.objects[1].translation, (4, 0, 0));
    assert_eq!(session.selected, 1);
    assert_eq!(session.objects[1].cells.len(), 2);
}

fn point(session: &mut Session, down: bool, pressed: bool, released: bool, ray: (Vec3, Vec3)) {
    session.pointer(
        true,
        down,
        pressed,
        released,
        false,
        false,
        false,
        false,
        false,
        glam::Vec2::ZERO,
        0.0,
        Some(ray),
        &ToolDrag {
            rays: Vec::new(),
            face_steps: 1,
            marquee: None,
            shift: false,
            object_band: None,
        },
        false,
        false,
    );
}

#[test]
fn alt_click_copies_the_voxel_color_and_does_not_paint() {
    let ray = (Vec3::new(0.5, 0.5, 5.0), Vec3::new(0.0, 0.0, -1.0));
    let mut session = Session::new();
    session.tool = Tool::Brush;
    session.tool_mode = ToolMode::Add;
    session.brush_size = 1;
    session.active_slot = 36;
    session.active_mut().cells.clear();
    session.active_mut().cells.insert((0, 0, 0), 7);
    session.pointer(
        true,
        true,
        true,
        false,
        false,
        false,
        false,
        false,
        false,
        glam::Vec2::ZERO,
        0.0,
        Some(ray),
        &ToolDrag {
            rays: Vec::new(),
            face_steps: 1,
            marquee: None,
            shift: false,
            object_band: None,
        },
        false,
        true,
    );
    assert_eq!(session.active_slot, 7);
    assert_eq!(session.active().cells.len(), 1);
    assert!(!session.sample_armed);

    session.active_slot = 36;
    session.sample_armed = true;
    session.pointer(
        true,
        true,
        true,
        false,
        false,
        false,
        false,
        false,
        false,
        glam::Vec2::ZERO,
        0.0,
        Some(ray),
        &ToolDrag {
            rays: Vec::new(),
            face_steps: 1,
            marquee: None,
            shift: false,
            object_band: None,
        },
        false,
        false,
    );
    assert_eq!(session.active_slot, 7);
    assert!(!session.sample_armed);
    assert_eq!(session.active().cells.len(), 1);
}

#[test]
fn a_held_brush_click_changes_one_layer() {
    // A click lasts several frames. Each one must aim at the model as it was
    // when the stroke began, not at the voxels the stroke just wrote.
    let ray = (Vec3::new(0.5, 0.5, 5.0), Vec3::new(0.0, 0.0, -1.0));
    let mut session = Session::new();
    session.tool = Tool::Brush;
    session.tool_mode = ToolMode::Add;
    session.brush_size = 1;
    session.active_mut().translation = (0, 0, 0);
    session.active_mut().cells.clear();
    session.active_mut().cells.insert((0, 0, 0), 1);
    point(&mut session, true, true, false, ray);
    for _ in 0..5 {
        point(&mut session, true, false, false, ray);
    }
    point(&mut session, false, false, true, ray);
    assert_eq!(session.active().cells.len(), 2);
    assert!(session.active().cells.contains_key(&(0, 0, 1)));
    session.undo();
    assert_eq!(session.active().cells.len(), 1);

    session.tool_mode = ToolMode::Remove;
    for z in 1..3 {
        session.active_mut().cells.insert((0, 0, z), 1);
    }
    point(&mut session, true, true, false, ray);
    for _ in 0..5 {
        point(&mut session, true, false, false, ray);
    }
    point(&mut session, false, false, true, ray);
    assert_eq!(session.active().cells.len(), 2);
    assert!(!session.active().cells.contains_key(&(0, 0, 2)));
}

#[test]
fn object_drag_slides_the_translation_and_undoes() {
    let mut session = Session::new();
    session.editor_mode = EditorMode::Object;
    session.active_mut().cells.clear();
    session.active_mut().cells.insert((0, 0, 0), 1);
    session.active_mut().translation = (0, 0, 0);
    let down = (Vec3::new(0.5, 0.5, 5.0), Vec3::new(0.0, 0.0, -1.0));
    let over = (Vec3::new(2.5, 0.5, 5.0), Vec3::new(0.0, 0.0, -1.0));
    point(&mut session, true, true, false, down);
    point(&mut session, false, false, true, over);
    assert_eq!(session.active().translation, (2, 0, 0));
    assert_eq!(session.active().cells.get(&(0, 0, 0)), Some(&1));
    session.undo();
    assert_eq!(session.active().translation, (0, 0, 0));
}

#[test]
fn resize_clips_voxels_past_the_new_size() {
    let mut session = Session::new();
    session.active_mut().cells.clear();
    session.active_mut().cells.insert((0, 0, 0), 1);
    session.active_mut().cells.insert((2, 0, 0), 2);
    assert_eq!(session.size_xyz(), Some((3, 1, 1)));
    session.resize_active(0, 2);
    assert_eq!(session.active().cells.get(&(0, 0, 0)), Some(&1));
    assert!(session.active().cells.get(&(2, 0, 0)).is_none());
    assert_eq!(session.size_xyz(), Some((2, 1, 1)));
    session.resize_active(0, 8);
    assert_eq!(session.size_xyz(), Some((8, 1, 1)));
    assert_eq!(session.active().cells.get(&(0, 0, 0)), Some(&1));
    session.undo();
    assert_eq!(session.size_xyz(), Some((2, 1, 1)));
    session.undo();
    assert_eq!(session.active().cells.get(&(2, 0, 0)), Some(&2));
    assert_eq!(session.size_xyz(), Some((3, 1, 1)));
}

#[test]
fn grown_size_survives_a_scene_save() {
    let mut session = Session::new();
    session.active_mut().cells.clear();
    session.active_mut().cells.insert((0, 0, 0), 4);
    session.resize_active(0, 5);
    let bytes = save_scene(
        &session.objects,
        &session.palette,
        &session.materials,
        &Skeleton::default(),
        &[],
    )
    .unwrap();
    let loaded = load_vox(&bytes).expect("scene");
    assert_eq!(loaded.objects[0].dims, (5, 1, 1));
    assert_eq!(loaded.objects[0].cells.get(&(0, 0, 0)), Some(&4));
    let world = (
        loaded.objects[0].cells.keys().next().copied().unwrap().0 + loaded.objects[0].translation.0,
        0,
        0,
    );
    assert_eq!(world.0, 0);
}

#[test]
fn resize_keeps_a_loaded_rotation() {
    let mut size = Vec::new();
    for dim in [2u32, 2, 2] {
        size.extend_from_slice(&dim.to_le_bytes());
    }
    let mut xyzi = Vec::new();
    xyzi.extend_from_slice(&2u32.to_le_bytes());
    xyzi.extend_from_slice(&[0, 0, 0, 3]);
    xyzi.extend_from_slice(&[0, 0, 1, 7]);
    let mut rot = Vec::new();
    push_i32(&mut rot, 0);
    push_dict(&mut rot, &[]);
    push_i32(&mut rot, 1);
    push_i32(&mut rot, -1);
    push_i32(&mut rot, -1);
    push_i32(&mut rot, 1);
    push_dict(&mut rot, &[("_r", "8")]);
    let mut shape = Vec::new();
    push_i32(&mut shape, 1);
    push_dict(&mut shape, &[]);
    push_i32(&mut shape, 1);
    push_i32(&mut shape, 0);
    push_dict(&mut shape, &[]);
    let mut children = chunk(b"SIZE", &size);
    children.extend(chunk(b"XYZI", &xyzi));
    children.extend(chunk(b"nTRN", &rot));
    children.extend(chunk(b"nSHP", &shape));
    let loaded = load_vox(&wrap_vox(children)).expect("rotated");
    let src = &loaded.objects[0];
    let mut session = Session::new();
    session.objects[0] = Volume {
        name: src.name.clone(),
        cells: src.cells.clone(),
        translation: src.translation,
        box_min: src.box_min,
        dims: src.dims,
        rot: src.rot,
        pivot: src.pivot,
        model_dims: src.model_dims,
        hidden: src.hidden,
        file: src.file.clone(),
        bone: src.bone.clone(),
        script: src.script.clone(),
    };
    let stay = src
        .cells
        .iter()
        .find(|(_, slot)| **slot == 3)
        .map(|(cell, _)| {
            (
                cell.0 + src.translation.0,
                cell.1 + src.translation.1,
                cell.2 + src.translation.2,
            )
        })
        .unwrap();
    assert_eq!(session.size_xyz(), Some((2, 2, 2)));
    session.resize_active(0, 4);
    assert_eq!(session.size_xyz(), Some((4, 2, 2)));
    assert_eq!(rot_to_byte(session.active().rot), 8);
    let moved = session
        .active()
        .cells
        .iter()
        .find(|(_, slot)| **slot == 3)
        .map(|(cell, _)| {
            (
                cell.0 + session.active().translation.0,
                cell.1 + session.active().translation.1,
                cell.2 + session.active().translation.2,
            )
        })
        .unwrap();
    assert_eq!(moved, stay);
    let bytes = save_scene(
        &session.objects,
        &session.palette,
        &session.materials,
        &Skeleton::default(),
        &[],
    )
    .unwrap();
    let again = load_vox(&bytes).expect("saved");
    assert_eq!(again.objects[0].model_dims, (4, 2, 2));
    assert_eq!(rot_to_byte(again.objects[0].rot), 8);
    session.undo();
    assert_eq!(session.size_xyz(), Some((2, 2, 2)));
    session.resize_active(2, 1);
    assert!(session.active().cells.values().all(|slot| *slot != 7));
    assert_eq!(rot_to_byte(session.active().rot), 8);
    let bytes = save_scene(
        &session.objects,
        &session.palette,
        &session.materials,
        &Skeleton::default(),
        &[],
    )
    .unwrap();
    let shrunk = load_vox(&bytes).expect("shrunk");
    assert_eq!(rot_to_byte(shrunk.objects[0].rot), 8);
    assert_eq!(shrunk.objects[0].model_dims.2, 1);
}

#[test]
fn pick_object_chooses_the_volume_under_the_ray() {
    let mut session = Session::new();
    session.active_mut().cells.clear();
    session.active_mut().cells.insert((0, 0, 0), 1);
    session.duplicate_object();
    let origin = Vec3::new(0.5, 0.5, 5.0);
    let dir = Vec3::new(0.0, 0.0, -1.0);
    assert_eq!(session.pick_object(origin, dir).unwrap().0, 0);
    let origin = Vec3::new(2.5, 0.5, 5.0);
    assert_eq!(session.pick_object(origin, dir).unwrap().0, 1);
}

#[test]
fn palette_range_selects_the_rectangle() {
    let mut session = Session::new();
    session.select_palette_range(0, 31, 1, 30, false);
    assert!(session.palette_slots.contains(&1));
    assert!(session.palette_slots.contains(&2));
    assert!(session.palette_slots.contains(&9));
    assert!(session.palette_slots.contains(&10));
    assert_eq!(session.palette_slots.len(), 4);
    assert_eq!(session.active_slot, 10);
}

#[test]
fn paste_color_and_gradient_undo() {
    let mut session = Session::new();
    session.palette[1] = [0, 0, 0, 255];
    session.palette[2] = [0, 0, 0, 255];
    session.palette[3] = [255, 0, 0, 255];
    session.palette_slots.clear();
    session.palette_slots.insert(1);
    session.palette_slots.insert(2);
    session.palette_slots.insert(3);
    session.copy_palette_color(3);
    session.paste_palette_color(1);
    assert_eq!(session.palette[1][0], 255);
    assert_eq!(session.palette[2][0], 255);
    session.undo();
    assert_eq!(session.palette[1][0], 0);
    assert_eq!(session.palette[2][0], 0);
    session.make_palette_gradient(1);
    assert_eq!(session.palette[2][0], 128);
    assert_eq!(session.palette[1][0], 0);
    assert_eq!(session.palette[3][0], 255);
}

#[test]
fn select_voxels_by_color_matches_the_chosen_slots() {
    let mut session = Session::new();
    session.active_mut().cells.clear();
    session.active_mut().cells.insert((0, 0, 0), 1);
    session.active_mut().cells.insert((1, 0, 0), 2);
    session.active_mut().cells.insert((2, 0, 0), 1);
    session.palette_slots.clear();
    session.palette_slots.insert(1);
    session.select_voxels_by_color(1);
    assert!(session.picked.contains(&(0, 0, 0)));
    assert!(session.picked.contains(&(2, 0, 0)));
    assert!(!session.picked.contains(&(1, 0, 0)));
    session.palette_slots.clear();
    session.palette_slots.insert(9);
    session.paint_picked_with_color(9);
    assert_eq!(session.active().cells.get(&(0, 0, 0)), Some(&9));
    assert_eq!(session.active().cells.get(&(2, 0, 0)), Some(&9));
    session.undo();
    assert_eq!(session.active().cells.get(&(0, 0, 0)), Some(&1));
    session.picked.insert((0, 0, 0));
    session.picked.insert((2, 0, 0));
    session.palette_slots.clear();
    session.palette_slots.insert(1);
    session.palette_slots.insert(2);
    session.paint_picked_with_random_colors(1);
    let a = session.active().cells[&(0, 0, 0)];
    let b = session.active().cells[&(2, 0, 0)];
    assert!(a == 1 || a == 2);
    assert!(b == 1 || b == 2);
}

#[test]
fn paste_lands_on_the_hovered_face() {
    let mut session = Session::new();
    session.active_mut().cells.clear();
    session.active_mut().cells.insert((0, 0, 0), 4);
    session.active_mut().cells.insert((1, 0, 0), 5);
    session.picked.insert((0, 0, 0));
    session.picked.insert((1, 0, 0));
    session.copy_picked();
    session.hover = Some(Hit {
        cell: (5, 0, 0),
        normal: (0, 0, 1),
    });
    session.paste_clipboard();
    assert_eq!(session.active().cells.get(&(5, 0, 1)), Some(&4));
    assert_eq!(session.active().cells.get(&(6, 0, 1)), Some(&5));
    assert_eq!(session.active().cells.get(&(0, 0, 0)), Some(&4));
    assert!(session.picked.contains(&(5, 0, 1)));
    assert!(!session.picked.contains(&(0, 0, 0)));
}

#[test]
fn paste_without_a_hover_uses_the_original_origin() {
    let mut session = Session::new();
    session.active_mut().cells.clear();
    session.active_mut().cells.insert((2, 3, 4), 7);
    session.picked.insert((2, 3, 4));
    session.copy_picked();
    session.active_mut().cells.insert((2, 3, 4), 1);
    session.hover = None;
    session.paste_clipboard();
    assert_eq!(session.active().cells.get(&(2, 3, 4)), Some(&7));
}

#[test]
fn move_slides_on_the_clicked_plane_and_undoes() {
    let mut session = Session::new();
    session.active_mut().cells.clear();
    session.active_mut().cells.insert((0, 0, 0), 4);
    session.active_mut().cells.insert((3, 0, 0), 9);
    session.picked.insert((0, 0, 0));
    let down = (Vec3::new(0.5, 0.5, 5.0), Vec3::new(0.0, 0.0, -1.0));
    let empty = ToolDrag {
        rays: Vec::new(),
        face_steps: 1,
        marquee: None,
        shift: false,
        object_band: None,
    };
    session.pointer(
        true,
        true,
        true,
        false,
        false,
        false,
        false,
        false,
        false,
        glam::Vec2::ZERO,
        0.0,
        Some(down),
        &empty,
        true,
        false,
    );
    let shifted = (Vec3::new(2.6, 0.4, 5.0), Vec3::new(0.0, 0.0, -1.0));
    session.pointer(
        true,
        true,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        glam::Vec2::ZERO,
        0.0,
        Some(shifted),
        &empty,
        true,
        false,
    );
    assert_eq!(session.active().cells.get(&(2, 0, 0)), Some(&4));
    assert!(!session.active().cells.contains_key(&(0, 0, 0)));
    assert_eq!(session.active().cells.get(&(3, 0, 0)), Some(&9));
    session.pointer(
        true,
        false,
        false,
        true,
        false,
        false,
        false,
        false,
        false,
        glam::Vec2::ZERO,
        0.0,
        Some(shifted),
        &empty,
        true,
        false,
    );
    session.undo();
    assert_eq!(session.active().cells.get(&(0, 0, 0)), Some(&4));
    assert!(!session.active().cells.contains_key(&(2, 0, 0)));
}

#[test]
fn plane_delta_locks_the_clicked_axis() {
    let delta = plane_delta(2, Vec3::new(0.5, 0.5, 1.0), Vec3::new(2.4, -0.6, 1.0));
    assert_eq!(delta, (2, -1, 0));
}

#[test]
fn paste_overwrites_the_cell_underneath() {
    let mut session = Session::new();
    session.active_mut().cells.clear();
    session.active_mut().cells.insert((0, 0, 0), 4);
    session.picked.insert((0, 0, 0));
    session.copy_picked();
    session.active_mut().cells.insert((0, 0, 0), 9);
    session.paste_clipboard();
    assert_eq!(session.active().cells.get(&(0, 0, 0)), Some(&4));
    session.undo();
    assert_eq!(session.active().cells.get(&(0, 0, 0)), Some(&9));
}

#[test]
fn delete_picked_removes_only_the_selection() {
    let mut session = Session::new();
    session.active_mut().cells.clear();
    session.active_mut().cells.insert((0, 0, 0), 4);
    session.active_mut().cells.insert((1, 0, 0), 5);
    session.picked.insert((1, 0, 0));
    session.delete_picked();
    assert_eq!(session.active().cells.get(&(0, 0, 0)), Some(&4));
    assert!(!session.active().cells.contains_key(&(1, 0, 0)));
    assert!(session.picked.is_empty());
    session.undo();
    assert_eq!(session.active().cells.get(&(1, 0, 0)), Some(&5));
}

#[test]
fn line_drag_adds_voxels_and_undoes() {
    let mut session = Session::new();
    let before = session.voxel_count();
    session.tool = Tool::Line;
    session.tool_mode = ToolMode::Add;
    session.checkpoint();
    let dir = Vec3::new(0.0, 1.0, -0.35);
    session.preview_tool(Vec3::new(0.5, -30.0, 10.0), dir, &[], 1);
    session.preview_tool(Vec3::new(5.5, -30.0, 10.0), dir, &[], 1);
    assert!(session.voxel_count() > before);
    session.stroke = None;
    session.commit();
    session.undo();
    assert_eq!(session.voxel_count(), before);
}

#[test]
fn brush_undo_restores_the_previous_volume() {
    let mut session = Session::new();
    let before = session.voxel_count();
    session.checkpoint();
    session.stamp(Hit {
        cell: (0, 0, 0),
        normal: (0, 0, 1),
    });
    assert!(session.voxel_count() >= before);
    session.commit();
    session.undo();
    assert_eq!(session.voxel_count(), before);
}

fn at_origin(cells: &HashMap<Cell, u8>) -> HashMap<Cell, u8> {
    if cells.is_empty() {
        return HashMap::new();
    }
    let mut min = *cells.keys().next().unwrap();
    for cell in cells.keys() {
        min.0 = min.0.min(cell.0);
        min.1 = min.1.min(cell.1);
        min.2 = min.2.min(cell.2);
    }
    cells
        .iter()
        .map(|(cell, index)| ((cell.0 - min.0, cell.1 - min.1, cell.2 - min.2), *index))
        .collect()
}

fn sample() -> (HashMap<Cell, u8>, [[u8; 4]; 256], [Material; 256]) {
    let mut cells = HashMap::new();
    cells.insert((5, -2, 3), 36);
    cells.insert((7, -2, 4), 10);
    cells.insert((5, 0, 3), 20);
    let mut palette = default_palette();
    palette[36] = [11, 22, 33, 255];
    palette[10] = [200, 10, 10, 255];
    palette[20] = [0, 180, 40, 255];
    let mut materials = [Material::default(); 256];
    materials[36] = Material {
        ty: MaterialType::Metal,
        roughness: 0.25,
        metallic: 0.5,
        ..Material::default()
    };
    materials[10] = Material {
        ty: MaterialType::Glass,
        roughness: 0.5,
        transparency: 0.25,
        ior: 2.0,
        ..Material::default()
    };
    materials[20] = Material {
        ty: MaterialType::Emissive,
        emission: 0.5,
        emission_power: 3,
        ..Material::default()
    };
    (cells, palette, materials)
}

#[test]
fn save_round_trips_voxels_palette_and_materials() {
    let (cells, palette, materials) = sample();
    let bytes = save_vox(&cells, &palette, &materials).expect("save");
    let again = save_vox(&cells, &palette, &materials).expect("save");
    assert_eq!(bytes, again);
    assert!(bytes.windows(4).any(|w| w == b"MATL"));

    let loaded = load_vox(&bytes).expect("load");
    assert_eq!(at_origin(&loaded.objects[0].cells), at_origin(&cells));
    assert_eq!(loaded.palette.unwrap()[36], [11, 22, 33, 255]);
    assert_eq!(loaded.palette.unwrap()[10], [200, 10, 10, 255]);
    assert_eq!(loaded.materials[36], materials[36]);
    assert_eq!(loaded.materials[10], materials[10]);
    assert_eq!(loaded.materials[20], materials[20]);
    assert_eq!(loaded.materials[1], Material::default());

    let defaults = save_vox(&cells, &default_palette(), &[Material::default(); 256]).unwrap();
    assert!(!defaults.windows(4).any(|w| w == b"MATL"));

    if let Ok(path) = std::env::var("FROXEL_SAMPLE_VOX") {
        std::fs::write(path, &bytes).unwrap();
    }
}

#[test]
fn save_refuses_empty_and_oversized_volumes() {
    let palette = default_palette();
    let materials = [Material::default(); 256];
    let err = save_vox(&HashMap::new(), &palette, &materials).unwrap_err();
    assert!(err.contains("empty"));

    let mut wide = HashMap::new();
    wide.insert((0, 0, 0), 1);
    wide.insert((256, 0, 0), 1);
    let err = save_vox(&wide, &palette, &materials).unwrap_err();
    assert!(err.contains("256"));
}

#[test]
fn undo_survives_a_save() {
    let mut session = Session::new();
    let before = session.voxel_count();
    session.checkpoint();
    session.stamp(Hit {
        cell: (0, 0, 0),
        normal: (0, 0, 1),
    });
    session.commit();
    let path = std::env::temp_dir().join("froxel-undo.vox");
    session.write_vox(&path).unwrap();
    assert!(session.status.starts_with("Saved "));
    session.undo();
    assert_eq!(session.voxel_count(), before);
    let _ = std::fs::remove_file(path);
}

#[test]
fn unsaved_changes_follow_the_last_save() {
    let mut session = Session::new();
    assert!(!session.has_unsaved_changes());
    let hit = Hit {
        cell: (0, 0, 0),
        normal: (0, 0, 1),
    };
    session.checkpoint();
    session.stamp(hit);
    assert!(session.has_unsaved_changes());
    session.undo();
    assert!(!session.has_unsaved_changes());

    session.checkpoint();
    session.stamp(hit);
    let path = std::env::temp_dir().join("froxel-unsaved.vox");
    session.write_vox(&path).unwrap();
    assert!(!session.has_unsaved_changes());
    session.undo();
    assert!(session.has_unsaved_changes());
    session.redo();
    assert!(!session.has_unsaved_changes());

    session
        .pose
        .insert("Preview".into(), crate::rig::BonePose::IDENTITY);
    assert!(!session.has_unsaved_changes());

    session.new_document();
    assert!(!session.has_unsaved_changes());
    session.load_path(&path);
    assert!(!session.has_unsaved_changes());
    let _ = std::fs::remove_file(path);
}

fn world_of(object: &LoadedObject, color: u8) -> Cell {
    let (cell, _) = object
        .cells
        .iter()
        .find(|(_, slot)| **slot == color)
        .expect("color");
    (
        cell.0 + object.translation.0,
        cell.1 + object.translation.1,
        cell.2 + object.translation.2,
    )
}

#[test]
fn scene_save_keeps_each_object_in_place() {
    let mut first = HashMap::new();
    first.insert((0, 0, 0), 1);
    let mut second = HashMap::new();
    second.insert((0, 0, 0), 2);
    let objects = vec![
        Volume {
            name: "A".into(),
            cells: first,
            translation: (0, 0, 0),
            box_min: (0, 0, 0),
            dims: (0, 0, 0),
            rot: ROT_IDENTITY,
            pivot: (0, 0, 0),
            model_dims: (0, 0, 0),
            hidden: false,
            file: None,
            bone: None,
            script: Default::default(),
        },
        Volume {
            name: "B".into(),
            cells: second,
            translation: (5, 0, 0),
            box_min: (0, 0, 0),
            dims: (0, 0, 0),
            rot: ROT_IDENTITY,
            pivot: (0, 0, 0),
            model_dims: (0, 0, 0),
            hidden: false,
            file: None,
            bone: None,
            script: Default::default(),
        },
    ];
    let bytes = save_scene(
        &objects,
        &default_palette(),
        &[Material::default(); 256],
        &Skeleton::default(),
        &[],
    )
    .unwrap();
    let loaded = load_vox(&bytes).expect("scene");
    assert_eq!(loaded.objects.len(), 2);
    assert_eq!(loaded.objects[0].name, "A");
    assert_eq!(loaded.objects[1].name, "B");
    assert_eq!(world_of(&loaded.objects[0], 1), (0, 0, 0));
    assert_eq!(world_of(&loaded.objects[1], 2), (5, 0, 0));
}

fn volume_from(object: &LoadedObject) -> Volume {
    let mut file = object.file.clone();
    if let Some(link) = file.as_mut() {
        link.name = object.name.clone();
    }
    Volume {
        name: object.name.clone(),
        cells: object.cells.clone(),
        translation: object.translation,
        box_min: object.box_min,
        dims: object.dims,
        rot: object.rot,
        pivot: object.pivot,
        model_dims: object.model_dims,
        hidden: object.hidden,
        file,
        bone: object.bone.clone(),
        script: object.script.clone(),
    }
}

#[test]
fn scene_graph_keeps_hidden_layers_frames_and_groups() {
    let mut size = Vec::new();
    for _ in 0..3 {
        size.extend_from_slice(&1u32.to_le_bytes());
    }
    let mut xyzi_a = Vec::new();
    xyzi_a.extend_from_slice(&1u32.to_le_bytes());
    xyzi_a.extend_from_slice(&[0, 0, 0, 1]);
    let mut xyzi_b = Vec::new();
    xyzi_b.extend_from_slice(&1u32.to_le_bytes());
    xyzi_b.extend_from_slice(&[0, 0, 0, 2]);

    let mut root = Vec::new();
    push_i32(&mut root, 0);
    push_dict(&mut root, &[]);
    push_i32(&mut root, 1);
    push_i32(&mut root, -1);
    push_i32(&mut root, -1);
    push_i32(&mut root, 1);
    push_dict(&mut root, &[]);

    let mut group = Vec::new();
    push_i32(&mut group, 1);
    push_dict(&mut group, &[("_name", "Group")]);
    push_i32(&mut group, 2);
    push_i32(&mut group, 2);
    push_i32(&mut group, 4);

    let mut hidden = Vec::new();
    push_i32(&mut hidden, 2);
    push_dict(&mut hidden, &[("_name", "Hidden"), ("_hidden", "1")]);
    push_i32(&mut hidden, 3);
    push_i32(&mut hidden, -1);
    push_i32(&mut hidden, -1);
    push_i32(&mut hidden, 2);
    push_dict(&mut hidden, &[]);
    push_dict(&mut hidden, &[("_t", "3 0 0"), ("_f", "1")]);

    let mut shown = Vec::new();
    push_i32(&mut shown, 4);
    push_dict(&mut shown, &[("_name", "Shown")]);
    push_i32(&mut shown, 5);
    push_i32(&mut shown, -1);
    push_i32(&mut shown, -1);
    push_i32(&mut shown, 1);
    push_dict(&mut shown, &[("_t", "6 0 0")]);

    let mut shape_a = Vec::new();
    push_i32(&mut shape_a, 3);
    push_dict(&mut shape_a, &[]);
    push_i32(&mut shape_a, 1);
    push_i32(&mut shape_a, 0);
    push_dict(&mut shape_a, &[]);

    let mut shape_b = Vec::new();
    push_i32(&mut shape_b, 5);
    push_dict(&mut shape_b, &[]);
    push_i32(&mut shape_b, 1);
    push_i32(&mut shape_b, 1);
    push_dict(&mut shape_b, &[]);

    let mut layer = Vec::new();
    push_i32(&mut layer, 0);
    push_dict(&mut layer, &[("_name", "Layer")]);
    push_i32(&mut layer, -1);

    let mut children = chunk(b"SIZE", &size);
    children.extend(chunk(b"XYZI", &xyzi_a));
    children.extend(chunk(b"SIZE", &size));
    children.extend(chunk(b"XYZI", &xyzi_b));
    children.extend(chunk(b"nTRN", &root));
    children.extend(chunk(b"nGRP", &group));
    children.extend(chunk(b"nTRN", &hidden));
    children.extend(chunk(b"nSHP", &shape_a));
    children.extend(chunk(b"nTRN", &shown));
    children.extend(chunk(b"nSHP", &shape_b));
    children.extend(chunk(b"LAYR", &layer));

    let loaded = load_vox(&wrap_vox(children)).expect("graph");
    assert_eq!(loaded.objects.len(), 2);
    assert!(loaded.objects[0].hidden);
    assert!(!loaded.objects[1].hidden);
    assert_eq!(loaded.objects[0].name, "Hidden");
    assert_eq!(loaded.objects[1].name, "Shown");

    let volumes: Vec<Volume> = loaded.objects.iter().map(volume_from).collect();
    let bytes = save_document(
        &volumes,
        &default_palette(),
        &[Material::default(); 256],
        loaded.graph.as_ref(),
        &Skeleton::default(),
        &[],
    )
    .unwrap();
    assert!(bytes.windows(4).any(|window| window == b"LAYR"));
    let again = load_vox(&bytes).expect("resave");
    assert!(again.objects[0].hidden);
    assert!(!again.objects[1].hidden);
    assert_eq!(
        world_of(&again.objects[1], 2).0 - world_of(&again.objects[0], 1).0,
        6
    );
    let graph = again.graph.expect("graph");
    let hidden_node = graph.nodes.iter().find(|node| node.id == 2).unwrap();
    assert_eq!(hidden_node.frames.len(), 2);
    assert_eq!(pair(&hidden_node.frames[1], "_t"), Some("3 0 0"));
    assert_eq!(pair(&hidden_node.frames[1], "_f"), Some("1"));
    assert!(attr_hidden(&hidden_node.attrs));
    let group = graph
        .nodes
        .iter()
        .find(|node| node.kind == VoxKind::Group)
        .unwrap();
    assert_eq!(group.children, vec![2, 4]);
    assert_eq!(pair(&graph.layers[0].attrs, "_name"), Some("Layer"));

    let mut moved = volumes;
    moved[1].translation.0 += 2;
    let bytes = save_document(
        &moved,
        &default_palette(),
        &[Material::default(); 256],
        loaded.graph.as_ref(),
        &Skeleton::default(),
        &[],
    )
    .unwrap();
    let shifted = load_vox(&bytes).expect("moved");
    let shown = shifted
        .graph
        .unwrap()
        .nodes
        .into_iter()
        .find(|node| node.id == 4)
        .unwrap();
    assert_eq!(pair(&shown.frames[0], "_t"), Some("8 0 0"));
    assert_eq!(
        world_of(&shifted.objects[1], 2).0 - world_of(&shifted.objects[0], 1).0,
        8
    );
}

#[test]
fn scene_rotation_is_baked_into_the_cells() {
    let mut size = Vec::new();
    for dim in [2u32, 2, 2] {
        size.extend_from_slice(&dim.to_le_bytes());
    }
    let mut xyzi = Vec::new();
    xyzi.extend_from_slice(&2u32.to_le_bytes());
    xyzi.extend_from_slice(&[0, 0, 0, 3]);
    xyzi.extend_from_slice(&[0, 0, 1, 7]);
    let mut rot = Vec::new();
    push_i32(&mut rot, 0);
    push_dict(&mut rot, &[]);
    push_i32(&mut rot, 1);
    push_i32(&mut rot, -1);
    push_i32(&mut rot, -1);
    push_i32(&mut rot, 1);
    push_dict(&mut rot, &[("_r", "8")]);
    let mut shape = Vec::new();
    push_i32(&mut shape, 1);
    push_dict(&mut shape, &[]);
    push_i32(&mut shape, 1);
    push_i32(&mut shape, 0);
    push_dict(&mut shape, &[]);
    let mut children = chunk(b"SIZE", &size);
    children.extend(chunk(b"XYZI", &xyzi));
    children.extend(chunk(b"nTRN", &rot));
    children.extend(chunk(b"nSHP", &shape));
    let file = wrap_vox(children);
    let loaded = load_vox(&file).expect("rotated");
    let object = &loaded.objects[0];
    let low = object.cells.iter().find(|(_, slot)| **slot == 3).unwrap().0;
    let high = object.cells.iter().find(|(_, slot)| **slot == 7).unwrap().0;
    assert_eq!((high.0 - low.0, high.1 - low.1, high.2 - low.2), (0, 1, 0));
    assert_eq!(rot_to_byte(object.rot), 8);
    let bytes = save_scene(
        &[Volume {
            name: object.name.clone(),
            cells: object.cells.clone(),
            translation: object.translation,
            box_min: object.box_min,
            dims: object.dims,
            rot: object.rot,
            pivot: object.pivot,
            model_dims: object.model_dims,
            hidden: object.hidden,
            file: object.file.clone(),
            bone: object.bone.clone(),
            script: object.script.clone(),
        }],
        &default_palette(),
        &[Material::default(); 256],
        &Skeleton::default(),
        &[],
    )
    .unwrap();
    let again = load_vox(&bytes).expect("resaved rotation");
    let saved = &again.objects[0];
    assert_eq!(rot_to_byte(saved.rot), 8);
    let low = saved.cells.iter().find(|(_, slot)| **slot == 3).unwrap().0;
    let high = saved.cells.iter().find(|(_, slot)| **slot == 7).unwrap().0;
    assert_eq!((high.0 - low.0, high.1 - low.1, high.2 - low.2), (0, 1, 0));
}

fn world_cells(object: &LoadedObject) -> Vec<(Cell, u8)> {
    let mut cells: Vec<_> = object
        .cells
        .iter()
        .map(|(cell, slot)| {
            (
                (
                    cell.0 + object.translation.0,
                    cell.1 + object.translation.1,
                    cell.2 + object.translation.2,
                ),
                *slot,
            )
        })
        .collect();
    cells.sort_unstable();
    cells
}

fn local_bounds(object: &LoadedObject) -> (Cell, Cell) {
    let mut keys = object.cells.keys().copied();
    let first = keys.next().expect("cells");
    let mut min = first;
    let mut max = first;
    for cell in keys {
        min.0 = min.0.min(cell.0);
        min.1 = min.1.min(cell.1);
        min.2 = min.2.min(cell.2);
        max.0 = max.0.max(cell.0);
        max.1 = max.1.max(cell.1);
        max.2 = max.2.max(cell.2);
    }
    (min, max)
}

#[test]
fn corner_position_round_trips_odd_and_even_sizes() {
    let mut anchor = HashMap::new();
    anchor.insert((0, 0, 0), 1);
    let mut even = HashMap::new();
    even.insert((0, 0, 0), 2);
    even.insert((1, 1, 0), 3);
    let mut odd = HashMap::new();
    odd.insert((0, 0, 0), 4);
    odd.insert((0, 1, 0), 5);
    odd.insert((0, 2, 0), 6);
    let objects = vec![
        bare_volume("anchor", anchor, (0, 0, 0)),
        bare_volume("even", even, (6, 2, 0)),
        bare_volume("odd", odd, (2, 0, 3)),
    ];
    let before: Vec<_> = objects
        .iter()
        .map(|object| {
            (
                object.name.clone(),
                object.translation,
                local_bounds_volume(object),
                world_cells_volume(object),
            )
        })
        .collect();
    let bytes = save_scene(
        &objects,
        &default_palette(),
        &[Material::default(); 256],
        &Skeleton::default(),
        &[],
    )
    .unwrap();
    let loaded = load_vox(&bytes).expect("scene");
    assert_eq!(loaded.objects.len(), 3);
    for (object, (name, translation, bounds, world)) in loaded.objects.iter().zip(before) {
        assert_eq!(object.name, name);
        assert_eq!(object.translation, translation);
        assert_eq!(local_bounds(object), bounds);
        assert_eq!(world_cells(object), world);
    }
}

#[test]
fn rotated_object_round_trips_world_cells() {
    let mut size = Vec::new();
    for dim in [2u32, 2, 2] {
        size.extend_from_slice(&dim.to_le_bytes());
    }
    let mut xyzi = Vec::new();
    xyzi.extend_from_slice(&2u32.to_le_bytes());
    xyzi.extend_from_slice(&[0, 0, 0, 3]);
    xyzi.extend_from_slice(&[0, 0, 1, 7]);
    let mut rot = Vec::new();
    push_i32(&mut rot, 0);
    push_dict(&mut rot, &[("_name", "spun")]);
    push_i32(&mut rot, 1);
    push_i32(&mut rot, -1);
    push_i32(&mut rot, -1);
    push_i32(&mut rot, 1);
    push_dict(&mut rot, &[("_t", "0 0 4"), ("_r", "8")]);
    let mut shape = Vec::new();
    push_i32(&mut shape, 1);
    push_dict(&mut shape, &[]);
    push_i32(&mut shape, 1);
    push_i32(&mut shape, 0);
    push_dict(&mut shape, &[]);
    let mut children = chunk(b"SIZE", &size);
    children.extend(chunk(b"XYZI", &xyzi));
    children.extend(chunk(b"nTRN", &rot));
    children.extend(chunk(b"nSHP", &shape));
    let loaded = load_vox(&wrap_vox(children)).expect("rotated");
    let object = &loaded.objects[0];
    assert_eq!(object.cells.get(&(0, 0, 0)), Some(&3));
    let world = world_cells(object);
    let bytes = save_scene(
        &[volume_from(object)],
        &default_palette(),
        &[Material::default(); 256],
        &Skeleton::default(),
        &[],
    )
    .unwrap();
    let again = load_vox(&bytes).expect("resaved");
    assert_eq!(world_cells(&again.objects[0]), world);
    assert_eq!(rot_to_byte(again.objects[0].rot), 8);
}

#[test]
fn magicavoxel_fixture_keeps_world_cells() {
    let bytes = include_bytes!("fixtures/magicavoxel.vox");
    let loaded = load_vox(bytes).expect("fixture");
    assert_eq!(loaded.objects.len(), 3);
    let mut by_name = HashMap::new();
    for object in &loaded.objects {
        by_name.insert(object.name.as_str(), object);
    }
    let even = by_name["even"];
    let odd = by_name["odd"];
    let spun = by_name["spun"];
    assert_eq!(world_cells(even), vec![((0, -1, 0), 1), ((1, 0, 1), 2)]);
    assert_eq!(
        world_cells(odd),
        vec![((7, 0, 1), 3), ((7, 1, 1), 4), ((7, 2, 1), 5)]
    );
    assert_eq!(world_cells(spun), vec![((-4, -1, 2), 8), ((-4, 0, 2), 9)]);
    assert_eq!(even.cells.get(&(0, 0, 0)), Some(&1));
    assert_eq!(odd.cells.get(&(0, 0, 0)), Some(&3));
    assert_eq!(odd.translation, (7, 0, 1));
    assert_eq!(local_bounds(odd).0 .1, 0);
    assert_eq!(local_bounds(odd).1 .1, 2);
    assert_eq!(spun.cells.get(&(0, 0, 0)), Some(&8));
}

fn bare_volume(name: &str, cells: HashMap<Cell, u8>, translation: Cell) -> Volume {
    Volume {
        name: name.into(),
        cells,
        translation,
        box_min: (0, 0, 0),
        dims: (0, 0, 0),
        rot: ROT_IDENTITY,
        pivot: (0, 0, 0),
        model_dims: (0, 0, 0),
        hidden: false,
        file: None,
        bone: None,
        script: Default::default(),
    }
}

fn local_bounds_volume(object: &Volume) -> (Cell, Cell) {
    let mut keys = object.cells.keys().copied();
    let first = keys.next().expect("cells");
    let mut min = first;
    let mut max = first;
    for cell in keys {
        min.0 = min.0.min(cell.0);
        min.1 = min.1.min(cell.1);
        min.2 = min.2.min(cell.2);
        max.0 = max.0.max(cell.0);
        max.1 = max.1.max(cell.1);
        max.2 = max.2.max(cell.2);
    }
    (min, max)
}

fn world_cells_volume(object: &Volume) -> Vec<(Cell, u8)> {
    let mut cells: Vec<_> = object
        .cells
        .iter()
        .map(|(cell, slot)| {
            (
                (
                    cell.0 + object.translation.0,
                    cell.1 + object.translation.1,
                    cell.2 + object.translation.2,
                ),
                *slot,
            )
        })
        .collect();
    cells.sort_unstable();
    cells
}

/// After `Froxel voxrt --output` resaves a sample, the geometry,
/// palette, and materials must still match. Skipped unless
/// `FROXEL_RESAVE_VOX` points at that file.
#[test]
fn froxel_resave_keeps_the_sample() {
    let Ok(path) = std::env::var("FROXEL_RESAVE_VOX") else {
        return;
    };
    let (cells, palette, materials) = sample();
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let loaded = load_vox(&bytes).expect("resave");
    assert_eq!(at_origin(&loaded.objects[0].cells), at_origin(&cells));
    let loaded_palette = loaded.palette.expect("rgba");
    assert_eq!(loaded_palette[36], palette[36]);
    assert_eq!(loaded_palette[10], palette[10]);
    assert_eq!(loaded_palette[20], palette[20]);
    assert_eq!(loaded.materials[36], materials[36]);
    assert_eq!(loaded.materials[10], materials[10]);
    assert_eq!(loaded.materials[20], materials[20]);
}

#[test]
fn face_click_on_an_empty_wall_paints_empty_cells() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    for x in 0..4 {
        for y in 0..4 {
            session.objects[0].cells.insert((x, y, 0), 1);
        }
    }
    session.objects[0].cells.insert((0, 0, 4), 2);
    session.tool = Tool::Face;
    session.tool_mode = ToolMode::Add;
    session.active_slot = 9;
    session.editor_mode = EditorMode::Model;
    session.render_mode = RenderMode::Edit;
    let ray = (Vec3::new(1.5, -5.0, 2.5), Vec3::new(0.0, 1.0, 0.0));
    session.click_viewport(ray);
    assert_eq!(session.active().cells.get(&(1, 3, 2)), Some(&9));
    assert_eq!(session.active().cells.get(&(1, 0, 0)), Some(&1));
    assert_eq!(session.active().cells.get(&(0, 0, 4)), Some(&2));
}

#[test]
fn recent_paths_keep_the_newest_ten() {
    let mut list = Vec::new();
    for i in 0..12 {
        remember(&mut list, Path::new(&format!("/tmp/m{i}.vox")));
    }
    assert_eq!(list.len(), 10);
    assert_eq!(list[0], PathBuf::from("/tmp/m11.vox"));
    remember(&mut list, Path::new("/tmp/m5.vox"));
    assert_eq!(list[0], PathBuf::from("/tmp/m5.vox"));
    assert_eq!(
        list.iter().filter(|path| path.ends_with("m5.vox")).count(),
        1
    );
}

#[test]
fn ui_click_selects_the_registered_tool() {
    let mut session = Session::new();
    session.note_ui(
        "viewport",
        "Viewport",
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 100.0)),
        UiAction::Viewport,
    );
    session.note_ui(
        "tool.line",
        "Line",
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(20.0, 20.0)),
        UiAction::Tool(Tool::Line),
    );
    let hit = session.ui_target(5.0, 5.0).unwrap();
    assert_eq!(hit.id, "tool.line");
    session.activate_ui(&hit, None).unwrap();
    assert!(session.tool == Tool::Line);
    let described = session.describe_ui();
    assert_eq!(described["tool"], "Line");
    assert_eq!(described["regions"][1]["id"], "tool.line");
}

#[test]
fn palette_click_selects_the_slot_and_a_slider_uses_the_track() {
    let mut session = Session::new();
    session.note_ui(
        "palette.4",
        "4",
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(14.0, 14.0)),
        UiAction::Palette(4),
    );
    let hit = session.ui_target(2.0, 2.0).unwrap();
    session.activate_ui(&hit, None).unwrap();
    assert_eq!(session.active_slot, 4);
    session.note_ui(
        "slider.brush-size",
        "Size",
        egui::Rect::from_min_size(egui::pos2(0.0, 20.0), egui::vec2(100.0, 12.0)),
        UiAction::Slider(SliderSpec {
            target: SliderTarget::BrushSize,
            min: 1.0,
            max: 63.0,
        }),
    );
    let hit = session.ui_target(99.0, 22.0).unwrap();
    session.activate_ui(&hit, Some((99.0, 22.0))).unwrap();
    assert_eq!(session.brush_size, 62);
}

#[test]
fn motion_round_trips_in_matl_and_a_still_file_is_unchanged() {
    let (cells, palette, mut materials) = sample();
    let still = save_vox(&cells, &palette, &materials).expect("save");
    assert!(
        !still.windows(7).any(|window| window == b"_motion"),
        "a still material writes no motion keys"
    );
    let loaded = load_vox(&still).expect("load");
    let again = save_vox(&cells, &palette, &loaded.materials).expect("save");
    assert_eq!(still, again);

    materials[36].motion = Motion::Sway;
    materials[36].motion_amount = 1.5;
    materials[10].motion = Motion::Bob;
    materials[10].motion_amount = 2.0;
    let bytes = save_vox(&cells, &palette, &materials).expect("save");
    let loaded = load_vox(&bytes).expect("load");
    assert_eq!(loaded.materials[36].motion, Motion::Sway);
    assert_eq!(loaded.materials[36].motion_amount, 1.5);
    assert_eq!(loaded.materials[10].motion, Motion::Bob);
    assert_eq!(loaded.materials[10].motion_amount, 2.0);
    assert_eq!(loaded.materials[20].motion, Motion::None);
    assert_eq!(loaded.materials[1].motion, Motion::None);
    let saved = save_vox(&cells, &loaded.palette.unwrap(), &loaded.materials).expect("save");
    assert_eq!(bytes, saved);
}

#[test]
fn a_motion_edit_undoes_and_redoes() {
    let mut session = Session::new();
    session.begin_edit();
    session.material_mut().motion = Motion::Sway;
    session.material_mut().motion_amount = 2.0;
    session.end_edit();
    session.undo();
    assert_eq!(session.materials[36].motion, Motion::None);
    assert_eq!(session.materials[36].motion_amount, 1.0);
    session.redo();
    assert_eq!(session.materials[36].motion, Motion::Sway);
    assert_eq!(session.materials[36].motion_amount, 2.0);
}

#[test]
fn color_and_material_edits_are_one_undo_step() {
    let mut session = Session::new();
    session.picked.insert((0, 0, 0));
    let before = session.palette[36];
    session.begin_edit();
    session.set_color(36, [9, 8, 7]);
    session.end_edit();
    session.begin_edit();
    session.material_mut().roughness = 0.25;
    session.end_edit();
    session.undo();
    assert_eq!(session.materials[36].roughness, 1.0);
    assert_eq!(session.palette[36], [9, 8, 7, 255]);
    assert!(session.picked.contains(&(0, 0, 0)));
    session.undo();
    assert_eq!(session.palette[36], before);
    assert!(session.picked.contains(&(0, 0, 0)));
}

#[test]
fn rotation_bytes_round_trip() {
    assert!(rotation_codec_errors().is_empty());
}

#[test]
fn every_fixture_survives_voxrt() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut files = 0;
    for entry in std::fs::read_dir(&dir).expect("fixtures") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|ext| ext == "vox") {
            files += 1;
            crate::cli::check_round_trip(&path).unwrap_or_else(|err| panic!("{err}"));
        }
    }
    assert!(files > 0, "tests/fixtures has no .vox files");
}

#[test]
fn bones_scripts_motion_and_hidden_survive_a_save() {
    let mut session = Session::new();
    session.active_mut().name = "Leaf".into();
    session.active_mut().cells.clear();
    session.active_mut().cells.insert((1, 2, 3), 4);
    session.active_mut().hidden = true;
    session.active_mut().script.source = "voxel(1, 2, 3, 4);\n".into();
    session.active_mut().script.auto = true;
    session
        .active_mut()
        .script
        .params
        .push(crate::script::ScriptParam {
            name: "n".into(),
            value: crate::script::ParamValue::Int {
                value: 2,
                min: 0,
                max: 4,
            },
        });
    session.materials[4].motion = Motion::Sway;
    session.materials[4].motion_amount = 1.5;
    session
        .add_bone("root", None, [0.0, 0.0, 0.0], None)
        .expect("bone");
    session.bind_object(0, Some("root")).expect("bind");
    let bytes = session.export_bytes().expect("save");
    let mut loaded = Session::new();
    loaded.import_bytes(&bytes).expect("load");
    assert_eq!(loaded.objects[0].name, "Leaf");
    assert!(loaded.objects[0].hidden);
    assert_eq!(
        loaded.objects[0]
            .cells
            .values()
            .copied()
            .collect::<Vec<_>>(),
        vec![4]
    );
    assert_eq!(loaded.objects[0].bone.as_deref(), Some("root"));
    assert_eq!(loaded.objects[0].script.source, "voxel(1, 2, 3, 4);\n");
    assert!(loaded.objects[0].script.auto);
    assert_eq!(loaded.objects[0].script.params.len(), 1);
    assert_eq!(loaded.materials[4].motion, Motion::Sway);
    assert_eq!(loaded.materials[4].motion_amount, 1.5);
    assert_eq!(loaded.skeleton.bones.len(), 1);
    let again = loaded.export_bytes().expect("save again");
    assert_eq!(again, bytes);
    let mut third = Session::new();
    third.import_bytes(&again).expect("reload");
    let diff = loaded.round_trip_differences(&third);
    assert!(diff.is_empty(), "{diff:?}");
}

#[test]
fn bone_pivots_follow_the_objects_when_a_file_is_recentred() {
    // The only object is off the ground, so loading moves the scene down by 5.
    let mut session = Session::new();
    session.active_mut().cells.clear();
    session.active_mut().cells.insert((0, 0, 5), 1);
    session
        .add_bone("joint", None, [0.5, 0.5, 5.5], None)
        .expect("bone");
    session.bind_object(0, Some("joint")).expect("bind");
    let bytes = session.export_bytes().expect("save");
    let mut loaded = Session::new();
    loaded.import_bytes(&bytes).expect("load");
    let object = &loaded.objects[0];
    let cell = *object.cells.keys().next().expect("cell");
    let centre = [
        (cell.0 + object.translation.0) as f32 + 0.5,
        (cell.1 + object.translation.1) as f32 + 0.5,
        (cell.2 + object.translation.2) as f32 + 0.5,
    ];
    assert_eq!(loaded.skeleton.bones[0].pivot, centre);
    let again = loaded.export_bytes().expect("save again");
    let mut third = Session::new();
    third.import_bytes(&again).expect("reload");
    assert_eq!(third.skeleton.bones[0].pivot, centre);
}

#[test]
fn a_reopened_scene_stands_on_its_lowest_voxel() {
    // The first object floats above a second one on the ground; nothing moves on reopen.
    let mut session = Session::new();
    session.active_mut().cells.clear();
    session.active_mut().cells.insert((0, 0, 20), 1);
    session
        .add_object(Some("foot"), [0, 0, 0], None, true)
        .expect("object");
    session.active_mut().cells.insert((0, 0, 0), 2);
    let bytes = session.export_bytes().expect("save");
    let mut loaded = Session::new();
    loaded.import_bytes(&bytes).expect("load");
    let world = |object: &Volume| {
        let cell = *object.cells.keys().next().expect("cell");
        (
            cell.0 + object.translation.0,
            cell.1 + object.translation.1,
            cell.2 + object.translation.2,
        )
    };
    assert_eq!(world(&loaded.objects[0]), (0, 0, 20));
    assert_eq!(world(&loaded.objects[1]), (0, 0, 0));
}

#[test]
#[ignore = "prints edit timings for a 256³ object; not a pass/fail check"]
fn edit_speed_of_a_256_object() {
    use std::time::Instant;

    fn rebuild(session: &Session) -> (u128, usize) {
        let object = session.active();
        let mut buckets: HashMap<(i32, i32, i32), Vec<(Cell, [u8; 3])>> = HashMap::new();
        for (&cell, &slot) in &object.cells {
            buckets
                .entry(crate::mesh::chunk_key(cell))
                .or_default()
                .push((cell, session.color(slot)));
        }
        let next: HashMap<(i32, i32, i32), u64> = buckets
            .iter()
            .map(|(key, listed)| (*key, crate::mesh::occupancy_hash(listed)))
            .collect();
        let dirty = crate::mesh::dirty_chunks(&HashMap::new(), &next);
        let started = Instant::now();
        let mut quads = 0usize;
        for key in &dirty {
            let built = crate::mesh::mesh_chunk(
                &object.cells,
                *key,
                |slot| session.color(slot),
                |slot| session.motion_uv(slot),
            );
            quads += built.indices.len() / 6;
        }
        (started.elapsed().as_millis(), quads)
    }

    let mut session = Session::new();
    session.active_mut().cells.clear();
    session.active_mut().dims = (256, 256, 256);
    session.brush_size = 17;
    session.brush_shape = BrushShape::Square;
    session.brush_3d = true;
    session.tool_mode = ToolMode::Add;
    session.active_slot = 1;

    let started = Instant::now();
    for step in 0..200 {
        session.stamp(Hit {
            cell: (step, 128, 128),
            normal: (0, 0, 1),
        });
    }
    let brush_ms = started.elapsed().as_millis();
    let (brush_mesh_ms, brush_quads) = rebuild(&session);

    let started = Instant::now();
    for cell in crate::tools::cube((0, 0, 0), (127, 127, 127), true) {
        session.write_cell(cell, 1);
    }
    let box_ms = started.elapsed().as_millis();
    let (box_mesh_ms, box_quads) = rebuild(&session);

    let seeds: Vec<Cell> = (0..128)
        .flat_map(|y| (0..128).map(move |x| (x, y, 127)))
        .collect();
    let started = Instant::now();
    for cell in crate::tools::extrude(&seeds, 2, 1, 1, true) {
        session.write_cell(cell, 1);
    }
    let extrude_ms = started.elapsed().as_millis();
    let (extrude_mesh_ms, extrude_quads) = rebuild(&session);

    println!(
        "edit 256³: brush 200×r8 {brush_ms} ms, mesh {brush_mesh_ms} ms ({brush_quads} quads); \
         box 128³ {box_ms} ms, mesh {box_mesh_ms} ms ({box_quads} quads); \
         extrude 128×128 {extrude_ms} ms, mesh {extrude_mesh_ms} ms ({extrude_quads} quads)"
    );
}

#[test]
fn panel_actions_hide_mirror_and_set_export_options() {
    let mut session = Session::new();
    session.replace_empty();
    session.active_mut().cells.insert((1, 0, 0), 5);
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(10.0, 10.0));
    let press = |session: &mut Session, action: UiAction| {
        session.ui_regions.clear();
        session.note_ui("test", "Test", rect, action);
        let hit = session.ui_target(1.0, 1.0).unwrap();
        session.activate_ui(&hit, None)
    };
    press(&mut session, UiAction::ObjectHidden(0)).unwrap();
    assert!(session.objects[0].hidden);
    session.undo();
    assert!(!session.objects[0].hidden);

    session.active_mut().name = "arm_l".into();
    press(&mut session, UiAction::Mirror(0)).unwrap();
    assert_eq!(session.objects.len(), 2);
    assert_eq!(session.selected, 1);
    assert_eq!(session.objects[1].name, "arm_r");
    let world: Vec<i32> = session.objects[1]
        .cells
        .keys()
        .map(|cell| cell.0 + session.objects[1].translation.0)
        .collect();
    assert_eq!(world, vec![-2]);
    session.undo();
    assert_eq!(session.objects.len(), 1);

    press(&mut session, UiAction::Export(ExportFlag::Split)).unwrap();
    press(&mut session, UiAction::Export(ExportFlag::Merge)).unwrap();
    assert!(session.export.split_objects);
    assert!(!session.export.merge);
    press(&mut session, UiAction::ShowBones).unwrap();
    assert!(session.show_bones);
    assert!(press(&mut session, UiAction::KeyPose).is_err());
}

#[test]
fn a_panel_drag_is_one_undo_step() {
    let mut session = Session::new();
    session.replace_empty();
    session.active_mut().cells.insert((0, 0, 0), 5);
    let start = session.objects[0].translation;
    session.begin_gesture();
    for x in 1..=3 {
        session.set_object(0, None, Some([x, 0, 0]), None).unwrap();
    }
    session.end_gesture();
    assert_eq!(session.objects[0].translation.0, 3);
    session.undo();
    assert_eq!(session.objects[0].translation, start);
    assert!(!session.can_undo());
}

#[test]
fn key_pose_keys_the_bind_rotation_times_the_pose() {
    let mut session = Session::new();
    session.replace_empty();
    let half = std::f32::consts::FRAC_1_SQRT_2;
    session
        .add_bone("root", None, [0.0, 0.0, 0.0], Some([0.0, 0.0, half, half]))
        .unwrap();
    session
        .add_bone("tip", Some("root"), [0.0, 0.0, 4.0], None)
        .unwrap();
    let turn = crate::rig::BonePose {
        rotation: [half, 0.0, 0.0, half],
        scale: [1.0, 2.0, 1.0],
    };
    session.set_bone_pose(0, turn).unwrap();
    session.selected_bone = Some(1);
    session.add_drafted_clip().unwrap();
    assert_eq!(session.clips[0].name, "Clip 1");
    session.clip_time = 0.5;
    assert_eq!(session.key_pose().unwrap(), 2);
    let clip = &session.clips[0];
    let root = clip
        .tracks
        .iter()
        .find(|track| track.bone == "root")
        .unwrap();
    let want = (glam::Quat::from_array([0.0, 0.0, half, half])
        * glam::Quat::from_array(turn.rotation))
    .normalize();
    let got = glam::Quat::from_array(root.keys[0].rotation.unwrap());
    assert!(got.dot(want).abs() > 0.9999);
    assert_eq!(root.keys[0].scale, [1.0, 2.0, 1.0]);
    let tip = clip
        .tracks
        .iter()
        .find(|track| track.bone == "tip")
        .unwrap();
    assert_eq!(tip.keys[0].rotation, None);

    // A second key at the same time replaces the first; another time adds one.
    session.key_pose().unwrap();
    session.clip_time = 1.0;
    session.key_pose().unwrap();
    assert_eq!(session.clips[0].tracks[0].keys.len(), 2);
    // Both bones keyed in one press are one undo step.
    session.undo();
    assert!(session.clips[0]
        .tracks
        .iter()
        .all(|track| track.keys.len() == 1));
}

#[test]
fn rounded_box_generator_matches_fill_rounded_box() {
    let gen = crate::tools::Gen {
        kind: GeneratorKind::RoundedBox,
        round_radius: 2.5,
        palette: 3,
        ..Default::default()
    };
    let mut drawn: Vec<Cell> = crate::tools::generate((0, 0, 0), (5, 6, 7), &gen)
        .into_iter()
        .map(|(cell, _)| cell)
        .collect();
    drawn.sort();
    let mut session = Session::new();
    session.replace_empty();
    session.active_mut().cells.clear();
    crate::remote::apply(
        &mut session,
        "voxel.rounded",
        &serde_json::json!({"x0": 0, "y0": 0, "z0": 0, "x1": 5, "y1": 6, "z1": 7, "palette": 3, "radius": 2.5}),
    )
    .unwrap();
    let mut filled: Vec<Cell> = session.active().cells.keys().copied().collect();
    filled.sort();
    assert!(!drawn.is_empty());
    assert!(drawn.len() < 6 * 7 * 8);
    assert_eq!(drawn, filled);
}

#[test]
fn the_export_dialog_names_its_options() {
    let mut session = Session::new();
    assert_eq!(
        session.export_summary(),
        "Export Mesh — glTF binary (.glb): merged faces, palette texture"
    );
    session.export.split_objects = true;
    session.export.vertex_colors = true;
    assert!(session
        .export_summary()
        .ends_with("vertex colors, one mesh per object"));
}
