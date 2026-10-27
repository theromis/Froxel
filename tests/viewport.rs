use super::*;

#[test]
fn marquee_contains_the_cell_under_its_projection() {
    let session = Session::new();
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 600.0));
    let cell = session.active().cells.keys().copied().next().unwrap();
    let aspect = rect.width() / rect.height();
    let vp = projection(&session, aspect) * view_matrix(&session);
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

#[test]
fn render_rays_match_the_edit_view() {
    let mut session = Session::new();
    session.yaw = -0.8;
    session.pitch = 0.45;
    session.distance = 28.0;
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(390.0, 620.0));
    let (w, h) = (390.0, 620.0);
    let eye = session.eye();
    let forward = (session.target - eye).normalize();
    let world_up = if forward.dot(Vec3::Z).abs() > 0.95 {
        Vec3::Y
    } else {
        Vec3::Z
    };
    let right = forward.cross(world_up).normalize();
    let up = right.cross(forward).normalize();
    let aspect = w / h;
    let tan = (50_f32.to_radians() * 0.5).tan();
    for (x, y) in [
        (0.0, 0.0),
        (389.0, 0.0),
        (195.0, 0.0),
        (389.0, 310.0),
        (10.0, 619.0),
    ] {
        let u = ((x + 0.5) / w) * 2.0 - 1.0;
        let v = 1.0 - ((y + 0.5) / h) * 2.0;
        let traced = (forward + right * (u * aspect * tan) + up * (v * tan)).normalize();
        let pos = egui::pos2((x + 0.5) / w * rect.width(), (y + 0.5) / h * rect.height());
        let (_, dir) = ray_through(&session, rect, pos);
        let edited = dir.normalize();
        let dot = traced.dot(edited).clamp(-1.0, 1.0);
        assert!(
            dot > 0.999,
            "pixel {x},{y} diverges by {} deg",
            dot.acos().to_degrees()
        );
    }
}

#[test]
fn orthographic_edit_rays_are_parallel_and_frame_the_render_height() {
    let mut session = Session::new();
    session.orthographic = true;
    session.yaw = -0.8;
    session.pitch = 0.45;
    session.distance = 28.0;
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(400.0, 300.0));
    let forward = (session.target - session.eye()).normalize();
    let (top, top_dir) = ray_through(&session, rect, egui::pos2(200.0, 0.0));
    let (bottom, bottom_dir) = ray_through(&session, rect, egui::pos2(200.0, 300.0));
    assert!(top_dir.normalize().dot(forward) > 0.9999);
    assert!(bottom_dir.normalize().dot(forward) > 0.9999);
    let height = (top - bottom).length();
    let expected = 2.0 * 28.0 * (25_f32.to_radians()).tan();
    assert!((height - expected).abs() < 1e-2, "{height} vs {expected}");
}
