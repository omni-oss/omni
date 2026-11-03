//! Registry checks that need every diagnostic-defining crate linked in.

// Crates are linked only if something references them; the registry is
// populated by constructors, so a dropped crate would silently lose its
// diagnostics.
use omni as _;

#[test]
fn registry_is_valid() {
    omni_diagnostic::testing::assert_registry_valid();
}

#[test]
fn known_diagnostics_are_registered() {
    omni_diagnostic::testing::assert_types_registered(&[
        "MetaFilterError",
        "FilterError",
        "ExecutionPlanProviderError",
        "TaskExecutorError",
    ]);
}
