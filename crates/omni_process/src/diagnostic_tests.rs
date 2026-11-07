use eyre::WrapErr;
use omni_diagnostic::{Audience, find};

use crate::{ChildError, ChildProcess, ChildProcessError};

const MISSING_PROGRAM: &str = "omni-no-such-program-for-diagnostic-tests";

fn report(err: ChildProcessError) -> eyre::Report {
    Err::<(), _>(err).wrap_err("running task").unwrap_err()
}

#[test]
fn no_command_is_a_user_diagnostic() {
    let r = report(ChildProcessError::no_command());
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "process.no_command");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details(), None);
}

#[test]
fn empty_program_reports_only_the_argument_count() {
    let r = report(ChildProcessError::empty_program_with_args(
        "",
        vec!["--token=s3cret".to_string()],
    ));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "process.empty_program");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details().unwrap()["args"], 1);
    assert!(!d.details().unwrap().to_string().contains("s3cret"));
    assert!(!d.hints().join(" ").contains("s3cret"));
    // The summary is the Display text; it must not echo the arguments either.
    let chain = r
        .chain()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(!chain.contains("s3cret"), "{chain}");
    assert!(!format!("{r:?}").contains("s3cret"));
}

#[tokio::test]
async fn spawn_failure_names_the_program_but_not_its_arguments() {
    let err = ChildProcess::new(
        MISSING_PROGRAM,
        vec!["--token=s3cret".to_string()],
        std::env::current_dir().unwrap(),
    )
    .exec()
    .await
    .expect_err("must not spawn");

    let r = report(err);
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "process.spawn_failed");
    assert_eq!(d.audience(), Audience::Environment);
    assert_eq!(d.details().unwrap()["program"], MISSING_PROGRAM);
    assert!(!d.details().unwrap().to_string().contains("s3cret"));
    assert!(!d.hints().join(" ").contains("s3cret"));
}

#[test]
fn spawn_failure_is_found_through_the_child_error() {
    let err: ChildError = crate::Child::spawn(
        MISSING_PROGRAM,
        Vec::<String>::new(),
        std::env::current_dir().unwrap(),
        std::collections::HashMap::new(),
    )
    .err()
    .expect("must not spawn");
    let r: eyre::Report = err.into();
    assert_eq!(find(&r).unwrap().code(), "process.spawn_failed");
}

#[test]
fn plain_failures_stay_uncoded() {
    use crate::child_process::ChildProcessErrorInner;

    let io = report(ChildProcessError::from(ChildProcessErrorInner::Io(
        std::io::Error::other("x"),
    )));
    assert!(find(&io).is_none());

    let stdin = report(ChildProcessError::from(
        ChildProcessErrorInner::CantTakeStdin,
    ));
    assert!(find(&stdin).is_none());

    let plain = report(ChildProcessError::custom(eyre::eyre!("boom")));
    assert!(find(&plain).is_none());
}

#[test]
fn custom_reports_delegate_to_their_diagnostic() {
    let coded = report(ChildProcessError::no_command());
    let r = report(ChildProcessError::custom(coded));
    assert_eq!(find(&r).unwrap().code(), "process.no_command");
}

#[test]
fn codes_are_registered_in_the_catalog() {
    for code in [
        "process.spawn_failed",
        "process.no_command",
        "process.empty_program",
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
