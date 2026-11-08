use eyre::WrapErr;
use omni_diagnostic::{Audience, find};
use serde_json::json;

use crate::{CapabilityDomain, Error};

fn report(error: Error) -> eyre::Report {
    Err::<(), _>(error)
        .wrap_err("loading capabilities")
        .unwrap_err()
}

#[test]
fn unsupported_domain_names_the_profile_domain_and_index() {
    let r = report(Error::unsupported_domain(
        "sandboxed",
        CapabilityDomain::Net,
        2,
    ));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "capability_policy.unsupported_domain");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(
        d.details(),
        Some(json!({ "profile": "sandboxed", "domain": "net", "index": 2 }))
    );
    assert_eq!(
        d.hints(),
        vec!["remove capability #2 or use a profile that supports `net`"]
    );
}

#[test]
fn invalid_pattern_has_no_details_because_patterns_may_hold_user_values() {
    let r = report(Error::invalid_pattern("net:*:[", "unclosed bracket"));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "capability_policy.invalid_pattern");
    assert_eq!(d.details(), None);
    assert!(!d.hints().join(" ").contains("unclosed"));
}

#[test]
fn deny_that_cannot_be_silenced_reports_its_index() {
    let r = report(Error::deny_cannot_silence_unenforceable(1));
    let d = find(&r).expect("diagnostic");
    assert_eq!(
        d.code(),
        "capability_policy.deny_cannot_silence_unenforceable"
    );
    assert_eq!(d.details(), Some(json!({ "index": 1 })));
    assert_eq!(
        d.hints(),
        vec!["set `on_unenforceable` to `warn` or `deny` on capability #1"]
    );
}

#[test]
fn plain_custom_errors_stay_uncoded() {
    assert!(find(&report(Error::custom("boom"))).is_none());
}

#[test]
fn codes_are_registered_in_the_catalog() {
    for code in [
        "capability_policy.unsupported_domain",
        "capability_policy.invalid_pattern",
        "capability_policy.deny_cannot_silence_unenforceable",
    ] {
        assert!(omni_codes::get(code).is_some(), "{code} is not registered");
    }
}
