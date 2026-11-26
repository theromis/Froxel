use super::*;

#[test]
fn the_schema_lists_exactly_the_registry_in_its_order() {
    let names: Vec<&str> = schema::tools()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    let ops: Vec<&str> = OPS.iter().map(|op| op.tool).collect();
    assert_eq!(names, ops);
    let mut unique = ops.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), ops.len(), "a tool is in OPS twice");
}

#[test]
fn hints_space_and_mirror_come_from_the_registry() {
    for op in OPS {
        let tool = schema::tool(op.tool).unwrap();
        let [read_only, destructive, idempotent] = op.access.hints();
        assert_eq!(
            tool["annotations"]["readOnlyHint"], read_only,
            "{}",
            op.tool
        );
        assert_eq!(
            tool["annotations"]["destructiveHint"], destructive,
            "{}",
            op.tool
        );
        assert_eq!(
            tool["annotations"]["idempotentHint"], idempotent,
            "{}",
            op.tool
        );
        // A flagged tool always lists the option; a few (split_parts,
        // paint_from_views) list one of their own without the hand-off.
        let properties = schema::properties(op.tool).unwrap();
        assert!(
            !op.space() || properties.contains_key("space"),
            "{}",
            op.tool
        );
        assert!(
            !op.mirror() || properties.contains_key("mirror"),
            "{}",
            op.tool
        );
    }
}

#[test]
fn every_frontend_gets_the_same_argument_check() {
    // A misspelt option from a panel, the CLI, or Rhai reaches apply as
    // it does from MCP, and is refused there.
    let mut session = crate::state::Session::new();
    session.replace_empty();
    let err = crate::remote::apply(
        &mut session,
        "voxel.set",
        &json!({"x": 0, "y": 0, "z": 0, "palette": 2, "colour": 3}),
    )
    .unwrap_err();
    assert!(err.contains("set_voxel has no argument `colour`"), "{err}");
    // The screenshot tools share a method; each one's arguments pass.
    assert!(check_arguments("ui.screenshot", &json!({"region": "viewport"})).is_ok());
    // A method no tool runs is left alone.
    assert!(check_arguments("ping", &json!({"anything": 1})).is_ok());
}

#[test]
fn the_methods_help_names_every_method_once() {
    let help = methods_help();
    let listed: Vec<&str> = help
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["method"].as_str().unwrap())
        .collect();
    for op in OPS {
        assert_eq!(
            listed.iter().filter(|method| **method == op.method).count(),
            1,
            "{}",
            op.method
        );
    }
    assert!(listed.contains(&"ping"));
}

/// The option lists of the Rhai functions, read from their source: each
/// `const NAME: &[&str] = &[...]`, and the function name it is checked
/// for, from `NAME, "function"` or `("function", _, NAME)`.
fn rhai_option_lists() -> Vec<(String, String, Vec<String>)> {
    let sources = [
        ("import", include_str!("../src/script/import.rs")),
        ("markers", include_str!("../src/script/markers.rs")),
        ("organic", include_str!("../src/script/organic.rs")),
        ("shapes", include_str!("../src/script/shapes.rs")),
        ("silhouette", include_str!("../src/script/silhouette.rs")),
        ("stone", include_str!("../src/script/stone.rs")),
    ];
    let quoted = |text: &str| -> Vec<String> {
        text.split('"')
            .skip(1)
            .step_by(2)
            .map(str::to_string)
            .collect()
    };
    let mut out = Vec::new();
    for (file, source) in sources {
        let flat: String = source.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut rest = flat.as_str();
        while let Some(at) = rest.find("const ") {
            rest = &rest[at + 6..];
            let Some((name, tail)) = rest.split_once(": &[&str] = &[") else {
                continue;
            };
            if name.contains(' ') {
                continue;
            }
            let Some((body, _)) = tail.split_once("];") else {
                continue;
            };
            let keys = quoted(body);
            let mut functions = Vec::new();
            for (pattern, after) in [(format!("{name}, \""), true), (format!(", {name})"), false)] {
                let mut from = flat.as_str();
                while let Some(hit) = from.find(&pattern) {
                    // `VIEWS_PAINT, "` is not a use of `PAINT`.
                    let joined = after
                        && from[..hit]
                            .chars()
                            .next_back()
                            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
                    if joined {
                        from = &from[hit + pattern.len()..];
                        continue;
                    }
                    if after {
                        let tail = &from[hit + pattern.len()..];
                        if let Some((function, _)) = tail.split_once('"') {
                            functions.push(function.to_string());
                        }
                    } else if let Some(open) = from[..hit].rfind("(\"") {
                        if let Some((function, _)) = from[open + 2..].split_once('"') {
                            functions.push(function.to_string());
                        }
                    }
                    from = &from[hit + pattern.len()..];
                }
            }
            for function in functions {
                out.push((format!("{file}::{name}"), function, keys.clone()));
            }
        }
    }
    out
}

