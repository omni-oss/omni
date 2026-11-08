use std::path::PathBuf;

use eyre::WrapErr;
use omni_diagnostic::{Audience, find};

use crate::error::{FenceError, IgnoreError};

fn report(err: IgnoreError) -> eyre::Report {
    Err::<(), _>(err)
        .wrap_err("syncing ignore files")
        .unwrap_err()
}

#[test]
fn empty_pattern_is_a_user_diagnostic() {
    let r = report(IgnoreError::EmptyPattern);
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "ignore.empty_pattern");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details(), None);
    assert_eq!(d.hints().len(), 1);
}

#[test]
fn multi_line_pattern_omits_the_pattern_text() {
    let r = report(IgnoreError::MultiLinePattern("a\nb".into()));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "ignore.multi_line_pattern");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details(), None);
}

#[test]
fn fence_in_pattern_omits_the_pattern_text() {
    let line = "# @@omni-managed:begin".to_string();
    let r = report(IgnoreError::FenceInPattern(line.clone()));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "ignore.pattern_collides_with_fence");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details(), None);
    assert!(!d.hints().join(" ").contains(&line));
}

#[test]
fn malformed_fence_reports_the_kind_and_names_the_file() {
    let err = IgnoreError::malformed_fence(
        &PathBuf::from(".gitignore"),
        FenceError::DuplicateBegin(2),
    );
    let r = report(err);
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "ignore.invalid_fence");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details().unwrap()["kind"], "multiple_begin");
    assert!(d.hints()[0].contains(".gitignore"));
}

#[test]
fn io_failures_are_environment_diagnostics() {
    let r = report(IgnoreError::Io {
        action: "write",
        path: PathBuf::from("/ws/.gitignore"),
        source: std::io::Error::other("denied"),
    });
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "ignore.io_failed");
    assert_eq!(d.audience(), Audience::Environment);
    let details = d.details().unwrap();
    assert_eq!(details["action"], "write");
    assert_eq!(details["path"], "/ws/.gitignore");
}

#[test]
fn fence_error_kinds_are_short_names() {
    assert_eq!(FenceError::DuplicateEnd(2).kind(), "multiple_end");
    assert_eq!(FenceError::UnclosedBegin.kind(), "missing_end");
    assert_eq!(FenceError::UnopenedEnd.kind(), "missing_begin");
    assert_eq!(FenceError::CrossedFence.kind(), "end_before_begin");
}

#[test]
fn codes_are_registered_in_the_catalog() {
    for code in [
        "ignore.empty_pattern",
        "ignore.multi_line_pattern",
        "ignore.pattern_collides_with_fence",
        "ignore.invalid_fence",
        "ignore.io_failed",
    ] {
        assert!(
            omni_codes::get(code).is_some(),
            "`{code}` is not registered"
        );
    }
    omni_diagnostic::testing::assert_registry_valid();
    assert!(omni_codes::validate().is_ok());
}
