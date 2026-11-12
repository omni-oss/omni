//! Old (eager, default-filled) versus new (presence-aware layers) effective
//! configuration for the documented RFC 0024 R04 corrections.

use merge::Merge;
use omni_configurations::{
    CacheConfiguration, LoadedLayer, ProjectConfiguration, ProjectLayer,
};

/// The previous pipeline: deserialize each file eagerly and merge the
/// default-filled values.
fn old(docs: &[&str]) -> ProjectConfiguration {
    let mut it = docs
        .iter()
        .map(|d| serde_json::from_str::<ProjectConfiguration>(d).unwrap());
    let mut acc = it.next().unwrap();
    for next in it {
        acc.merge(next);
    }
    acc
}

/// The previous project-to-task inheritance in the data extractor.
fn old_task_cache(p: &ProjectConfiguration, task: &str) -> CacheConfiguration {
    let mut cache = p.cache.clone();
    if let Some(task_cache) = p.tasks.as_map()[task].cache() {
        cache.merge(task_cache.clone());
    }
    cache
}

fn new(docs: &[&str]) -> omni_configurations::FinalizedProject {
    let mut it = docs.iter().map(|d| LoadedLayer {
        layer: serde_json::from_str::<ProjectLayer>(d).unwrap(),
        ..Default::default()
    });
    let mut acc = it.next().unwrap();
    for next in it {
        acc.merge(next);
    }
    acc.finalize_composed().unwrap()
}

const BASE_LOGS_FALSE: &str =
    r#"{"name":"base","cache":{"output":{"logs":false}},"tasks":{"t":"echo"}}"#;

#[test]
fn child_omitting_logs_no_longer_resets_the_base() {
    let child = r#"{"name":"app","cache":{"key":{"env":["E"]}}}"#;
    assert!(old(&[BASE_LOGS_FALSE, child]).cache.output.logs);
    assert!(!new(&[BASE_LOGS_FALSE, child]).config.cache.output.logs);
}

#[test]
fn child_stating_logs_true_still_wins() {
    let child = r#"{"name":"app","cache":{"output":{"logs":true}}}"#;
    assert!(old(&[BASE_LOGS_FALSE, child]).cache.output.logs);
    assert!(new(&[BASE_LOGS_FALSE, child]).config.cache.output.logs);
}

#[test]
fn task_cache_omitting_logs_no_longer_resets_the_project() {
    let doc = r#"{"name":"a","cache":{"output":{"logs":false}},
        "tasks":{"t":{"cache":{"key":{"env":["T"]}}}}}"#;
    let o = old(&[doc]);
    assert!(old_task_cache(&o, "t").output.logs);
    assert!(!new(&[doc]).task_cache["t"].output.logs);
}

#[test]
fn explicit_task_logs_true_still_wins() {
    let doc = r#"{"name":"a","cache":{"output":{"logs":false}},
        "tasks":{"t":{"cache":{"output":{"logs":true}}}}}"#;
    assert!(old_task_cache(&old(&[doc]), "t").output.logs);
    assert!(new(&[doc]).task_cache["t"].output.logs);
}

#[test]
fn a_child_may_omit_the_name_when_a_base_supplies_it() {
    let child = r#"{"description":"d"}"#;
    assert!(serde_json::from_str::<ProjectConfiguration>(child).is_err());
    assert_eq!(new(&[r#"{"name":"base"}"#, child]).config.name, "base");
}

#[test]
fn unchanged_cases_stay_identical() {
    let docs = [
        r#"{"name":"a","dependencies":["x"],"cache":{"enabled":true,
        "key":{"files":{"include":["i"],"exclude":["x"]}}}}"#,
        r#"{"name":"b","dependencies":{"append":["y"]},
        "cache":{"key":{"files":{"include":{"append":["j"]}}}}}"#,
    ];
    let (o, n) = (old(&docs), new(&docs).config);
    assert_eq!(o.name, n.name);
    assert_eq!(o.dependencies.into_vec(), n.dependencies.into_vec());
    assert_eq!(o.cache.key.files.normalize(), n.cache.key.files.normalize());
    assert_eq!(o.cache.enabled, n.cache.enabled);
}
