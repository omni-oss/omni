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

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
struct Wrapper(InnerErr);

impl Wrapper {
    fn diag(&self) -> Option<&dyn Diagnostic> {
        Some(&self.0)
    }
}

omni_diagnostic::register_wrapper!(Wrapper, |w| w.diag());

#[test]
fn wrapper_exposes_a_diagnostic_hidden_by_transparent() {
    use std::error::Error;
    let r: eyre::Report = Wrapper(InnerErr).into();
    assert!(r.chain().all(|e| e.downcast_ref::<InnerErr>().is_none()));
    assert!(Wrapper(InnerErr).source().is_none());
    assert_eq!(find(&r).unwrap().code(), "sample.inner");
    assert_eq!(
        DiagnosticReport::from_report(&r).unwrap().code,
        "sample.inner"
    );
}

// Mirrors the output of generators/rust-error with `use_diagnostics` on.
#[derive(Debug, thiserror::Error, Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
struct GeneratedWrapper(GeneratedInner);

#[derive(Debug, thiserror::Error, Diagnostic)]
enum GeneratedInner {
    #[error(transparent)]
    #[diagnostic(delegate)]
    Custom(#[from] eyre::Report),
}

#[test]
fn generated_wrapper_forwards_custom_report_diagnostics() {
    use eyre::WrapErr;

    let inner = Err::<(), _>(unknown())
        .wrap_err("loading project")
        .unwrap_err();
    let report: eyre::Report =
        GeneratedWrapper(GeneratedInner::Custom(inner)).into();
    let diagnostic = find(&report).expect("diagnostic inside custom report");
    assert_eq!(diagnostic.code(), "sample.unknown_project");
    assert_eq!(diagnostic.audience(), Audience::User);
    assert_eq!(diagnostic.details().unwrap()["name"], "web");
    assert_eq!(diagnostic.hints().len(), 2);
}

#[test]
fn generated_wrapper_leaves_plain_custom_errors_uncoded() {
    let report: eyre::Report = GeneratedWrapper(GeneratedInner::Custom(
        eyre::eyre!("plain custom message"),
    ))
    .into();
    assert!(find(&report).is_none());
}

fn hints(_: &ShadowCb) -> Vec<String> {
    vec!["from callback".into()]
}

#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("shadow")]
#[diagnostic(code = "sample.shadow", hints = hints)]
struct ShadowCb {
    #[allow(dead_code)]
    hints: String,
}

#[test]
fn callbacks_are_not_shadowed_by_fields() {
    let e = ShadowCb {
        hints: "field".into(),
    };
    assert_eq!(e.hints(), vec!["from callback"]);
}

#[cfg(not(feature = "explicit-registry"))]
mod codes_registry {
    use omni_codes::Code;

    use super::*;

    #[test]
    fn diagnostic_code_matches_code_info() {
        let cases = [
            Sample::Unknown {
                name: "a".into(),
                suggestion: "b".into(),
            },
            Sample::Body,
            Sample::Tuple("x".into()),
        ];
        // `Sample` forwards one variant, so it has no static `Code` impl;
        // the registry entries still exist for its own codes.
        for c in &cases {
            let code = Diagnostic::code(c);
            let info = omni_codes::get(code).expect("registered");
            assert_eq!(info.code, code);
            assert_eq!(info.audience(), Some(Diagnostic::audience(c)));
        }
    }

    #[derive(Debug, thiserror::Error, Diagnostic)]
    #[error("plain")]
    #[diagnostic(code = "plain.thing", audience = environment, stable)]
    /// Plain thing happened.
    ///
    /// More detail.
    struct Plain;

    #[test]
    fn code_impl_matches_diagnostic() {
        assert_eq!(Code::info(&Plain).code, Diagnostic::code(&Plain));
        let i = Code::info(&Plain);
        assert_eq!(i.summary, "Plain thing happened.");
        assert_eq!(i.explanation, "More detail.");
        assert!(i.stable);
        assert_eq!(i.audience(), Some(omni_codes::Audience::Environment));
    }
}

/// A public wrapper over an enum that mixes owned codes with variants that
/// carry no diagnostic of their own.
#[derive(Debug, thiserror::Error, Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
struct Public(PublicInner);

#[derive(Debug, thiserror::Error, Diagnostic)]
enum PublicInner {
    /// The thing is missing.
    #[error("missing `{name}`")]
    #[diagnostic(code = "mixed.missing", hint = "create `{name}`")]
    Missing {
        #[diagnostic(detail)]
        name: String,
    },

    #[error(transparent)]
    #[diagnostic(skip)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    #[diagnostic(delegate)]
    Wrapped(InnerErr),

    #[error(transparent)]
    #[diagnostic(delegate)]
    Custom(#[from] eyre::Report),
}

fn public(inner: PublicInner) -> eyre::Report {
    Public(inner).into()
}

#[test]
fn owned_variant_is_found_through_a_transparent_wrapper() {
    let r = public(PublicInner::Missing { name: "a".into() });
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "mixed.missing");
    assert_eq!(d.details().unwrap()["name"], "a");
    assert_eq!(d.hints(), vec!["create `a`"]);
    assert!(d.reportable());
}

#[test]
fn skipped_variant_is_a_plain_error() {
    let r = public(PublicInner::Io(std::io::Error::other("x")));
    assert!(find(&r).is_none());
    assert!(DiagnosticReport::from_report(&r).is_none());
}

#[test]
fn delegate_finds_the_inner_diagnostic() {
    let r = public(PublicInner::Wrapped(InnerErr));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "sample.inner");
    assert_eq!(d.audience(), Audience::Internal);
}

#[test]
fn delegate_sees_through_wrap_err_and_ignores_plain_reports() {
    use eyre::WrapErr;
    let coded = Err::<(), _>(unknown()).wrap_err("loading").unwrap_err();
    let r = public(PublicInner::Custom(coded));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "sample.unknown_project");
    assert_eq!(d.details().unwrap()["name"], "web");

    let plain = public(PublicInner::Custom(eyre::eyre!("boom")));
    assert!(find(&plain).is_none());
}

#[test]
fn mixed_types_register_their_own_codes() {
    omni_diagnostic::testing::assert_registry_valid();
    omni_diagnostic::testing::assert_types_registered(&[
        "Public",
        "PublicInner",
    ]);
    let info = omni_codes::get("mixed.missing").expect("registered");
    assert_eq!(info.summary, "The thing is missing.");
}
