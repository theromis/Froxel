use std::collections::HashMap;

use serde_json::{json, Value};

use super::*;
use crate::script::{execute, DocView, Stop};
use crate::state::{Cell, Session};

fn session() -> Session {
    let mut session = Session::new();
    session.replace_empty();
    session
}

fn cells(session: &Session) -> HashMap<Cell, u8> {
    session.objects[0].cells.clone().into_map()
}

fn call(session: &mut Session, tool: &str, args: Value) -> Result<Value, String> {
    let method = crate::ops::by_tool(tool).unwrap().method;
    crate::remote::apply(session, method, &args)
}

fn run(source: &str) -> Result<HashMap<Cell, u8>, String> {
    let view = DocView {
        names: vec!["host".to_string()],
        ..DocView::default()
    };
    let outcome = execute(source, &[], 0, &view, &Stop::after(None))?;
    Ok(outcome.built.into_iter().next().unwrap().1)
}

/// The tools that take a box, as the schema says; a new one is covered by
/// the shared parser without an entry here, and this list says which.
#[test]
fn every_box_tool_takes_min_and_max_and_its_corners_are_not_required() {
    let names: Vec<&str> = box_tools().iter().map(|form| form.tool.as_str()).collect();
    for tool in [
        "fill_box",
        "get_voxels",
        "fill_ellipsoid",
        "fill_rounded_box",
        "generate",
        "recolor",
        "carve",
        "smooth",
        "scatter",
        "moss",
        "foliage",
        "deform",
        "blend_shapes",
        "set_marker",
        "planks",
    ] {
        assert!(names.contains(&tool), "{tool} is not a box tool");
    }
    for tool in crate::ops::schema::tools() {
        let properties = tool["inputSchema"]["properties"].as_object().unwrap();
        let name = tool["name"].as_str().unwrap();
        let has_corners = properties.contains_key("x0") && properties.contains_key("x1");
        assert_eq!(names.contains(&name), has_corners, "{name}");
        if has_corners {
            assert!(
                properties.contains_key("min") && properties.contains_key("max"),
                "{name}"
            );
            let required = tool["inputSchema"]["required"].as_array().unwrap();
            for key in CORNERS.iter().chain(LIST_KEYS.iter()) {
                assert!(
                    !required.iter().any(|found| found == key),
                    "{name} requires {key}"
                );
            }
        }
    }
    let planks = box_form("planks").unwrap();
    assert_eq!(planks.axes, vec![0, 1]);
    assert!(box_form("fill_box").unwrap().required);
    assert!(!box_form("recolor").unwrap().required);
}

#[test]
fn min_and_max_fill_the_same_cells_as_the_corners() {
    for (tool, extra) in [
        ("fill_box", json!({"palette": 3})),
        ("fill_ellipsoid", json!({"palette": 3})),
        ("fill_rounded_box", json!({"palette": 3, "radius": 1})),
        ("generate", json!({"kind": "cone"})),
    ] {
        let mut corners = extra.clone();
        for (key, value) in CORNERS.iter().zip([1, 2, 0, 7, 9, 5]) {
            corners[*key] = json!(value);
        }
        let mut lists = extra.clone();
        lists["min"] = json!([1, 2, 0]);
        lists["max"] = json!([7, 9, 5]);
        let (mut a, mut b) = (session(), session());
        call(&mut a, tool, corners).unwrap();
        call(&mut b, tool, lists).unwrap();
        assert!(!cells(&a).is_empty(), "{tool}");
        assert_eq!(cells(&a), cells(&b), "{tool}");
    }
}

#[test]
fn a_batch_item_takes_min_and_max() {
    let mut s = session();
    call(
        &mut s,
        "batch",
        json!({"operations": [
            {"tool": "fill_ellipsoid", "arguments": {"min": [0, 0, 0], "max": [6, 6, 6], "palette": 2}},
            {"tool": "carve", "arguments": {"shape": "box", "min": [0, 0, 0], "max": [6, 6, 2]}},
            {"tool": "recolor", "arguments": {"from": 2, "to": 4, "min": [0, 0, 0], "max": [6, 6, 4]}}
        ]}),
    )
    .unwrap();
    let found = cells(&s);
    assert!(found.keys().all(|cell| cell.2 >= 3));
    assert!(found.iter().any(|(cell, slot)| cell.2 <= 4 && *slot == 4));
    assert!(found.iter().any(|(cell, slot)| cell.2 > 4 && *slot == 2));
}

