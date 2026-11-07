use eyre::WrapErr;
use omni_diagnostic::{Audience, find};
use serde_json::json;

use crate::Error;

fn report(error: Error) -> eyre::Report {
    Err::<(), _>(error)
        .wrap_err("expanding meta sources")
        .unwrap_err()
}

#[test]
fn cycle_is_a_user_diagnostic_with_its_chain() {
    let r = report(Error::cycle_detected("a → b → a"));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "meta_source.cycle_detected");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details(), Some(json!({ "chain": "a → b → a" })));
}

#[test]
fn depth_exceeded_names_the_source_and_limit() {
    let r = report(Error::depth_exceeded(8, "root.nested"));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "meta_source.depth_exceeded");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(
        d.details(),
        Some(json!({ "max": 8, "qualified_id": "root.nested" }))
    );
}

#[test]
fn duplicate_member_id_names_the_bundle_and_id() {
    let r = report(Error::duplicate_member_id(Some("core"), "lint"));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "meta_source.duplicate_member_id");
    assert_eq!(d.details(), Some(json!({ "bundle": "core", "id": "lint" })));

    let projection = report(Error::duplicate_member_id(None::<String>, "lint"));
    let d = find(&projection).expect("diagnostic");
    assert_eq!(d.details(), Some(json!({ "bundle": null, "id": "lint" })));
}

#[test]
fn plain_custom_errors_stay_uncoded() {
    assert!(find(&report(Error::custom("boom"))).is_none());
}

#[test]
fn codes_are_registered_in_the_catalog() {
    for code in [
        "meta_source.cycle_detected",
        "meta_source.depth_exceeded",
        "meta_source.duplicate_member_id",
    ] {
        assert!(omni_codes::get(code).is_some(), "{code} is not registered");
    }
}
