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
