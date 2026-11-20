use super::*;

#[test]
fn explain_leaves_messages_alone_outside_the_sandbox() {
    if active() {
        return;
    }
    let message = "Could not write /x/y.vox: Operation not permitted (os error 1)";
    assert_eq!(explain(message), message);
    assert!(summary().is_none());
    assert!(granted_folders().is_empty());
}

#[test]
fn denied_matches_the_sandbox_errors() {
    assert!(denied(
        "Could not read /a: Operation not permitted (os error 1)"
    ));
    assert!(denied("Permission denied (os error 13)"));
    assert!(!denied("No such file or directory (os error 2)"));
    assert!(!denied("bad value (os error 10)"));
}
