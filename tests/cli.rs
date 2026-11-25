use super::*;

#[test]
fn a_flag_becomes_an_argument_in_camel_case_with_its_type() {
    let arg = |flag: &str, value: &str| flag_argument(flag, value).unwrap();
    assert_eq!(
        arg("--min-feature", "2"),
        ("minFeature".into(), serde_json::json!(2))
    );
    assert_eq!(
        arg("--scale", "1.5"),
        ("scale".into(), serde_json::json!(1.5))
    );
    assert_eq!(
        arg("--center", "1,2,3"),
        ("center".into(), serde_json::json!([1, 2, 3]))
    );
    assert_eq!(
        arg("--merge", "true"),
        ("merge".into(), serde_json::json!(true))
    );
    assert_eq!(
        arg("--op", "taper"),
        ("op".into(), serde_json::json!("taper"))
    );
    assert_eq!(
        arg("--mirror", "{\"axis\": \"x\"}"),
        ("mirror".into(), serde_json::json!({"axis": "x"}))
    );
    assert!(flag_argument("--center", "1,a").is_err());
}

#[test]
fn run_fills_saves_and_refuses_what_it_cannot_run() {
    let path = std::env::temp_dir().join(format!("froxel-run-{}.vox", std::process::id()));
    let args: Vec<String> = [
        "fill_box",
        "--x0",
        "0",
        "--y0",
        "0",
        "--z0",
        "0",
        "--x1",
        "2",
        "--y1",
        "2",
        "--z1",
        "2",
        "--palette",
        "4",
        "--out",
    ]
    .iter()
    .map(|s| s.to_string())
    .chain([path.display().to_string()])
    .collect();
    assert_eq!(run_tool(&args), 0);
    let mut session = Session::new();
    session.open_vox(&path).unwrap();
    assert_eq!(session.voxel_count(), 27);
    std::fs::remove_file(&path).ok();
    let strings = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(run_tool(&strings(&["no_such_tool"])), 2);
    assert_eq!(run_tool(&strings(&["ui_click"])), 2);
    assert_eq!(run_tool(&strings(&["set_voxel", "--colour", "3"])), 1);
}

#[test]
fn an_args_file_names_its_files_relative_to_itself() {
    let mut args = serde_json::json!({
        "path": "sheet.png",
        "views": [{"view": "front", "path": "front.png", "depth": {"path": "d.png"}}],
        "inputs": ["a.vox", "/abs/b.vox"],
        "depthPath": "depth.png",
        "preset": "game",
        "palette": 4
    });
    args_relative(&mut args, Path::new("/work/sheets"));
    assert_eq!(args["path"], "/work/sheets/sheet.png");
    assert_eq!(args["views"][0]["path"], "/work/sheets/front.png");
    assert_eq!(args["views"][0]["depth"]["path"], "/work/sheets/d.png");
    assert_eq!(
        args["inputs"],
        serde_json::json!(["/work/sheets/a.vox", "/abs/b.vox"])
    );
    assert_eq!(args["depthPath"], "/work/sheets/depth.png");
    assert_eq!(args["preset"], "game");
    assert_eq!(args["palette"], 4);
}

#[test]
fn every_old_subcommand_names_a_tool_that_replaces_it() {
    for old in [
        "render",
        "preview",
        "export",
        "bake-scene",
        "carve",
        "fit",
        "optimize",
        "compare",
        "measure",
        "pixelate",
        "import",
        "orient",
        "deterrace",
        "deform",
        "blend",
        "relief",
        "split-parts",
        "reduce-palette",
        "components",
        "variants",
    ] {
        let tool = replaced_by(old).unwrap_or_else(|| panic!("{old}"));
        assert!(crate::ops::by_tool(tool).is_some(), "{old} -> {tool}");
    }
    assert!(replaced_by("paint").is_some());
    assert!(replaced_by("voxrt").is_none());
}
