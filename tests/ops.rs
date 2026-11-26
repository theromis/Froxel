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

#[test]
fn no_rhai_function_keeps_a_hand_written_option_list() {
    // Rhai option lists come from the schema (script/bind.rs); a plain
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
                Value::String(text) => stated == *text,
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
            "choice_label(",
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
                    // A prop_* row whose tool is a variable (mirror_row, the
                    // Growth plant) is checked where the table merges.
                    _ if widget.starts_with("prop_") || widget == "choice_label(" => continue,
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
                if widget == "choice_label(" {
                    let _ = choice_ui(&tool, &key);
                    checked += 1;
                    continue;
                }
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

/// Every panel choice row names tools, and arguments one of them has, and
/// no two rows of a tool share an id.
#[test]
fn every_panel_choice_names_arguments_of_its_tools() {
    let mut seen = std::collections::HashSet::new();
    for choice in panel::CHOICES {
        for tool in choice.tools {
            assert!(
                schema::properties(tool).is_some(),
                "{} names `{tool}`",
                choice.id
            );
        }
        assert!(
            !choice.keys.is_empty() || choice.tools.len() > 1,
            "{} sets no argument and picks no tool",
            choice.id
        );
        for key in choice.keys {
            assert!(
                choice
                    .tools
                    .iter()
                    .any(|tool| schema::properties(tool).unwrap().contains_key(*key)),
                "{} sets `{key}`, which none of {:?} takes",
                choice.id,
                choice.tools
            );
        }
        assert!(
            seen.insert((choice.tools[0], choice.id)),
            "{} twice",
            choice.id
        );
        assert!(!choice.label.is_empty() && !choice.hint.is_empty());
    }
}

/// Every panel field of an object argument is named in that argument's
/// schema description, the only place the schema describes it.
#[test]
fn every_panel_field_is_named_in_its_argument() {
    for field in panel::FIELDS {
        let (key, name) = field.path.split_once('.').expect("key.field");
        let description = schema::properties(field.tool).unwrap()[key]["description"]
            .as_str()
            .unwrap_or("");
        assert!(
            description.contains(name),
            "{} {key} does not describe `{name}`",
            field.tool
        );
        let prop = field_ui(field.tool, field.path);
        assert!(!prop.label.is_empty() && !prop.hint.is_empty());
    }
}

#[test]
fn every_frontend_refuses_a_value_outside_its_hard_range() {
    let mut session = crate::state::Session::new();
    session.replace_empty();
    let corners = json!({"x0": 0, "y0": 0, "z0": 0, "x1": 7, "y1": 7, "z1": 7});
    let with = |key: &str, value: Value| {
        let mut args = corners.clone();
        args["kind"] = json!("tree");
        args[key] = value;
        args
    };
    for (key, value) in [
        ("depth", json!(6)),
        ("depth", json!(0)),
        ("wildness", json!(1.5)),
        ("leaf", json!(0)),
    ] {
        let error = crate::remote::apply(&mut session, "voxel.generate", &with(key, value.clone()))
            .unwrap_err();
        assert!(error.contains(key), "{key} {value}: {error}");
    }
    assert!(crate::remote::apply(&mut session, "voxel.generate", &with("depth", json!(5))).is_ok());
    // The panel's call goes through the same check.
    let mut extra = corners.as_object().unwrap().clone();
    extra.insert("kind".into(), json!("tree"));
    extra.insert("depth".into(), json!(6));
    assert!(session.op_call("generate", extra).is_err());
}

#[test]
fn every_range_a_description_states_is_the_schema_range() {
    // Where the prose and the range differ on purpose: 0 also erases, a
    // cushion is thicker in its middle, and twist has no range.
    let differ = [
        ("set_voxel", "palette"),
        ("surface_stroke", "palette"),
        ("moss", "thickness"),
        ("deform", "angle"),
    ];
    let mut checked = 0;
    for tool in schema::tools() {
        let name = tool["name"].as_str().unwrap();
        let Some(properties) = tool["inputSchema"]["properties"].as_object() else {
            continue;
        };
        for (key, property) in properties {
            let numeric = matches!(property["type"].as_str(), Some("integer" | "number"));
            let description = property["description"].as_str().unwrap_or("");
            let Some(stated) = stated_range(description) else {
                continue;
            };
            if !numeric || differ.contains(&(name, key.as_str())) {
                continue;
            }
            let declared = (property["minimum"].as_f64(), property["maximum"].as_f64());
            assert_eq!(
                declared,
                (Some(stated.0), Some(stated.1)),
                "{name} `{key}` says {}..{} but declares {declared:?}; add it to ops::ranges",
                stated.0,
                stated.1
            );
            checked += 1;
        }
    }
    assert!(checked >= 200, "only {checked} ranges");
}

/// The first `a..b` or `a..=b` of a description.
fn stated_range(description: &str) -> Option<(f64, f64)> {
    let at = description.find("..")?;
    let number = |text: &str| -> Option<f64> { text.parse().ok() };
    let before: String = description[..at]
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let after: String = description[at + 2..]
        .trim_start_matches('=')
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    Some((number(&before)?, number(after.trim_end_matches('.'))?))
}
