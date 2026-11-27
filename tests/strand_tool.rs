use glam::Vec3;
use serde_json::json;

use super::*;

/// A 5x5 floor at z = 0 with the Strand tool active.
fn floor() -> Session {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    for x in 0..5 {
        for y in 0..5 {
            session.objects[0].cells.insert((x, y, 0), 1);
        }
    }
    session.tool = Tool::Strand;
    session.editor_mode = EditorMode::Model;
    session.render_mode = RenderMode::Edit;
    session
}

fn down_at(x: f32, y: f32) -> Option<(Vec3, Vec3)> {
    Some((Vec3::new(x, y, 30.0), Vec3::new(0.0, 0.0, -1.0)))
}

fn from_side(y: f32, z: f32) -> Option<(Vec3, Vec3)> {
    Some((Vec3::new(-30.0, y, z), Vec3::new(1.0, 0.0, 0.0)))
}

/// Place a strand from the top of cell (2, 2, 0) straight up to z 6.
fn placed(session: &mut Session) {
    assert!(!session.strand_press(down_at(2.5, 2.5), false));
    assert_eq!(session.strand_tool.placing, vec![[2.0, 2.0, 0.5]]);
    // In the air: the plane through the last point, facing the camera.
    assert!(!session.strand_press(from_side(2.5, 6.5), false));
    assert_eq!(session.strand_tool.placing[1], [2.0, 2.0, 6.0]);
    assert!(session.placing_strand().is_some());
    session.finish_strand();
    assert!(session.strand_tool.placing.is_empty());
}

#[test]
fn clicks_place_points_and_finish_adds_one_strand_as_one_undo_step() {
    let mut session = floor();
    placed(&mut session);
    assert_eq!(session.active().strands.len(), 1);
    assert_eq!(session.strand_tool.selected, Some(0));
    assert_eq!(
        session.active().strands[0].slots,
        vec![session.active_slot.max(1)]
    );
    session.undo();
    assert!(session.active().strands.is_empty());
}

#[test]
fn esc_and_delete_drop_placed_points() {
    let mut session = floor();
    session.strand_press(down_at(1.5, 1.5), false);
    session.strand_press(down_at(3.5, 1.5), false);
    session.delete_strand_part();
    assert_eq!(session.strand_tool.placing.len(), 1);
    session.cancel_strand();
    assert!(session.strand_tool.placing.is_empty());
    assert!(session.active().strands.is_empty());
}

#[test]
fn dragging_a_strand_moves_it_and_one_undo_puts_it_back() {
    let mut session = floor();
    placed(&mut session);
    session.select_strand(None);
    // Press on its body from the side, drag 3 cells along +y.
    assert!(session.strand_press(from_side(2.5, 4.0), false));
    assert_eq!(session.strand_tool.selected, Some(0));
    session.strand_drag(from_side(5.5, 4.0));
    session.strand_release();
    assert_eq!(session.active().strands[0].points[0].at, [2.0, 5.0, 0.5]);
    session.undo();
    assert_eq!(session.active().strands[0].points[0].at, [2.0, 2.0, 0.5]);
}

#[test]
fn dragging_a_handle_moves_one_point() {
    let mut session = floor();
    placed(&mut session);
    // The tip's handle, seen from the side.
    assert!(session.strand_press(from_side(2.5, 6.5), false));
    assert_eq!(session.strand_tool.point, Some(1));
    session.strand_drag(from_side(4.5, 8.5));
    session.strand_release();
    let strand = &session.active().strands[0];
    assert_eq!(strand.points[0].at, [2.0, 2.0, 0.5]);
    assert_eq!(strand.points[1].at, [2.0, 4.0, 8.0]);
    // Shift-click on the body inserts a point; Delete removes it again.
    assert!(!session.strand_press(from_side(3.0, 4.0), true));
    assert_eq!(session.active().strands[0].points.len(), 3);
    session.delete_strand_part();
    assert_eq!(session.active().strands[0].points.len(), 2);
    // Delete with no point selected removes the strand.
    session.delete_strand_part();
    assert!(session.active().strands.is_empty());
}

#[test]
fn the_panel_changes_the_selected_strand_once_the_slider_is_let_go() {
    let mut session = floor();
    placed(&mut session);
    session.set_op_arg("add_strand", "width", Some(json!(2.5)));
    session.sync_strand_panel(true);
    assert_eq!(session.active().strands[0].width, 2.5);
    session.set_op_arg("add_strand", "width", Some(json!(3.0)));
    session.sync_strand_panel(true);
    session.sync_strand_panel(false);
    assert_eq!(session.active().strands[0].width, 3.0);
    // The whole slider drag is one step.
    session.undo();
    assert_eq!(session.active().strands[0].width, 1.0);
    // Selecting loads the strand into the panel, so nothing changes by itself.
    session.select_strand(Some(0));
    session.sync_strand_panel(false);
    assert_eq!(session.active().strands[0].width, 1.0);
}
