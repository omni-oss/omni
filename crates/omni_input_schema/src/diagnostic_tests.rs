use eyre::WrapErr;
use omni_diagnostic::{Audience, find};
use serde_json::json;
use value_bag::ValueBag;

use crate::{Error, error::ErrorInner, input::InputKind};

fn report(inner: ErrorInner) -> eyre::Report {
    Err::<(), _>(Error::from(inner))
        .wrap_err("validating inputs")
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
fn duplicate_name_is_found_through_wrap_err() {
    let d_report = report(ErrorInner::DuplicateInputName("token".into()));
    let d = find(&d_report).expect("diagnostic");
    assert_eq!(d.code(), "input.duplicate_name");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details(), Some(json!({ "input_name": "token" })));
    assert_eq!(
        d.hints(),
        vec![
            "rename one of the inputs named `token` so input names are unique"
        ]
    );
}

#[test]
fn invalid_value_details_carry_only_the_input_name() {
    let r = report(ErrorInner::InvalidValue {
        input_name: "port".into(),
        value: ValueBag::from_str("hunter2").to_owned(),
        error_message: "must be below 1024".into(),
    });
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "input.invalid_value");
    assert_eq!(d.details(), Some(json!({ "input_name": "port" })));
    assert!(!d.details().unwrap().to_string().contains("hunter2"));
    assert!(!d.hints().join(" ").contains("hunter2"));
}

#[test]
fn boolean_result_details_never_echo_the_expression() {
    let r = report(ErrorInner::InvalidBooleanExpressionResult {
        result: "maybe".into(),
        expression: "token == 'abc'".into(),
    });
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "input.invalid_boolean_result");
    assert_eq!(d.details(), None);
    assert!(!d.hints().join(" ").contains("abc"));
}

#[test]
fn secret_remember_conflict_names_the_input() {
    let r = report(ErrorInner::SecretRememberConflict {
        input_name: "api_key".into(),
    });
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "input.secret_remember_conflict");
    assert_eq!(d.details(), Some(json!({ "input_name": "api_key" })));
}

#[test]
fn unsupported_kind_reports_the_input_and_kind() {
    let r = report(ErrorInner::UnsupportedInputKind {
        input_name: "payload".into(),
        kind: InputKind::Object,
    });
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "input.unsupported_kind");
    assert_eq!(
        d.details(),
        Some(json!({ "input_name": "payload", "kind": "object" }))
    );
}

#[test]
fn plain_custom_errors_stay_uncoded() {
    let r = report(ErrorInner::Custom(eyre::eyre!("boom")));
    assert!(find(&r).is_none());
}

#[test]
fn codes_are_registered_in_the_catalog() {
    for code in [
        "input.duplicate_name",
        "input.invalid_value",
        "input.invalid_boolean_result",
        "input.secret_remember_conflict",
        "input.unsupported_kind",
    ] {
        assert!(omni_codes::get(code).is_some(), "{code} is not registered");
    }
}

#[test]
fn summaries_never_echo_input_values_or_rendered_expressions() {
    let invalid = report(ErrorInner::InvalidValue {
        input_name: "token".into(),
        value: ValueBag::from_str("hunter2").to_owned(),
        error_message: "must be at least 12 characters".into(),
    });
    let all = rendered(&invalid);
    assert!(!all.contains("hunter2"), "{all}");
    assert!(all.contains("token"), "{all}");
    assert!(all.contains("must be at least 12 characters"), "{all}");

    let boolean = report(ErrorInner::InvalidBooleanExpressionResult {
        result: "hunter2".into(),
        expression: "{{ value }}".into(),
    });
    let all = rendered(&boolean);
    assert!(!all.contains("hunter2"), "{all}");
    assert!(!all.contains("{{ value }}"), "{all}");
    assert!(all.contains("input.invalid_boolean_result"), "{all}");
}
