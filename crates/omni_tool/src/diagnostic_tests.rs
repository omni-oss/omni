use std::path::PathBuf;

use eyre::WrapErr;
use omni_diagnostic::{Audience, find};

use crate::error::{Error, ErrorInner};

fn report(inner: ErrorInner) -> eyre::Report {
    Err::<(), _>(Error::from(inner))
        .wrap_err("running tool")
        .unwrap_err()
}

/// Everything a user or an agent can see for this error: each message in the
/// chain plus the rendered CLI text and the JSON envelope.
fn rendered(r: &eyre::Report) -> String {
    let chain = r
        .chain()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    let report = omni_diagnostic::DiagnosticReport::from_report(r)
        .expect("diagnostic report");
    format!("{chain}\n{}\n{}", report.to_text(), report.to_json())
}

#[test]
fn tool_not_found_is_a_user_diagnostic() {
    let r = report(ErrorInner::ToolNotFound {
        name: "lint".into(),
    });
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "tool.not_found");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details().unwrap()["name"], "lint");
    assert_eq!(d.hints().len(), 1);
}

#[test]
fn duplicate_tool_name_carries_the_config_path() {
    let r = report(ErrorInner::DuplicateToolName {
        name: "lint".into(),
        path: PathBuf::from("tools/lint/tool.omni.yaml"),
    });
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "tool.duplicate_name");
    assert_eq!(d.audience(), Audience::User);
    let details = d.details().unwrap();
    assert_eq!(details["name"], "lint");
    assert_eq!(details["path"], "tools/lint/tool.omni.yaml");
}

#[test]
fn missing_runtimes_are_environment_diagnostics() {
    let none = report(ErrorInner::NoJsRuntime);
    let d = find(&none).expect("diagnostic");
    assert_eq!(d.code(), "tool.no_js_runtime");
    assert_eq!(d.audience(), Audience::Environment);
    assert_eq!(d.details(), None);

    let selected = report(ErrorInner::RuntimeNotFound {
        runtime: "bun".into(),
    });
    let d = find(&selected).expect("diagnostic");
    assert_eq!(d.code(), "tool.runtime_not_found");
    assert_eq!(d.audience(), Audience::Environment);
    assert_eq!(d.details().unwrap()["runtime"], "bun");
    assert!(d.hints()[0].contains("bun"));
}

#[test]
fn unsupported_backend_names_the_backend() {
    let r = report(ErrorInner::UnsupportedBackend {
        backend: "command".into(),
    });
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "tool.unsupported_backend");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details().unwrap()["backend"], "command");
}

#[test]
fn pipeline_depth_and_if_condition_failures_are_user_diagnostics() {
    let depth = report(ErrorInner::MaxDepthExceeded { max: 8 });
    let d = find(&depth).expect("diagnostic");
    assert_eq!(d.code(), "tool.max_depth_exceeded");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details().unwrap()["max"], 8);

    let cond = report(ErrorInner::InvalidIfCondition {
        step: "build".into(),
        expr: "inputs.secret_token".into(),
        result: "hunter2".into(),
    });
    let d = find(&cond).expect("diagnostic");
    assert_eq!(d.code(), "tool.invalid_if_condition");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details().unwrap(), serde_json::json!({ "step": "build" }));
    assert!(!d.hints().join(" ").contains("hunter2"));
}

#[test]
fn template_failures_are_found_without_leaking_the_template() {
    let err = omni_tera::one_off("{{ unclosed", "step", &Default::default())
        .expect_err("template must not parse");
    let r = report(ErrorInner::Tera(err));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "tool.template_failed");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details(), None);
}

#[test]
fn custom_reports_delegate_and_plain_ones_stay_uncoded() {
    let coded: eyre::Report = Error::from(ErrorInner::NoJsRuntime).into();
    let wrapped: eyre::Report = Error::from(ErrorInner::Custom(coded)).into();
    assert_eq!(find(&wrapped).unwrap().code(), "tool.no_js_runtime");

    let plain: eyre::Report = Error::custom("boom").into();
    assert!(find(&plain).is_none());
}

#[test]
fn plain_failures_stay_uncoded() {
    let serde_err = serde_json::from_str::<u8>("x").unwrap_err();
    assert!(find(&report(ErrorInner::SerdeJson(serde_err))).is_none());

    let enforcement = report(ErrorInner::CapabilityEnforcement {
        message: "no sandbox".into(),
    });
    let diagnostic = find(&enforcement).expect("enforcement diagnostic");
    assert_eq!(diagnostic.code(), "tool.capability_enforcement_failed");
    assert_eq!(diagnostic.audience(), Audience::Environment);
    assert_eq!(diagnostic.details(), None);
}

#[test]
fn codes_are_registered_in_the_catalog() {
    for code in [
        "tool.not_found",
        "tool.duplicate_name",
        "tool.no_js_runtime",
        "tool.runtime_not_found",
        "tool.unsupported_backend",
        "tool.max_depth_exceeded",
        "tool.invalid_if_condition",
        "tool.template_failed",
        "tool.config_load_failed",
        "tool.capability_enforcement_failed",
    ] {
        assert!(
            omni_codes::get(code).is_some(),
            "`{code}` is not registered"
        );
    }
}

#[test]
fn config_load_failure_names_the_file_and_preserves_the_cause() {
    let inner = omni_file_data_serde::ext_to_format(std::ffi::OsStr::new(
        "unsupported",
    ))
    .unwrap_err();
    let report = report(ErrorInner::LoadConfig {
        path: "tool.unsupported".into(),
        inner,
    });
    let diagnostic = find(&report).expect("tool config diagnostic");
    assert_eq!(diagnostic.code(), "tool.config_load_failed");
    assert_eq!(diagnostic.details().unwrap()["path"], "tool.unsupported");
    assert!(
        report
            .chain()
            .any(|e| e.downcast_ref::<omni_file_data_serde::Error>().is_some())
    );
}

#[test]
fn if_condition_summary_never_echoes_the_condition_or_its_result() {
    let r = report(ErrorInner::InvalidIfCondition {
        step: "deploy".into(),
        expr: "{{ inputs.token }}".into(),
        result: "hunter2".into(),
    });
    let all = rendered(&r);
    assert!(!all.contains("hunter2"), "{all}");
    assert!(!all.contains("inputs.token"), "{all}");
    assert!(all.contains("deploy"), "{all}");
    assert!(all.contains("tool.invalid_if_condition"), "{all}");
}
