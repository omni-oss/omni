use eyre::WrapErr;
use omni_core::{ProjectGraphError, TaskExecutionGraphError};
use omni_diagnostic::{Audience, DiagnosticReport, find};

use crate::{
    ExecutionPlanProviderError, FilterError, MetaFilterError, MetaFilterGroup,
    execution_plan_provider::ExecutionPlanProviderErrorInner,
    filter::FilterErrorInner,
};

fn wrapped<E>(error: E) -> eyre::Report
where
    E: std::error::Error + Send + Sync + 'static,
{
    Err::<(), _>(error)
        .wrap_err("planning tasks for `test`")
        .unwrap_err()
}

fn meta_error(total: usize) -> MetaFilterError {
    MetaFilterError {
        filter: "knd == 1".into(),
        total,
        groups: vec![MetaFilterGroup {
            cause: "Undeclared reference to 'knd'".into(),
            count: total,
            samples: vec![],
            omitted: 0,
            hint: None,
        }],
        other: 0,
    }
}

#[test]
fn meta_filter_error_is_found_through_filter_and_plan_layers() {
    let r = wrapped(ExecutionPlanProviderError::from(FilterError::from(
        FilterErrorInner::MetaFilter(meta_error(3)),
    )));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "meta.filter_failed");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details().unwrap()["total"], 3);

    let rep = DiagnosticReport::from_report(&r).unwrap();
    assert_eq!(rep.context, vec!["planning tasks for `test`"]);
}

#[test]
fn meta_filter_error_is_found_through_filter_error_alone() {
    let r = wrapped(FilterError::from(FilterErrorInner::MetaFilter(
        meta_error(2),
    )));
    assert_eq!(find(&r).unwrap().code(), "meta.filter_failed");
}

#[test]
fn meta_filter_error_is_found_in_the_plan_error_directly() {
    let err = ExecutionPlanProviderError::from(
        ExecutionPlanProviderErrorInner::MetaFilter(meta_error(1)),
    );
    assert!(err.meta_filter_error().is_some());

    let r = wrapped(err);
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "meta.filter_failed");
    assert_eq!(d.details().unwrap()["total"], 1);
}

#[test]
fn meta_filter_error_accessor_sees_through_filter_error() {
    let err = ExecutionPlanProviderError::from(FilterError::from(
        FilterErrorInner::MetaFilter(meta_error(4)),
    ));
    assert_eq!(err.meta_filter_error().unwrap().total, 4);
}

#[test]
fn no_project_found_for_filter_is_a_user_diagnostic() {
    let r = wrapped(ExecutionPlanProviderError::from(
        ExecutionPlanProviderErrorInner::NoProjectFoundForFilter {
            filter: "web*".into(),
        },
    ));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "plan.no_project_found_for_filter");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details().unwrap()["filter"], "web*");
    assert_eq!(d.hints().len(), 1);
}

#[test]
fn graph_errors_are_found_through_the_plan_error() {
    let r = wrapped(ExecutionPlanProviderError::from(
        TaskExecutionGraphError::task_not_found("app", "build"),
    ));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "task.not_found");
    assert_eq!(d.details().unwrap()["task"], "build");

    let r = wrapped(ExecutionPlanProviderError::from(
        TaskExecutionGraphError::project_graph(ProjectGraphError::not_found(
            "web",
        )),
    ));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "project.not_found");
    assert_eq!(d.details().unwrap()["project"], "web");
}

#[test]
fn context_reports_delegate_to_their_cause_chain() {
    let coded = FilterError::from(FilterErrorInner::MetaFilter(meta_error(1)));
    let r = wrapped(ExecutionPlanProviderError::from(
        ExecutionPlanProviderErrorInner::Context(eyre::Report::new(coded)),
    ));
    assert_eq!(find(&r).unwrap().code(), "meta.filter_failed");
}

#[test]
fn plain_failures_stay_uncoded() {
    let r = wrapped(ExecutionPlanProviderError::from(
        ExecutionPlanProviderErrorInner::Context(eyre::eyre!("boom")),
    ));
    assert!(find(&r).is_none());

    let glob = globset::Glob::new("[").unwrap_err();
    let r = wrapped(FilterError::from(FilterErrorInner::Glob(glob)));
    assert!(find(&r).is_none());

    let r = wrapped(FilterError::from(FilterErrorInner::Scm(
        omni_scm::error::Error::no_repository_found(),
    )));
    assert!(find(&r).is_none());
}

#[test]
fn codes_are_registered_in_the_catalog() {
    for code in [
        "plan.no_project_found_for_filter",
        "meta.filter_failed",
        "meta.filter_excluded",
    ] {
        assert!(
            omni_codes::get(code).is_some(),
            "`{code}` is not registered"
        );
    }
}

#[test]
fn registry_accepts_the_declared_prefixes() {
    if let Err(errors) = omni_codes::validate() {
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        panic!("invalid code registry:\n{}", msgs.join("\n"));
    }
}
