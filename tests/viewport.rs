use super::*;

#[test]
fn marquee_contains_the_cell_under_its_projection() {
    let session = Session::new();
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 600.0));
    let cell = session.active().cells.keys().copied().next().unwrap();
    let aspect = rect.width() / rect.height();
    let vp = projection(aspect) * view_matrix(&session);
    let center = Vec3::new(
        cell.0 as f32 + 0.5,
        cell.1 as f32 + 0.5,
        cell.2 as f32 + 0.5,
    );
    let ndc = to_ndc(vp, center).unwrap();
    let sx = rect.min.x + (ndc.x * 0.5 + 0.5) * rect.width();
    let sy = rect.min.y + (1.0 - (ndc.y * 0.5 + 0.5)) * rect.height();
    let hit = marquee_cells(
        &session,
        rect,
        egui::pos2(sx - 4.0, sy - 4.0),
        egui::pos2(sx + 4.0, sy + 4.0),
    );
    assert!(hit.contains(&cell));
    let click = marquee_cells(&session, rect, egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    assert!(click.is_empty());
}
