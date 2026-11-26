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

/// The Rhai option lists, read from the source of `src/script`: each
/// `tool_options(opts, "tool", excluded)` or `tool_options_as(opts, "tool",
/// excluded, ...)`, and the `("moss", Stone::Moss, &[...])` rows of the
/// growth tools, with `excluded` inline or a `const` of the same file.
/// Returns the file, the tool, and the excluded keys.
fn rhai_exclusions() -> Vec<(String, String, Vec<String>)> {
    let sources = [
        ("document", include_str!("../src/script/document.rs")),
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
        // The file's `const NAME: &[&str] = &[...];` lists.
        let mut consts = Vec::new();
        let mut rest = flat.as_str();
        while let Some(at) = rest.find("const ") {
            rest = &rest[at + 6..];
            if let Some((name, tail)) = rest.split_once(": &[&str] = &[") {
                if let Some((body, _)) = tail.split_once("];") {
                    if !name.contains(' ') {
                        consts.push((name.to_string(), quoted(body)));
                    }
                }
            }
        }
        let list = |text: &str| -> Option<Vec<String>> {
            let text = text.trim_start();
            if let Some(body) = text.strip_prefix("&[") {
                return Some(quoted(body.split_once(']')?.0));
            }
            let name: String = text
                .chars()
                .take_while(|c| c.is_ascii_uppercase() || *c == '_')
                .collect();
            consts
                .iter()
                .find(|(found, _)| *found == name)
                .map(|(_, keys)| keys.clone())
        };
        for call in ["tool_options(", "tool_options_as("] {
            let mut from = flat.as_str();
            while let Some(hit) = from.find(call) {
                from = &from[hit + call.len()..];
                // The definitions in shapes.rs take parameters, not a tool.
                let Some((_, after)) = from.split_once(", \"") else {
                    continue;
                };
                let Some((tool, tail)) = after.split_once('"') else {
                    continue;
                };
                if tool.contains(' ') {
                    continue;
                }
                let Some(tail) = tail.strip_prefix(",") else {
                    continue;
                };
                if let Some(keys) = list(tail) {
                    out.push((file.to_string(), tool.to_string(), keys));
                }
            }
        }
        // `corners(.., BOX_SLOT, "fill_box")`: a const, then the tool.
        for (name, keys) in &consts {
            let pattern = format!("{name}, \"");
            let mut from = flat.as_str();
            while let Some(hit) = from.find(&pattern) {
                from = &from[hit + pattern.len()..];
                // `tool_options_as(.., CARVE_BOX, "carve_box")` names the
                // Rhai function there, not the tool.
                if let Some((tool, _)) = from.split_once('"') {
                    if schema::properties(tool).is_some() {
                        out.push((file.to_string(), tool.to_string(), keys.clone()));
                    }
                }
            }
        }
        let mut from = flat.as_str();
        while let Some(hit) = from.find(", Stone::") {
            let tool = from[..hit]
                .strip_suffix('"')
                .and_then(|head| head.rsplit('"').next())
                .unwrap_or("");
            let tail = &from[hit..];
            if let Some((_, tail)) = tail.split_once(", ") {
                if let Some((_, tail)) = tail.split_once(", ") {
                    if let Some(keys) = list(tail) {
                        out.push((file.to_string(), tool.to_string(), keys));
                    }
                }
            }
            from = &from[hit + 9..];
        }
    }
    out
}

#[test]
fn every_rhai_exclusion_is_an_argument_of_its_tool() {
    let lists = rhai_exclusions();
    assert!(lists.len() > 45, "found only {} option lists", lists.len());
    for (file, tool, keys) in &lists {
        let properties = schema::properties(tool).unwrap_or_else(|| {
            panic!("src/script/{file}.rs derives options from `{tool}`, which is not a tool")
        });
        for key in keys {
            assert!(
                properties.contains_key(key),
                "src/script/{file}.rs leaves `{key}` out of `{tool}`, which has no such argument"
            );
        }
        // The list it derives is what Rhai takes.
        let _ = rhai_options(tool, &keys.iter().map(String::as_str).collect::<Vec<_>>());
    }
    for tool in [
        "moss",
        "algae",
        "vines",
        "bark",
        "fill_box",
        "carve",
        "import_mesh",
    ] {
        assert!(lists.iter().any(|(_, found, _)| found == tool), "{tool}");
    }
}

