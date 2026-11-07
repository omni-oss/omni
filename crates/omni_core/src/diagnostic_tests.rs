use eyre::WrapErr;
use omni_diagnostic::{Audience, DiagnosticReport, find};

use crate::{
    ExtensionGraphError, ProjectGraphError, TaskExecutionGraphError,
    extension_graph::ExtensionGraphErrorInner,
};

fn wrapped<E>(error: E) -> eyre::Report
where
    E: std::error::Error + Send + Sync + 'static,
{
    Err::<(), _>(error)
        .wrap_err("planning tasks for `test`")
        .unwrap_err()
}

#[test]
fn project_graph_errors_are_found_after_wrap_err() {
    let r = wrapped(ProjectGraphError::not_found("web"));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "project.not_found");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details().unwrap()["project"], "web");
    assert_eq!(d.hints().len(), 1);

    let rep = DiagnosticReport::from_report(&r).unwrap();
    assert_eq!(rep.context, vec!["planning tasks for `test`"]);
}

#[test]
fn project_graph_cycles_and_duplicates_are_coded() {
    let r = wrapped(ProjectGraphError::already_exists("web"));
    let d = find(&r).unwrap();
    assert_eq!(d.code(), "project.already_exists");
    assert_eq!(d.details().unwrap()["project"], "web");

    let r = wrapped(ProjectGraphError::cyclic_dependency(
        "app".into(),
        "lib".into(),
    ));
    let d = find(&r).unwrap();
    assert_eq!(d.code(), "project.cyclic_dependency");
    assert_eq!(d.details().unwrap()["from"], "app");
    assert_eq!(d.details().unwrap()["to"], "lib");

    let r = wrapped(ProjectGraphError::cycle_detected("app".into()));
    let d = find(&r).unwrap();
    assert_eq!(d.code(), "project.cycle_detected");
    assert_eq!(d.details().unwrap()["project"], "app");
}

#[test]
fn task_graph_errors_are_coded() {
    let r = wrapped(TaskExecutionGraphError::task_not_found("app", "build"));
    let d = find(&r).unwrap();
    assert_eq!(d.code(), "task.not_found");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details().unwrap()["project"], "app");
    assert_eq!(d.details().unwrap()["task"], "build");

    let r = wrapped(TaskExecutionGraphError::cycle_detected(
        "app", "a", "app", "b",
    ));
    let d = find(&r).unwrap();
    assert_eq!(d.code(), "task.cycle_detected");
    assert_eq!(d.details().unwrap()["from_task"], "a");
    assert_eq!(d.details().unwrap()["to_task"], "b");

    let r = wrapped(TaskExecutionGraphError::cant_depend_on_persistent_task(
        "app", "a", "app", "dev",
    ));
    let d = find(&r).unwrap();
    assert_eq!(d.code(), "task.persistent_dependency");
    assert_eq!(d.details().unwrap()["to_task"], "dev");

    let r = wrapped(TaskExecutionGraphError::sibling_dependency_cycle(vec![
        "a".into(),
        "b".into(),
    ]));
    let d = find(&r).unwrap();
    assert_eq!(d.code(), "task.sibling_cycle");
    assert_eq!(d.details().unwrap()["tasks"], serde_json::json!(["a", "b"]));
}

#[test]
fn project_graph_error_is_found_through_task_graph_error() {
    let r = wrapped(TaskExecutionGraphError::project_graph(
        ProjectGraphError::not_found("web"),
    ));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "project.not_found");
    assert_eq!(d.details().unwrap()["project"], "web");
}

#[test]
fn extension_cycle_is_coded_and_invariants_are_plain() {
    let r = wrapped(ExtensionGraphError::from(
        ExtensionGraphErrorInner::CyclicDependency {
            message: "a -> b -> a".into(),
        },
    ));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "project.extends_cycle");

    let r = wrapped(ExtensionGraphError::from(
        ExtensionGraphErrorInner::NodeNotFound {
            message: "x".into(),
        },
    ));
    assert!(find(&r).is_none());
}

#[test]
fn plain_failures_stay_uncoded() {
    let r = wrapped(ProjectGraphError::unknown(eyre::eyre!("boom")));
    assert!(find(&r).is_none());

    let r = wrapped(TaskExecutionGraphError::from(eyre::eyre!("boom")));
    assert!(find(&r).is_none());

    let r = wrapped(ExtensionGraphError::from(eyre::eyre!("boom")));
    assert!(find(&r).is_none());
}

#[test]
fn new_codes_are_registered_in_the_catalog() {
    for code in [
        "project.already_exists",
        "project.not_found",
        "project.cyclic_dependency",
        "project.cycle_detected",
        "project.extends_cycle",
        "task.not_found",
        "task.cycle_detected",
        "task.persistent_dependency",
        "task.sibling_cycle",
    ] {
        assert!(
            omni_codes::get(code).is_some(),
            "`{code}` is not registered"
        );
    }
}