#[test]
fn every_rhai_option_is_an_argument_of_its_tool() {
    let lists = rhai_option_lists();
    assert!(lists.len() > 30, "found only {} option lists", lists.len());
    let mut checked = 0;
    for (list, function, keys) in &lists {
        // Rhai-only functions (carve_tube, voxelize, ...) have no tool.
        let Some(properties) = schema::properties(function) else {
            continue;
        };
        checked += 1;
        for key in keys {
            assert!(
                properties.contains_key(key),
                "{list} lets Rhai `{function}` take `{key}`, which MCP `{function}` does not, so the call would be refused"
            );
        }
    }
    assert!(checked > 25, "checked only {checked} lists against a tool");
}

#[test]
fn every_schema_default_is_the_one_its_description_states() {
    let mut checked = 0;
    for op in OPS {
        let Some(properties) = schema::properties(op.tool) else {
            continue;
        };
        for (key, property) in properties {
            let Some(default) = property.get("default") else {
                continue;
            };
            checked += 1;
            let description = property["description"].as_str().unwrap_or("");
            // "Defaults to 17", "Default 1", "0 (the default)".
            let stated = ["Defaults to ", "Default "]
                .iter()
                .find_map(|lead| {
                    let at = description.find(lead)? + lead.len();
                    let word: String = description[at..]
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
                        .collect();
                    Some(word.trim_end_matches('.').to_string())
                })
                .or_else(|| {
                    let at = description.find(" (the default)")?;
                    Some(description[..at].rsplit(' ').next()?.to_string())
                });
            let Some(stated) = stated else {
                panic!(
                    "{} `{key}` has default {default} but its description names none",
                    op.tool
                );
            };
            let same = match default {
                Value::Bool(value) => stated == value.to_string(),
                Value::Number(number) => stated
                    .parse::<f64>()
                    .is_ok_and(|text| (text - number.as_f64().unwrap()).abs() < 1e-9),
                _ => stated == default.to_string(),
            };
            assert!(
                same,
                "{} `{key}`: default {default}, description says {stated}",
                op.tool
            );
        }
    }
    assert!(checked >= 50, "only {checked} defaults");
}

#[test]
fn a_left_out_argument_takes_the_schema_default_and_panels_start_from_it() {
    let mut args = json!({"x0": 0});
    fill_defaults("voxel.smooth", &mut args);
    assert_eq!(args["fillAt"], 17);
    assert_eq!(args["x0"], 0);
    let session = crate::state::Session::new();
    assert_eq!(session.smooth_fill_at, default_i32("smooth", "fillAt"));
    assert_eq!(
        crate::state::FoliagePanel::default().flatten,
        default_f32("foliage", "flatten")
    );
}

/// Every property with panel metadata (`schema::ui`): a label and a hint,
/// a soft range inside the hard one, and a toggle item that is an item of
/// the enum and not its default.
#[test]
fn panel_metadata_is_complete_and_inside_the_schema() {
    let mut checked = 0;
    for op in OPS {
        let properties = schema::properties(op.tool).unwrap();
        for (key, property) in properties {
            if property.get(schema::UI).is_none() && property.get("title").is_none() {
                continue;
            }
            checked += 1;
            let prop = prop_ui(op.tool, key);
            assert!(!prop.label.is_empty(), "{} `{key}`", op.tool);
            assert!(
                !prop.hint.is_empty() && !prop.hint.ends_with('.'),
                "{} `{key}`: a hint is one clause with no full stop",
                op.tool
            );
            if let Some((low, high)) = prop.soft {
                assert!(low < high, "{} `{key}` soft range", op.tool);
                let min = property["minimum"].as_f64().unwrap_or(f64::NEG_INFINITY);
                let max = property["maximum"].as_f64().unwrap_or(f64::INFINITY);
                assert!(
                    min <= low && high <= max,
                    "{} `{key}`: soft {low}..{high} is outside {min}..{max}",
                    op.tool
                );
                assert!(
                    matches!(property["type"].as_str(), Some("number" | "integer")),
                    "{} `{key}` has a soft range but is not a number",
                    op.tool
                );
            }
            if let Some(on) = prop.on {
                assert!(prop.items.contains(&on), "{} `{key}` on `{on}`", op.tool);
                let description = property["description"].as_str().unwrap_or("");
                assert!(
                    !description.contains(&format!("{on} (default)")),
                    "{} `{key}`: on `{on}` is the default, so off would be the same",
                    op.tool
                );
            }
        }
    }
    assert!(
        checked >= 8,
        "only {checked} properties have panel metadata"
    );
}