#[test]
fn no_rhai_function_keeps_a_hand_written_option_list() {
    // Rhai option lists come from the schema (tool_options); a plain
    // `options(opts, &[...], ...)` call would drift from it.
    for (file, source) in [
        ("document", include_str!("../src/script/document.rs")),
        ("import", include_str!("../src/script/import.rs")),
        ("markers", include_str!("../src/script/markers.rs")),
        ("organic", include_str!("../src/script/organic.rs")),
        ("silhouette", include_str!("../src/script/silhouette.rs")),
        ("stone", include_str!("../src/script/stone.rs")),
    ] {
        let flat: String = source.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            !flat.contains(" options(") && !flat.contains("(options("),
            "src/script/{file}.rs calls options() with a hand-written list"
        );
    }
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

/// Every property with panel metadata (`schema::ui` or `ops::panel`): a
/// label and a hint,
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
            assert!(!prop.hint.is_empty(), "{} `{key}` has no hint", op.tool);
            if let Some((low, high)) = prop.soft {
                assert!(low < high, "{} `{key}` soft range", op.tool);
                let min = property["minimum"].as_f64().unwrap_or(f64::NEG_INFINITY);
                let max = property["maximum"].as_f64().unwrap_or(f64::INFINITY);
                // A panel may start at 0 below a positive minimum, where 0
                // leaves the argument out (Markers' maxBoxes: no limit).
                let off = low == 0.0 && min > 0.0;
                assert!(
                    (min <= low || off) && high <= max,
                    "{} `{key}`: soft {low}..{high} is outside {min}..{max}",
                    op.tool
                );
                // A number, or a union with one (despeckle: true or a count).
                let numeric = |kind: &Value| matches!(kind.as_str(), Some("number" | "integer"));
                let kinds = &property["type"];
                assert!(
                    numeric(kinds)
                        || kinds
                            .as_array()
                            .is_some_and(|list| list.iter().any(numeric)),
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

/// Every `prop_*` and `op_*` widget in `src/panels` names an argument with
/// panel metadata, and a slider or drag one with a soft range, so no row
/// a panel draws only sometimes can panic in `prop_ui`.
#[test]
fn every_panel_widget_names_an_argument_with_panel_metadata() {
    let sources = [
        include_str!("../src/panels/edit.rs"),
        include_str!("../src/panels/foliage.rs"),
        include_str!("../src/panels/import.rs"),
        include_str!("../src/panels/organic.rs"),
        include_str!("../src/panels/render.rs"),
        include_str!("../src/panels/silhouette.rs"),
        include_str!("../src/panels/stone.rs"),
        include_str!("../src/panels/tools.rs"),
    ];
    let mut checked = 0;
    for source in sources {
        let flat: String = source.split_whitespace().collect::<Vec<_>>().join(" ");
        for widget in [
            "prop_label(",
            "prop_slider(",
            "prop_drag(",
            "prop_check(",
            "op_toggle(",
            "op_choice(",
            "op_slider(",
            "op_slider_as(",
        ] {
            let mut from = flat.as_str();
            while let Some(hit) = from.find(widget) {
                from = &from[hit + widget.len()..];
                // `ui, "tool", "key"` or `ui, session, tool, "key"`.
                let args: Vec<&str> = from.splitn(5, ", ").collect();
                let quoted = |text: &str| -> Option<String> {
                    let text = text.trim();
                    let text = text.strip_prefix('"')?;
                    Some(text.split('"').next()?.to_string())
                };
                let (tool, key) = match (quoted(args[1]), args.get(2).and_then(|a| quoted(a))) {
                    (Some(tool), Some(key)) => (tool, key),
                    _ => {
                        // op_* take the session, then `tool` (a variable
                        // for the Tube tool) and the key.
                        let tool = args
                            .get(2)
                            .and_then(|a| quoted(a))
                            .unwrap_or("fill_tube".into());
                        let Some(key) = args.get(3).and_then(|a| quoted(a)) else {
                            continue;
                        };
                        (tool, key)
                    }
                };
                let prop = prop_ui(&tool, &key);
                if matches!(
                    widget,
                    "prop_slider(" | "prop_drag(" | "op_slider(" | "op_slider_as("
                ) {
                    assert!(
                        prop.soft.is_some(),
                        "{widget}{tool} {key} has no soft range"
                    );
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 300, "only {checked} panel widgets");
}
