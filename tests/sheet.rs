use serde_json::json;

use super::*;

#[test]
fn presets_have_the_game_angle_and_take_overrides() {
    let game = preset("game", None).unwrap();
    assert_eq!(
        (game.yaw, game.pitch, game.orthographic),
        (-60.0, 25.0, true)
    );
    let custom = json!({"game": {"yaw": -45}, "close": {"pitch": 10, "fov": 30}});
    let game = preset("game", Some(&custom)).unwrap();
    assert_eq!(
        (game.yaw, game.pitch, game.orthographic),
        (-45.0, 25.0, true)
    );
    let close = preset("close", Some(&custom)).unwrap();
    assert_eq!(
        (close.pitch, close.fov, close.orthographic),
        (10.0, Some(30.0), false)
    );
    assert!(preset("nope", Some(&custom)).unwrap_err().contains("close"));
    assert!(preset("game", Some(&json!({"game": {"roll": 1}})))
        .unwrap_err()
        .contains("roll"));
}

#[test]
fn text_draws_inside_its_box_and_fit_keeps_the_aspect() {
    let (w, h) = (64u32, 16u32);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    draw_text(&mut rgba, w, h, 1, 1, "Ab1", 1, [255, 255, 255, 255]);
    let lit: Vec<(u32, u32)> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .filter(|(x, y)| rgba[((y * w + x) * 4) as usize] == 255)
        .collect();
    assert!(!lit.is_empty());
    assert!(lit
        .iter()
        .all(|(x, y)| *x >= 1 && *x < 1 + text_width("Ab1", 1) && *y >= 1 && *y < 8));
    // A wide red image fits a square tile as a centred band.
    let wide = Image {
        width: 40,
        height: 10,
        rgba: [200, 0, 0, 255].repeat(400),
    };
    let tile = fit(&wide, 20, 20, [0, 0, 0, 255]);
    let at = |x: u32, y: u32| tile.rgba[((y * 20 + x) * 4) as usize];
    assert_eq!(at(10, 10), 200);
    assert_eq!(at(10, 1), 0);
}
