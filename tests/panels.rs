//! The window layout: no Inspector row wider than the panel, and docks that
//! tile the window without covering each other. A row too wide for the
//! Inspector once pushed its edge under the viewport, hiding its tabs and
//! its resize handle.

use super::*;

/// Sessions that show the Inspector's optional rows as well as its default
/// ones.
fn sessions() -> Vec<(&'static str, Session)> {
    let plain = Session::new();
    let mut baked = Session::new();
    baked.export.bake = Some(crate::export::bake::Bake::default());
    let mut object = Session::new();
    object.editor_mode = EditorMode::Object;
    vec![
        ("default", plain),
        ("bake light", baked),
        ("object mode", object),
    ]
}

/// A context themed and with the fonts of the window.
fn themed() -> egui::Context {
    let ctx = egui::Context::default();
    crate::apply_theme(&ctx);
    ctx
}

fn input(width: f32, height: f32) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width, height),
        )),
        ..Default::default()
    }
}

/// Grids and wrapped rows settle over a few frames; the last one counts.
const FRAMES: usize = 4;

/// One frame with no renderer: its texture uploads are dropped.
fn frame(ctx: &egui::Context, width: f32, height: f32, run: impl FnMut(&mut egui::Ui)) {
    ctx.run_ui(input(width, height), run).textures_delta.clear();
}

#[test]
fn every_inspector_row_fits_every_panel_width() {
    let start = *INSPECTOR_WIDTH.start();
    let end = *INSPECTOR_WIDTH.end();
    for (name, mut session) in sessions() {
        for width in [start, 300.0, end] {
            let ctx = themed();
            let mut fit = InspectorFit::default();
            for _ in 0..FRAMES {
                frame(&ctx, 1280.0, 800.0, |ui| {
                    let style = ui.style().clone();
                    fit = inspector_panel(&style)
                        .exact_size(width)
                        .show(ui, |ui| right(ui, &mut session))
                        .inner;
                });
            }
            assert!(
                fit.overflow < 0.5,
                "{name}: Inspector section {} is {:.1} points wider than a {width}-point panel; \
                 wrap its rows (horizontal_wrapped) or narrow them",
                fit.section,
                fit.overflow
            );
        }
    }
}

/// Lay the window out at `width` by `height` and check that the docks tile
/// it: the Inspector against the right edge at an allowed width, the tool
/// options, viewport, and palette between the strip and the Inspector and
/// stacked without overlap, and no Inspector section wider than its panel.
fn check_docks(at: &str, width: f32, height: f32, session: &mut Session) {
    let ctx = themed();
    let mut docks = None;
    for _ in 0..FRAMES {
        frame(&ctx, width, height, |ui| {
            docks = Some(desktop(ui, session, |ui, _| {
                ui.allocate_space(ui.available_size());
            }));
        });
    }
    let docks = docks.unwrap();
    assert!(
        (docks.inspector.right() - width).abs() < 0.5,
        "{at}: the Inspector ends at {} instead of the window's edge",
        docks.inspector.right()
    );
    assert!(
        INSPECTOR_WIDTH.contains(&docks.inspector.width().round()),
        "{at}: the Inspector is {} points wide",
        docks.inspector.width()
    );
    for (dock, rect) in [
        ("viewport", docks.viewport),
        ("tool options", docks.tool_options),
        ("palette", docks.palette),
    ] {
        assert!(
            rect.left() >= docks.strip.right() - 0.5,
            "{at}: the {dock} starts at {} under the tool strip (ends {})",
            rect.left(),
            docks.strip.right()
        );
        assert!(
            rect.right() <= docks.inspector.left() + 0.5,
            "{at}: the {dock} reaches {} over the Inspector (starts {})",
            rect.right(),
            docks.inspector.left()
        );
    }
    assert!(
        docks.tool_options.bottom() <= docks.viewport.top() + 0.5
            && docks.viewport.bottom() <= docks.palette.top() + 0.5,
        "{at}: the tool options, viewport, and palette overlap"
    );
    assert!(
        docks.inspector_fit.overflow < 0.5,
        "{at}: Inspector section {} is {:.1} points wider than the panel",
        docks.inspector_fit.section,
        docks.inspector_fit.overflow
    );
}

/// The smallest window the layout is checked at.
const SMALL: (f32, f32) = (1024.0, 700.0);

#[test]
fn docks_tile_the_window() {
    for (width, height) in [SMALL, (1280.0, 800.0), (1920.0, 1080.0)] {
        for (name, mut session) in sessions() {
            check_docks(
                &format!("{name} at {width}x{height}"),
                width,
                height,
                &mut session,
            );
        }
    }
}

#[test]
fn every_tool_option_row_fits_a_small_window() {
    for (tool, label, _) in tools::TOOLS {
        let mut session = Session::new();
        session.tool = tool;
        let (width, height) = SMALL;
        check_docks(
            &format!("{label} tool at {width}x{height}"),
            width,
            height,
            &mut session,
        );
    }
}
