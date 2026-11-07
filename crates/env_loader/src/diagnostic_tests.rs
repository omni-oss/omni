use eyre::WrapErr;
use omni_diagnostic::{Audience, find};

use crate::error::{EnvLoaderError, EnvLoaderErrorInner};

fn report(inner: EnvLoaderErrorInner) -> eyre::Report {
    Err::<(), _>(EnvLoaderError::from(inner))
        .wrap_err("loading env")
        .unwrap_err()
}

#[test]
fn missing_path_is_a_user_diagnostic() {
    let r = report(EnvLoaderErrorInner::PathDoesNotExist {
        path: "/work/app".into(),
    });
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "env_file.not_found");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details().unwrap()["path"], "/work/app");
    assert!(d.hints()[0].contains("/work/app"));
}

#[test]
fn unreadable_file_is_an_environment_diagnostic() {
    let r = report(EnvLoaderErrorInner::CantReadFile {
        path: "/work/app/.env".into(),
    });
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "env_file.read_failed");
    assert_eq!(d.audience(), Audience::Environment);
    assert_eq!(d.details().unwrap()["path"], "/work/app/.env");
}

#[test]
fn parse_failure_reports_path_and_location_without_contents() {
    let err =
        env::parse_default("TOKEN=hunter2\n=oops").expect_err("must not parse");
    let r = report(EnvLoaderErrorInner::CantParseEnv {
        path: "/work/app/.env".into(),
        err,
    });
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "env_file.parse_failed");
    assert_eq!(d.audience(), Audience::User);
    let details = d.details().unwrap();
    assert_eq!(details["path"], "/work/app/.env");
    assert!(details["line"].is_number());
    assert!(details["column"].is_number());
    assert!(!details.to_string().contains("hunter2"));
    assert!(!d.hints().join(" ").contains("hunter2"));
    // The summary is the Display text; it must not echo the file's contents.
    let chain = r
        .chain()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(!chain.contains("hunter2"), "{chain}");
    assert!(!format!("{r:?}").contains("hunter2"));
    assert!(chain.contains("/work/app/.env"), "{chain}");
}

#[test]
fn io_and_expansion_failures_stay_uncoded() {
    let io = report(EnvLoaderErrorInner::Io(std::io::Error::other("x")));
    assert!(find(&io).is_none());

    let cwd = report(EnvLoaderErrorInner::CantLoadCurrentDir);
    assert!(find(&cwd).is_none());
}

#[test]
fn codes_are_registered_in_the_catalog() {
    for code in [
        "env_file.not_found",
        "env_file.read_failed",
        "env_file.parse_failed",
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
