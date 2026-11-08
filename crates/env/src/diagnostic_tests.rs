use eyre::WrapErr;
use omni_diagnostic::{Audience, find};

use crate::{EnvParseError, ExpansionError, parse_default};

fn report(err: EnvParseError) -> eyre::Report {
    Err::<(), _>(err).wrap_err("loading env").unwrap_err()
}

#[test]
fn syntax_error_reports_only_its_location() {
    let err =
        parse_default("TOKEN=hunter2\n=oops").expect_err("must not parse");
    let r = report(err);
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "env_parse.syntax_error");
    assert_eq!(d.audience(), Audience::User);
    let details = d.details().unwrap();
    assert!(details["line"].is_number());
    assert!(details["column"].is_number());
    assert!(!details.to_string().contains("hunter2"));
    assert!(!d.hints().join(" ").contains("hunter2"));
}

#[test]
fn io_and_expansion_failures_stay_uncoded() {
    let io = EnvParseError::from(std::io::Error::other("x"));
    assert!(find(&report(io)).is_none());

    let expansion =
        EnvParseError::from(ExpansionError::from(std::io::Error::other("x")));
    assert!(find(&report(expansion)).is_none());
}

#[test]
fn codes_are_registered_in_the_catalog() {
    assert!(omni_codes::get("env_parse.syntax_error").is_some());
}

#[test]
fn registry_accepts_the_declared_prefix() {
    if let Err(errors) = omni_codes::validate() {
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        panic!("invalid code registry:\n{}", msgs.join("\n"));
    }
}
