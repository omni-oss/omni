use std::collections::BTreeMap;

use omni_tera::{Context, Value};

#[test]
fn diagnostic_support_can_be_enabled_or_disabled_in_builtin_templates() {
    for enabled in [true, false] {
        let mut context = Context::new();
        context.insert(
            "inputs",
            &BTreeMap::from([
                ("crate_name", Value::from("sample")),
                ("crate_type", Value::from("lib")),
                ("version", Value::from("workspace")),
                ("use_tracing", Value::from(false)),
                ("use_diagnostics", Value::from(enabled)),
            ]),
        );
        context.insert(
            "data",
            &BTreeMap::from([("rust_error_name", "SampleError")]),
        );

        let cargo = omni_tera::one_off(
            include_str!("../../../generators/crate/templates/Cargo.toml.tpl"),
            "cargo",
            &context,
        )
        .unwrap();
        assert_eq!(
            cargo.contains("omni_diagnostic = { workspace = true }"),
            enabled
        );

        let project = omni_tera::one_off(
            include_str!(
                "../../../generators/crate/templates/project.omni.yaml.tpl"
            ),
            "project",
            &context,
        )
        .unwrap();
        assert_eq!(
            project.contains("dependencies:\n  - omni_diagnostic"),
            enabled
        );

        let error = omni_tera::one_off(
            include_str!("../../../generators/rust-error/templates/{{ vars.rust_file_name }}.rs.tpl"),
            "error",
            &context,
        )
        .unwrap();
        assert!(
            error.contains(
                "pub struct SampleError(pub(crate) SampleErrorInner)"
            )
        );
        assert_eq!(
            error.contains("omni_diagnostic::register_wrapper!(SampleError"),
            enabled
        );
        assert_eq!(error.contains("SampleErrorInner::Custom(report) => omni_diagnostic::find(report)"), enabled);
    }
}
