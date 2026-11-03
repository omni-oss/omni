#![cfg(not(feature = "explicit-registry"))]

use omni_diagnostic::{Audience, Diagnostic, DiagnosticReport, find};
use serde_json::json;

#[derive(Debug, thiserror::Error, Diagnostic)]
#[diagnostic(audience = user)]
enum Sample {
    #[error("unknown project `{name}`")]
    #[diagnostic(
        code = "sample.unknown_project",
        hint = "run `omni projects`",
        hint = "did you mean `{suggestion}`?"
    )]
    Unknown {
        #[diagnostic(detail)]
        name: String,
        #[diagnostic(detail = "did_you_mean")]
        suggestion: String,
    },

    #[error("cannot read config")]
    #[diagnostic(code = "sample.unreadable", audience = environment)]
    Io(#[source] std::io::Error),

    #[error("tuple {0}")]
    #[diagnostic(code = "sample.tuple", hint = "value was {0:?}")]
    Tuple(String),

    #[error("with body")]
    #[diagnostic(code = "sample.body", body = write_body, details = make_details)]
    Body,

    #[error(transparent)]
    #[diagnostic(forward)]
    Inner(#[from] InnerErr),
}

fn write_body(_: &Sample, out: &mut dyn std::fmt::Write) -> std::fmt::Result {
    write!(out, "  line one\n  line two\n")
}

fn make_details(_: &Sample) -> serde_json::Value {
    json!({ "n": 3 })
}

#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("inner failure")]
#[diagnostic(code = "sample.inner", audience = internal)]
struct InnerErr;

#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("generic {0}")]
#[diagnostic(code = "sample.generic", no_register)]
struct Generic<T: std::fmt::Debug + std::fmt::Display + Send + Sync + 'static>(
    T,
);

fn unknown() -> Sample {
    Sample::Unknown {
        name: "web".into(),
        suggestion: "website".into(),
    }
}

#[test]
fn trait_methods_follow_attributes() {
    let e = unknown();
    assert_eq!(e.code(), "sample.unknown_project");
    assert_eq!(e.audience(), Audience::User);
    assert_eq!(
        e.details(),
        Some(json!({ "name": "web", "did_you_mean": "website" }))
    );
    assert_eq!(
        e.hints(),
        vec!["run `omni projects`", "did you mean `website`?"]
    );

    let io = Sample::Io(std::io::Error::other("x"));
    assert_eq!(io.audience(), Audience::Environment);
    assert_eq!(io.details(), None);
}

#[test]
fn tuple_hints_and_escape_hatches() {
    assert_eq!(Sample::Tuple("a".into()).hints(), vec!["value was \"a\""]);
    let b = Sample::Body;
    assert_eq!(b.details(), Some(json!({ "n": 3 })));
    let mut s = String::new();
    b.render_body(&mut s).unwrap();
    assert_eq!(s, "  line one\n  line two\n");
}

#[test]
fn forward_delegates() {
    let e: Sample = InnerErr.into();
    assert_eq!(e.code(), "sample.inner");
    assert_eq!(e.audience(), Audience::Internal);
}

#[test]
fn generic_types_work_without_registration() {
    let g = Generic(5u8);
    assert_eq!(g.code(), "sample.generic");
}

#[test]
fn find_sees_through_wrap_err_layers() {
    use eyre::WrapErr;
    let r: eyre::Report = Err::<(), _>(unknown())
        .wrap_err("planning tasks for `test`")
        .wrap_err("running")
        .unwrap_err();
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "sample.unknown_project");

    let rep = DiagnosticReport::from_report(&r).unwrap();
    assert_eq!(rep.context, vec!["running", "planning tasks for `test`"]);
    let j = rep.to_json();
    assert_eq!(j["kind"], "sample.unknown_project");
    assert_eq!(j["audience"], "user");
    assert_eq!(j["details"]["name"], "web");
    assert_eq!(rep.exit_code(), 2);
    let t = rep.to_text();
    assert!(t.starts_with("error[sample.unknown_project]: unknown project `web`\n  while running\n  while planning"));
    assert!(t.contains("  hint: did you mean `website`?"));
}

#[test]
fn outermost_diagnostic_wins_and_plain_errors_are_none() {
    let plain = eyre::eyre!("boom");
    assert!(find(&plain).is_none());
    assert!(DiagnosticReport::from_report(&plain).is_none());

    let wrapped: eyre::Report = Sample::from(InnerErr).into();
    assert_eq!(find(&wrapped).unwrap().code(), "sample.inner");
}

#[test]
fn registry_is_valid_and_complete() {
    omni_diagnostic::testing::assert_registry_valid();
    omni_diagnostic::testing::assert_types_registered(&["Sample", "InnerErr"]);
    let md = omni_diagnostic::catalog::render(
        omni_diagnostic::catalog::Format::Markdown,
    );
    assert!(md.contains("| `sample.inner` | internal |"));
    assert!(!md.contains("sample.generic"));
}

#[test]
fn body_is_rendered_between_summary_and_hints() {
    let rep = DiagnosticReport::new(&Sample::Body, vec![]);
    assert_eq!(
        rep.to_text(),
        "error[sample.body]: with body\n\n  line one\n  line two\n"
    );
}

#[test]
fn source_chain_is_reported_as_causes() {
    let rep = DiagnosticReport::new(
        &Sample::Io(std::io::Error::other("permission denied")),
        vec![],
    );
    assert_eq!(rep.causes, vec!["permission denied"]);
    assert_eq!(rep.to_json()["causes"], json!(["permission denied"]));
    assert_eq!(
        rep.to_text(),
        "error[sample.unreadable]: cannot read config\n  caused by: permission denied\n"
    );
}
