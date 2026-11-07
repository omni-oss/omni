use eyre::WrapErr;
use omni_capabilities::CapabilityDomain;
use omni_diagnostic::{Audience, find};
use serde_json::json;

use crate::EnforcementError;

fn report(error: EnforcementError) -> eyre::Report {
    Err::<(), _>(error)
        .wrap_err("enforcing capabilities")
        .unwrap_err()
}

#[test]
fn uncovered_domain_names_the_domain_and_backends() {
    let r = report(EnforcementError::uncovered_domain(
        CapabilityDomain::Net,
        "deno, node",
    ));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "enforcement.uncovered_domain");
    assert_eq!(d.audience(), Audience::Environment);
    assert_eq!(
        d.details(),
        Some(json!({ "domain": "net", "backends": "deno, node" }))
    );
}

#[test]
fn unenforceable_policy_is_an_environment_diagnostic_without_details() {
    let r = report(EnforcementError::unenforceable("net:*"));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "enforcement.unenforceable");
    assert_eq!(d.audience(), Audience::Environment);
    assert_eq!(d.details(), None);
}

#[test]
fn floor_and_superset_gaps_are_environment_diagnostics() {
    let floor = report(EnforcementError::no_floor("fs:read"));
    let d = find(&floor).expect("diagnostic");
    assert_eq!(d.code(), "enforcement.no_floor");
    assert_eq!(d.audience(), Audience::Environment);

    let superset = report(EnforcementError::superset_unnarrowed("env:*"));
    let d = find(&superset).expect("diagnostic");
    assert_eq!(d.code(), "enforcement.superset_unnarrowed");
    assert_eq!(d.audience(), Audience::Environment);
}

#[test]
fn conflicting_runtimes_is_an_internal_diagnostic() {
    let r = report(EnforcementError::conflicting_runtimes("deno, bun"));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "enforcement.conflicting_runtimes");
    assert_eq!(d.audience(), Audience::Internal);
    assert_eq!(d.details(), Some(json!({ "backends": "deno, bun" })));
}

#[test]
fn codes_are_registered_in_the_catalog() {
    for code in [
        "enforcement.uncovered_domain",
        "enforcement.unenforceable",
        "enforcement.no_floor",
        "enforcement.superset_unnarrowed",
        "enforcement.conflicting_runtimes",
    ] {
        assert!(omni_codes::get(code).is_some(), "{code} is not registered");
    }
}
