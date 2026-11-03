#![cfg(feature = "explicit-registry")]

use omni_diagnostic::{Diagnostic, find};

#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("e")]
#[diagnostic(code = "explicit.sample")]
struct Sample;

#[test]
fn nothing_is_found_until_the_list_is_installed() {
    let r: eyre::Report = Sample.into();
    assert!(find(&r).is_none());

    assert!(omni_diagnostic::install(&[
        &Sample::__DIAGNOSTIC_REGISTRATION
    ]));
    assert_eq!(find(&r).unwrap().code(), "explicit.sample");
    omni_diagnostic::testing::assert_registry_valid();
    omni_diagnostic::testing::assert_types_registered(&["Sample"]);
}
