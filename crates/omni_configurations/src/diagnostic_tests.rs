use eyre::WrapErr;
use omni_core::ExtensionGraphError;
use omni_diagnostic::{Audience, find};
use std::path::Path;
use system_traits::{FsCreateDirAll as _, FsWrite, impls::InMemorySys};

use crate::task_extension::{TaskExtensionError, TaskExtensionErrorInner};
use crate::utils::fs::{LoadConfigError, load_config};

fn wrapped(error: TaskExtensionError) -> eyre::Report {
    Err::<(), _>(error)
        .wrap_err("resolving task extensions")
        .unwrap_err()
}

fn wrapped_load(error: LoadConfigError) -> eyre::Report {
    Err::<(), _>(error).wrap_err("loading config").unwrap_err()
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

#[test]
fn missing_config_file_is_coded_with_its_path() {
    let sys = InMemorySys::default();
    let err =
        load_config::<serde_json::Value, _, _>(Path::new("missing.yaml"), &sys)
            .unwrap_err();
    let r = wrapped_load(err);
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "config.file_not_found");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details().unwrap()["path"], "missing.yaml");
    assert!(d.hints()[0].contains("missing.yaml"));
    assert!(format!("{r:#}").contains("file not found: missing.yaml"));
}

#[test]
fn unparsable_config_keeps_the_cause_chain() {
    let sys = InMemorySys::default();
    sys.fs_create_dir_all(Path::new("cfg")).expect("mkdir");
    sys.fs_write(Path::new("cfg/bad.yaml"), "name: [")
        .expect("write");
    let err =
        load_config::<serde_json::Value, _, _>(Path::new("cfg/bad.yaml"), &sys)
            .unwrap_err();
    let r = wrapped_load(err);
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "config.load_failed");
    assert_eq!(d.audience(), Audience::User);
    let details = d.details().unwrap();
    assert_eq!(details["path"], "cfg/bad.yaml");
    assert_eq!(details["kind"], "ParseFailed");
    assert!(!details.to_string().contains("name"));
    assert!(
        format!("{r:#}")
            .contains("error when loading config from cfg/bad.yaml")
    );
}

#[test]
fn unsupported_extension_is_a_load_failure() {
    let sys = InMemorySys::default();
    let err =
        load_config::<serde_json::Value, _, _>(Path::new("config.txt"), &sys)
            .unwrap_err();
    let r = wrapped_load(err);
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "config.load_failed");
    assert_eq!(d.details().unwrap()["kind"], "UnsupportedFileExtension");
}

#[test]
fn config_codes_are_registered_in_the_catalog() {
    for code in ["config.file_not_found", "config.load_failed"] {
        assert!(
            omni_codes::get(code).is_some(),
            "`{code}` is not registered"
        );
    }
}
