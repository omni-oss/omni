use eyre::WrapErr;
use omni_diagnostic::{Audience, find};

use crate::{Context, Error, ErrorInner, parse};

fn wrapped(error: Error) -> eyre::Report {
    Err::<(), _>(error)
        .wrap_err("evaluating filter")
        .unwrap_err()
}

fn eval_error(expr: &str) -> Error {
    parse(expr)
        .and_then(|e| e.evaluate_bool(&Context::default()).map(|_| ()))
        .unwrap_err()
}

#[test]
fn syntax_errors_are_coded_without_the_expression_text() {
    let err = parse("secret_token ==").unwrap_err();
    let r = wrapped(err);
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "expression.invalid");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details(), None);
    assert_eq!(d.hints().len(), 1);
    assert!(!d.to_string().contains("secret_token"));
    assert!(!d.hints().join(" ").contains("secret_token"));
}

#[test]
fn undeclared_names_are_coded_with_the_name() {
    let r = wrapped(eval_error("knd == \"e2e\""));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "expression.undeclared_reference");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details().unwrap()["name"], "knd");
}

#[test]
fn undeclared_functions_are_coded_with_the_name() {
    let r = wrapped(eval_error("\"a\".startWith(\"a\")"));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "expression.undeclared_reference");
    assert_eq!(d.details().unwrap()["name"], "startWith");
}

#[test]
fn non_boolean_results_carry_only_the_kind() {
    let r = wrapped(eval_error("\"hunter2\""));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "expression.not_boolean");
    assert_eq!(d.details().unwrap()["got"], "string");
    assert_eq!(d.details().unwrap().as_object().unwrap().len(), 1);
    assert!(!d.to_string().contains("hunter2"));
    assert!(!d.hints().join(" ").contains("hunter2"));
}

#[test]
fn missing_variables_are_coded_with_the_name() {
    let err: Error = ErrorInner::VariableNotFound {
        name: "meta".into(),
    }
    .into();
    let r = wrapped(err);
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "expression.variable_not_found");
    assert_eq!(d.details().unwrap()["name"], "meta");
}

#[test]
fn runtime_failures_stay_uncoded() {
    assert!(find(&wrapped(eval_error("1 / 0 == 1"))).is_none());
    assert!(find(&wrapped(eval_error("1 + \"a\" == 1"))).is_none());
}

#[test]
fn new_codes_are_registered_in_the_catalog() {
    for code in [
        "expression.invalid",
        "expression.undeclared_reference",
        "expression.not_boolean",
        "expression.variable_not_found",
    ] {
        assert!(
            omni_codes::get(code).is_some(),
            "`{code}` is not registered"
        );
    }
}

#[test]
fn registry_accepts_the_declared_prefix() {
    if let Err(errors) = omni_codes::validate() {
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        panic!("invalid code registry:\n{}", msgs.join("\n"));
    }
}
