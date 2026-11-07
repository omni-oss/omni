use eyre::WrapErr;
use omni_diagnostic::{Audience, find};

use crate::error::Error;

fn report(err: Error) -> eyre::Report {
    Err::<(), _>(err).wrap_err("detecting scm").unwrap_err()
}

#[test]
fn no_repository_is_a_user_diagnostic() {
    let r = report(Error::no_repository_found());
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "scm.no_repository_found");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details(), None);
    assert!(d.hints()[0].contains("git repository"));
}

#[test]
fn unsupported_scm_is_a_user_diagnostic() {
    let r = report(Error::unsupported_scm());
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "scm.unsupported");
    assert_eq!(d.audience(), Audience::User);
}

#[test]
fn custom_reports_stay_uncoded_unless_they_carry_a_diagnostic() {
    let plain: eyre::Report = Error::from(eyre::eyre!("boom")).into();
    assert!(find(&plain).is_none());

    let coded: eyre::Report = Error::no_repository_found().into();
    let wrapped: eyre::Report = Error::from(coded).into();
    assert_eq!(find(&wrapped).unwrap().code(), "scm.no_repository_found");
}

#[test]
fn codes_are_registered_in_the_catalog() {
    for code in ["scm.no_repository_found", "scm.unsupported"] {
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