#[test]
fn mcp_lists_the_schema_without_the_panel_metadata() {
    let listed = schema::mcp_tools();
    assert_eq!(listed.len(), schema::tools().len());
    let text = serde_json::to_string(listed).unwrap();
    assert!(!text.contains(schema::UI));
    let caps = &listed
        .iter()
        .find(|tool| tool["name"] == "fill_tube")
        .unwrap()["inputSchema"]["properties"]["caps"];
    assert_eq!(caps["title"], "Flat ends");
    assert_eq!(caps["enum"], json!(["round", "flat"]));
}

#[test]
fn a_hard_range_is_refused_just_outside_it() {
    // fill_tube's minimum and maximum are the checks remote::apply makes.
    let properties = schema::properties("fill_tube").unwrap();
    let mut checked = 0;
    for (key, property) in properties {
        let (Some(min), Some(max)) = (property["minimum"].as_f64(), property["maximum"].as_f64())
        else {
            continue;
        };
        let step = if property["type"] == "integer" {
            1.0
        } else {
            0.01
        };
        for (value, ok) in [
            (min, true),
            (max, true),
            (min - step, false),
            (max + step, false),
        ] {
            let mut session = crate::state::Session::new();
            session.replace_empty();
            let mut args = json!({"points": [[0, 0, 0], [0, 4, 0]], "radius": 1});
            if key == "sectors" {
                args["palettes"] = json!([2, 3]);
                args["pattern"] = json!("stripes");
            } else {
                args["palette"] = json!(2);
            }
            args[key] = if property["type"] == "integer" {
                json!(value as i64)
            } else {
                json!(value)
            };
            let result = crate::remote::apply(&mut session, "voxel.tube", &args);
            // At the edge of the range another limit may still refuse it
            // (radius 128 is too many cells), but not the range itself.
            let refused_for_it = result.as_ref().is_err_and(|err| err.contains(key.as_str()));
            assert_eq!(refused_for_it, !ok, "fill_tube {key} {value}: {result:?}");
        }
        checked += 1;
    }
    assert_eq!(checked, 3, "radius, aspect, and sectors have a hard range");
}

#[test]
fn rhai_fill_tube_options_are_its_schema_less_the_positional_ones() {
    let options = rhai_options("fill_tube", &["points", "palette", "palettes"]);
    for key in [
        "mode", "radius", "curve", "caps", "aspect", "roll", "profile", "pattern", "period",
        "sectors", "seed", "space", "mirror",
    ] {
        assert!(options.contains(&key), "{key}");
    }
    assert!(!options.contains(&"points"));
    // carve_tube takes carve's tube keys; each is an argument of carve.
    let carve = schema::properties("carve").unwrap();
    for key in schema::TUBE_SHAPE_KEYS {
        assert!(carve.contains_key(key), "{key}");
        assert_eq!(
            carve[key]["enum"],
            schema::properties("fill_tube").unwrap()[key]["enum"]
        );
    }
}

#[test]
fn the_tube_panel_draws_from_the_fill_tube_schema() {
    // The arguments panels/tools.rs `tube` draws with op_slider/op_toggle.
    for key in [
        "radius", "aspect", "roll", "profile", "caps", "curve", "period", "sectors",
    ] {
        let prop = prop_ui("fill_tube", key);
        assert!(prop.tooltip().ends_with(")"), "{key}");
    }
    let caps = prop_ui("fill_tube", "caps");
    assert_eq!(caps.label, "Flat ends");
    assert_eq!(caps.on, Some("flat"));
    assert!(caps.tooltip().contains("(MCP fill_tube caps flat)"));
    assert_eq!(prop_ui("fill_tube", "sectors").soft, Some((1.0, 32.0)));
}
