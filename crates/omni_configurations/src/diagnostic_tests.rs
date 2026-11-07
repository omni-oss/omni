use eyre::WrapErr;
use omni_core::ExtensionGraphError;
use omni_diagnostic::{Audience, find};

use crate::task_extension::{TaskExtensionError, TaskExtensionErrorInner};

fn wrapped(error: TaskExtensionError) -> eyre::Report {
    Err::<(), _>(error)
        .wrap_err("resolving task extensions")
        .unwrap_err()
}

#[test]
fn unknown_extendee_is_found_after_wrap_err() {
    let r = wrapped(TaskExtensionError::from(
        TaskExtensionErrorInner::UnknownExtendee {
            task: "derived".into(),
            extendee: "base_task".into(),
        },
    ));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "task_extension.unknown_extendee");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details().unwrap()["task"], "derived");
    assert_eq!(d.details().unwrap()["extendee"], "base_task");
    assert_eq!(d.hints().len(), 1);
}

#[test]
fn cyclic_extension_is_coded() {
    let r = wrapped(TaskExtensionError::from(TaskExtensionErrorInner::Cycle {
        message: "a -> b -> a".into(),
    }));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "task_extension.cycle");
    assert_eq!(d.audience(), Audience::User);
}

#[test]
fn plain_extension_graph_failures_stay_uncoded() {
    let r = wrapped(TaskExtensionError::from(ExtensionGraphError::from(
        eyre::eyre!("boom"),
    )));
    assert!(find(&r).is_none());
}

#[test]
fn new_codes_are_registered_in_the_catalog() {
    for code in ["task_extension.unknown_extendee", "task_extension.cycle"] {
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
