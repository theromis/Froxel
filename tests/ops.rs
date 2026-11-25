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
