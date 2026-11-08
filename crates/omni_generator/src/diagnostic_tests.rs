use std::path::PathBuf;

use eyre::WrapErr;
use omni_diagnostic::{Audience, find};

use crate::{
    action_handlers::utils::{
        ResolveOutputPathError, ResolveOutputPathErrorInner,
    },
    error::{Error, ErrorInner},
};

fn report(inner: ErrorInner) -> eyre::Report {
    Err::<(), _>(Error::from(inner))
        .wrap_err("running generator")
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
fn generator_not_found_is_a_user_diagnostic() {
    let r = report(ErrorInner::GeneratorNotFound { name: "app".into() });
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "generator.not_found");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details().unwrap()["name"], "app");
    assert_eq!(d.hints().len(), 1);
}

#[test]
fn duplicate_generator_name_carries_the_config_path() {
    let r = report(ErrorInner::DuplicateGeneratorName {
        name: "app".into(),
        path: PathBuf::from("gens/app/generator.omni.yaml"),
    });
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "generator.duplicate_name");
    let details = d.details().unwrap();
    assert_eq!(details["name"], "app");
    assert_eq!(details["path"], "gens/app/generator.omni.yaml");
}

#[test]
fn invalid_boolean_result_never_reports_the_expression_or_result() {
    let r = report(ErrorInner::InvalidBooleanResult {
        result: "hunter2".into(),
        expr: "inputs.secret_token".into(),
        expr_name: "enabled".into(),
    });
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "generator.invalid_boolean_result");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(
        d.details().unwrap(),
        serde_json::json!({ "expr_name": "enabled" })
    );
    let text = d.hints().join(" ");
    assert!(!text.contains("hunter2"));
    assert!(!text.contains("secret_token"));
    // The summary is the Display text and also reaches MCP error data.
    let all = rendered(&r);
    assert!(!all.contains("hunter2"), "{all}");
    assert!(!all.contains("secret_token"), "{all}");
    assert!(all.contains("enabled"), "{all}");
}

#[test]
fn command_failure_reports_the_exit_code_but_not_the_command() {
    let r = report(ErrorInner::CommandFailed {
        command: "deploy --token hunter2".into(),
        exit_code: 2,
    });
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "generator.command_failed");
    assert_eq!(d.details().unwrap(), serde_json::json!({ "exit_code": 2 }));
    assert!(!d.hints().join(" ").contains("hunter2"));
    let all = rendered(&r);
    assert!(!all.contains("hunter2"), "{all}");
    assert!(!all.contains("--token"), "{all}");
    assert!(
        all.contains("deploy"),
        "the program should still be named: {all}"
    );
}

#[test]
fn io_failures_on_files_are_environment_diagnostics() {
    let write = report(ErrorInner::FailedToWriteFile {
        path: PathBuf::from("out/a.txt"),
        error: std::io::Error::other("denied"),
    });
    let d = find(&write).expect("diagnostic");
    assert_eq!(d.code(), "generator.write_failed");
    assert_eq!(d.audience(), Audience::Environment);
    assert_eq!(d.details().unwrap()["path"], "out/a.txt");

    let read = report(ErrorInner::FailedToReadFile {
        path: PathBuf::from("tpl/a.txt"),
        error: std::io::Error::other("denied"),
    });
    let d = find(&read).expect("diagnostic");
    assert_eq!(d.code(), "generator.read_failed");
    assert_eq!(d.audience(), Audience::Environment);
    assert_eq!(d.details().unwrap()["path"], "tpl/a.txt");

    let missing = report(ErrorInner::FileNotFound {
        path: PathBuf::from("tpl/b.txt"),
        error: std::io::Error::from(std::io::ErrorKind::NotFound),
    });
    let d = find(&missing).expect("diagnostic");
    assert_eq!(d.code(), "generator.file_not_found");
    assert_eq!(d.details().unwrap()["path"], "tpl/b.txt");
}

#[test]
fn path_that_is_not_a_directory_names_the_path() {
    let r = report(ErrorInner::PathExistsButNotDir {
        path: PathBuf::from("out"),
    });
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "generator.path_not_dir");
    assert_eq!(d.details().unwrap()["path"], "out");
}

#[test]
fn capability_errors_keep_their_own_codes_and_name_the_generator() {
    let r = report(ErrorInner::InvalidCapabilities {
        name: "app".into(),
        config_path: PathBuf::from("gens/app/generator.omni.yaml"),
        source: omni_capabilities::Error::custom("bad policy"),
    });
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "generator.invalid_capabilities");
    let details = d.details().unwrap();
    assert_eq!(details["name"], "app");
    assert_eq!(details["config_path"], "gens/app/generator.omni.yaml");

    let workspace = report(ErrorInner::InvalidWorkspaceCapabilities {
        source: omni_capabilities::Error::custom("bad policy"),
    });
    let d = find(&workspace).expect("diagnostic");
    assert_eq!(d.code(), "generator.invalid_workspace_capabilities");
    assert_eq!(d.details(), None);
}

#[test]
fn unsafe_output_targets_are_user_diagnostics() {
    let absolute =
        report(ErrorInner::ResolveOutputPath(ResolveOutputPathError::from(
            ResolveOutputPathErrorInner::TargetIsAbsolute {
                target: PathBuf::from("/etc"),
            },
        )));
    let d = find(&absolute).expect("diagnostic");
    assert_eq!(d.code(), "generator.target_absolute");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details().unwrap()["target"], "/etc");

    let outside =
        report(ErrorInner::ResolveOutputPath(ResolveOutputPathError::from(
            ResolveOutputPathErrorInner::TargetIsOutsideOutputDir {
                target: PathBuf::from("/out/../etc"),
                output_dir: PathBuf::from("/out"),
            },
        )));
    let d = find(&outside).expect("diagnostic");
    assert_eq!(d.code(), "generator.target_outside_output");
    let details = d.details().unwrap();
    assert_eq!(details["target"], "/out/../etc");
    assert_eq!(details["output_dir"], "/out");
}

#[test]
fn custom_reports_delegate_and_plain_ones_stay_uncoded() {
    let coded: eyre::Report =
        Error::from(ErrorInner::GeneratorNotFound { name: "app".into() })
            .into();
    let wrapped: eyre::Report = Error::from(ErrorInner::Custom(coded)).into();
    assert_eq!(find(&wrapped).unwrap().code(), "generator.not_found");

    let plain: eyre::Report = Error::custom("boom").into();
    assert!(find(&plain).is_none());
}

#[test]
fn plain_failures_stay_uncoded() {
    let serde_err = serde_json::from_str::<u8>("x").unwrap_err();
    assert!(find(&report(ErrorInner::SerdeJson(serde_err))).is_none());

    let io = report(ErrorInner::GenericIo(std::io::Error::other("x")));
    assert!(find(&io).is_none());
}

#[test]
fn codes_are_registered_in_the_catalog() {
    for code in [
        "generator.not_found",
        "generator.config_load_failed",
        "generator.invalid_boolean_result",
        "generator.duplicate_name",
        "generator.write_failed",
        "generator.read_failed",
        "generator.path_not_dir",
        "generator.file_not_found",
        "generator.command_failed",
        "generator.not_invocable",
        "generator.recursion",
        "generator.max_depth_exceeded",
        "generator.invalid_capabilities",
        "generator.invalid_workspace_capabilities",
        "generator.target_absolute",
        "generator.target_outside_output",
    ] {
        assert!(
            omni_codes::get(code).is_some(),
            "`{code}` is not registered"
        );
    }
}