#[test]
fn mixed_partial_missing_and_malformed_boxes_are_refused() {
    let mut s = session();
    let refused = |s: &mut Session, args: Value, expect: &str| {
        let error = call(s, "fill_box", args).unwrap_err();
        assert!(error.contains(expect), "{error}");
    };
    refused(&mut s, json!({"palette": 1}), "needs a box");
    refused(
        &mut s,
        json!({"palette": 1, "min": [0, 0, 0]}),
        "min needs max",
    );
    refused(
        &mut s,
        json!({"palette": 1, "max": [0, 0, 0]}),
        "max needs min",
    );
    refused(
        &mut s,
        json!({"palette": 1, "x0": 0, "y0": 0, "z0": 0}),
        "part of a box",
    );
    refused(
        &mut s,
        json!({"palette": 1, "min": [0, 0], "max": [1, 1, 1]}),
        "min must be [x, y, z]",
    );
    refused(
        &mut s,
        json!({"palette": 1, "min": [0, 0, 0.5], "max": [1, 1, 1]}),
        "of integers",
    );
    refused(
        &mut s,
        json!({"palette": 1, "min": [0, 0, 0], "max": [1, 1, 1], "x0": 0, "y0": 0, "z0": 0, "x1": 1, "y1": 1, "z1": 2}),
        "they differ",
    );
    refused(
        &mut s,
        json!({"palette": 1, "min": [0, 0, 0], "max": [1, 1, 1], "x0": 0}),
        "they differ",
    );
    assert!(cells(&s).is_empty());
    // Both forms that agree are one box.
    call(
        &mut s,
        "fill_box",
        json!({"palette": 1, "min": [0, 0, 0], "max": [1, 1, 1], "x0": 0, "y0": 0, "z0": 0, "x1": 1, "y1": 1, "z1": 1}),
    )
    .unwrap();
    assert_eq!(cells(&s).len(), 8);
    // An optional box may be left out in both forms.
    call(&mut s, "recolor", json!({"from": 1, "to": 2})).unwrap();
    assert!(cells(&s).values().all(|slot| *slot == 2));
}

#[test]
fn a_world_box_and_a_marker_box_take_min_and_max() {
    let mut s = session();
    call(
        &mut s,
        "set_object",
        json!({"index": 0, "position": [10, 0, 0]}),
    )
    .unwrap();
    call(
        &mut s,
        "fill_box",
        json!({"palette": 1, "min": [10, 0, 0], "max": [11, 0, 0], "space": "world"}),
    )
    .unwrap();
    let found = cells(&s);
    assert!(found.contains_key(&(0, 0, 0)) && found.contains_key(&(1, 0, 0)));
    let reply = call(
        &mut s,
        "set_marker",
        json!({"name": "hit", "kind": "box", "min": [0, 0, 0], "max": [1, 2, 3]}),
    )
    .unwrap();
    assert_eq!(reply["marker"]["local"]["max"], json!([2.0, 3.0, 4.0]));
}

#[test]
fn rhai_takes_the_box_in_the_options_map() {
    let positional = run(r#"
        fill_ellipsoid(0, 0, 0, 8, 8, 8, 2);
        fill_box(0, 0, 0, 8, 8, 2, 3, #{mode: "replace"});
        carve_box(0, 0, 0, 2, 8, 8);
        smooth(0, 0, 0, 8, 8, 8);
        "#)
    .unwrap();
    let mapped = run(r#"
        fill_ellipsoid(2, #{min: [0, 0, 0], max: [8, 8, 8]});
        fill_box(3, #{min: [0, 0, 0], max: [8, 8, 2], mode: "replace"});
        carve_box(#{min: [0, 0, 0], max: [2, 8, 8]});
        smooth(#{x0: 0, y0: 0, z0: 0, x1: 8, y1: 8, z1: 8});
        "#)
    .unwrap();
    assert!(!positional.is_empty());
    assert_eq!(positional, mapped);
    for (source, expect) in [
        ("fill_ellipsoid(2, #{min: [0, 0, 0]});", "min needs max"),
        ("fill_ellipsoid(2, #{mode: \"empty\"});", "needs its box"),
        (
            "fill_ellipsoid(0, 0, 0, 2, 2, 2, 1, #{min: [0, 0, 0], max: [2, 2, 2]});",
            "no option `max`",
        ),
    ] {
        let error = run(source).unwrap_err();
        assert!(error.contains(expect), "{source}: {error}");
    }
    // deform over the whole object, or a box in its options.
    run(r#"fill_box(0, 0, 0, 6, 6, 6, 1, #{}); deform("twist", #{angle: 30}); deform("twist", #{min: [0, 0, 0], max: [3, 3, 3], angle: 30});"#)
        .unwrap();
}

#[test]
fn rhai_collision_box_takes_min_and_max() {
    let view = DocView {
        names: vec!["host".to_string()],
        ..DocView::default()
    };
    let outcome = execute(
        r#"fill_box(0, 0, 0, 3, 3, 3, 1, #{}); collision_box("a", #{min: [0, 0, 0], max: [1, 1, 1]}); collision_box(#{min: [0, 0, 0], max: [2, 2, 2]});"#,
        &[],
        0,
        &view,
        &Stop::after(None),
    )
    .unwrap();
    assert_eq!(outcome.markers.len(), 2);
}
