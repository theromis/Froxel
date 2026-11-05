use serde_json::json;

use super::*;

#[test]
fn presets_have_the_game_angle_and_take_overrides() {
    let game = preset("game", None).unwrap();
    assert_eq!(
        (game.yaw, game.pitch, game.orthographic),
        (-135.0, 32.0, true)
    );
    let custom = json!({"game": {"yaw": -45}, "close": {"pitch": 10, "fov": 30}});
    let game = preset("game", Some(&custom)).unwrap();
    assert_eq!(
        (game.yaw, game.pitch, game.orthographic),
        (-45.0, 32.0, true)
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
fn the_game_preset_is_frog_worlds_camera_and_godot_look_gives_its_angles() {
    // scripts/player/pond_camera.gd: Godot yaw -45, pitch 32.
    let (yaw, pitch) = godot_look_angles([0.5997, -0.5299, -0.5997]).unwrap();
    let game = preset("game", None).unwrap();
    assert!((yaw - game.yaw).abs() < 0.01, "{yaw}");
    assert!((pitch - game.pitch).abs() < 0.01, "{pitch}");
    assert!(game.orthographic);
    // Looking down -Z in Godot, the eye is on Godot +Z, which is Froxel -Y.
    let (yaw, pitch) = godot_look_angles([0.0, 0.0, -1.0]).unwrap();
    assert_eq!((yaw, pitch), (-90.0, 0.0));
    assert_eq!(godot_look_angles([0.0, 0.0, 0.0]), None);

    let custom = json!({"game": {"godotLook": [1, 0, 0]}});
    let found = preset("game", Some(&custom)).unwrap();
    assert_eq!((found.yaw, found.pitch), (180.0, 0.0));
    for wrong in [
        json!({"game": {"godotLook": [1, 0, 0], "yaw": 3}}),
        json!({"game": {"godotLook": [0, 0, 0]}}),
        json!({"game": {"godotLook": [1, 0]}}),
    ] {
        assert!(preset("game", Some(&wrong))
            .unwrap_err()
            .contains("godotLook"));
    }
}

#[test]
fn a_presets_file_defines_the_game_camera() {
    let dir = std::env::temp_dir().join(format!("froxel-camera-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = |name: &str, text: &str| -> Value {
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        json!(path.to_string_lossy())
    };
    let flat = file("flat.json", r#"{"game": {"yaw": 10}, "low": {"pitch": 8}}"#);
    let found = preset("game", Some(&flat)).unwrap();
    assert_eq!(
        (found.yaw, found.pitch, found.orthographic),
        (10.0, 32.0, true),
        "over the built-in one, field by field"
    );
    assert_eq!(preset("low", Some(&flat)).unwrap().pitch, 8.0);
    assert_eq!(
        preset_names(Some(&flat)).unwrap(),
        ["game", "default", "low"]
    );
    // One project file can hold its cameras and its lighting.
    let both = file(
        "both.json",
        r#"{"presets": {"game": {"yaw": 20}}, "lighting": {"pond": {"sunEnergy": 1}}}"#,
    );
    assert_eq!(preset("game", Some(&both)).unwrap().yaw, 20.0);
    assert!(preset("pond", Some(&both)).unwrap_err().contains("game"));
    let lights = file("lights.json", r#"{"lighting": {"pond": {"sunEnergy": 1}}}"#);
    assert_eq!(preset_names(Some(&lights)).unwrap(), ["game", "default"]);

    let broken = file("broken.json", "[1]");
    assert!(preset("game", Some(&broken)).is_err());
    assert!(preset("game", Some(&json!("/no/such/presets.json")))
        .unwrap_err()
        .contains("Could not read"));
    assert!(preset("game", Some(&json!(3)))
        .unwrap_err()
        .contains("path"));
    std::fs::remove_dir_all(&dir).ok();
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
