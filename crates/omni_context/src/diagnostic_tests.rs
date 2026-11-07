use std::path::PathBuf;

use eyre::WrapErr;
use omni_diagnostic::{Audience, DiagnosticReport, find};

use crate::{
    context::{ContextError, ContextErrorInner},
    extracted_data_validator::{
        ExtractedDataValidationError, ExtractedDataValidationErrorInner,
    },
    project_config_loader::{
        ProjectConfigLoaderError, ProjectConfigLoaderErrorInner,
    },
    project_data_extractor::{
        ProjectDataExtractorError, ProjectDataExtractorErrorInner,
    },
};

fn duplicate(name: &str) -> ExtractedDataValidationError {
    ExtractedDataValidationErrorInner::DuplicateProjectName {
        project_name: name.into(),
        project_paths: vec![PathBuf::from("a"), PathBuf::from("b")],
    }
    .into()
}

fn dangling(task: &str) -> ExtractedDataValidationError {
    ExtractedDataValidationErrorInner::DanglingTaskDependency {
        project_name: "app".into(),
        task_name: task.into(),
        dependency: "helper".into(),
    }
    .into()
}

fn report(inner: ContextErrorInner) -> eyre::Report {
    Err::<(), _>(ContextError::from(inner))
        .wrap_err("loading workspace")
        .unwrap_err()
}

#[test]
fn validation_failures_are_aggregated_structurally() {
    let r = report(ContextErrorInner::ValidationError(
        vec![duplicate("web"), dangling("build")].into(),
    ));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "workspace.validation_failed");
    assert_eq!(d.audience(), Audience::User);
    let details = d.details().unwrap();
    let errors = details["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 2);
    assert_eq!(errors[0]["code"], "workspace.duplicate_project_name");
    assert_eq!(errors[0]["details"]["project"], "web");
    assert_eq!(errors[0]["details"]["paths"], serde_json::json!(["a", "b"]));
    assert_eq!(errors[1]["code"], "workspace.dangling_task_dependency");
    assert_eq!(errors[1]["details"]["dependency"], "helper");
    assert_eq!(d.hints().len(), 2);

    let text = DiagnosticReport::from_report(&r).unwrap().to_text();
    assert!(text.contains("[workspace.duplicate_project_name]"), "{text}");
    assert!(text.contains("[workspace.dangling_task_dependency]"), "{text}");
}

#[test]
fn workspace_not_found_is_a_user_diagnostic() {
    let r = report(ContextErrorInner::FailedToFindWorkspaceConfiguration);
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "workspace.not_found");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.hints().len(), 1);
}

#[test]
fn project_loader_errors_are_found_through_the_context_error() {
    let loader: ProjectConfigLoaderError =
        ProjectConfigLoaderErrorInner::ExtendedProjectConfigOutsideWorkspace {
            extender_path: PathBuf::from("apps/app/project.omni.yaml"),
            extendee_path: PathBuf::from("../shared/base.omni.yaml"),
        }
        .into();
    let r = report(ContextErrorInner::ProjectLoader(loader));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "workspace.extends_outside_workspace");
    assert_eq!(
        d.details().unwrap()["extender"],
        "apps/app/project.omni.yaml"
    );
    assert!(d.hints()[0].contains("apps/app/project.omni.yaml"));
}

#[test]
fn invalid_redact_is_found_through_the_context_error() {
    let inner: ProjectDataExtractorError =
        ProjectDataExtractorErrorInner::InvalidRedact {
            location: "project `app`".into(),
            message: "bad pattern".into(),
        }
        .into();
    let r = report(ContextErrorInner::ProjectDataExtractor(inner));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "workspace.invalid_redact");
    assert_eq!(d.details().unwrap()["reason"], "bad pattern");
}

#[test]
fn plain_failures_stay_uncoded() {
    let cache = report(ContextErrorInner::CacheDirectory("boom".into()));
    assert!(find(&cache).is_none());

    let io = report(ContextErrorInner::Io(std::io::Error::other("x")));
    assert!(find(&io).is_none());

    let plain: ProjectDataExtractorError =
        ProjectDataExtractorErrorInner::Unknown(eyre::eyre!("plain")).into();
    assert!(
        find(&report(ContextErrorInner::ProjectDataExtractor(plain))).is_none()
    );
}

#[test]
fn codes_are_registered_in_the_catalog() {
    for code in [
        "workspace.not_found",
        "workspace.config_load_failed",
        "workspace.worktree_config_load_failed",
        "workspace.validation_failed",
        "workspace.duplicate_project_name",
        "workspace.dangling_task_dependency",
        "workspace.extends_not_found",
        "workspace.extends_outside_workspace",
        "workspace.project_config_not_found",
        "workspace.invalid_redact",
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
