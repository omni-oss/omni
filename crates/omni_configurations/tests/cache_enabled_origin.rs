//! The origin tier of each task's effective `cache.enabled` (RFC 0024 F05.1, the
//! parts of C01 that need no plugins), and its parity with the value the data
//! extractor already computed.

use merge::Merge;
use omni_configurations::{
    CacheEnabled, CacheEnabledSource, FinalizedProject, LoadedLayer,
    ProjectLayer,
};

fn compose(docs: &[&str]) -> FinalizedProject {
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

fn decided(p: &FinalizedProject, task: &str) -> CacheEnabled {
    p.task_cache_enabled[task]
}

/// What the data extractor computed before this change.
fn legacy_value(p: &FinalizedProject, task: &str) -> bool {
    p.task_cache[task]
        .enabled
        .map(|e| *e.as_ref())
        .unwrap_or(true)
}

use CacheEnabledSource::*;

fn expect(
    p: &FinalizedProject,
    task: &str,
    value: bool,
    source: CacheEnabledSource,
) {
    assert_eq!(
        decided(p, task),
        CacheEnabled { value, source },
        "task {task}"
    );
    assert_eq!(
        decided(p, task).value,
        legacy_value(p, task),
        "parity for {task}"
    );
}

#[test]
fn a_task_setting_wins_over_the_project() {
    let p = compose(&[r#"{"name":"a","cache":{"enabled":true},"tasks":{
            "off":{"exec":"x","cache":{"enabled":false}},
            "on":{"exec":"x","cache":{"enabled":true}}}}"#]);
    expect(&p, "off", false, TaskExplicit);
    expect(&p, "on", true, TaskExplicit);

    let project_off =
        compose(&[r#"{"name":"a","cache":{"enabled":false},"tasks":{
            "on":{"exec":"x","cache":{"enabled":true}}}}"#]);
    expect(&project_off, "on", true, TaskExplicit);
}

#[test]
fn a_task_without_its_own_setting_inherits_the_project_one() {
    let p = compose(&[r#"{"name":"a","cache":{"enabled":false},"tasks":{
            "short":"echo",
            "long":{"exec":"x"},
            "other_cache_field":{"exec":"x","cache":{"output":{"logs":false}}}}}"#]);
    for task in ["short", "long", "other_cache_field"] {
        expect(&p, task, false, ProjectExplicit);
    }
}

#[test]
fn nothing_set_is_the_host_fallback() {
    let p =
        compose(
            &[r#"{"name":"a","tasks":{"short":"echo","long":{"exec":"x"},
            "keys":{"exec":"x","cache":{"key":{"env":["E"]}}}}}"#],
        );
    for task in ["short", "long", "keys"] {
        expect(&p, task, true, HostFallback);
    }
}

#[test]
fn an_explicit_value_equal_to_the_default_keeps_its_explicit_origin() {
    let p = compose(&[r#"{"name":"a","cache":{"enabled":true},"tasks":{
            "p":"echo","t":{"exec":"x","cache":{"enabled":true}}}}"#]);
    expect(&p, "p", true, ProjectExplicit);
    expect(&p, "t", true, TaskExplicit);
}

#[test]
fn origin_follows_the_composed_layers_not_one_file() {
    // The child does not mention `enabled`; the base's project setting survives.
    let base =
        r#"{"name":"base","cache":{"enabled":false},"tasks":{"t":"echo"}}"#;
    let child = r#"{"name":"app","cache":{"key":{"env":["E"]}}}"#;
    expect(&compose(&[base, child]), "t", false, ProjectExplicit);

    // The child states it, and overrides the base.
    let child_on = r#"{"name":"app","cache":{"enabled":true}}"#;
    expect(&compose(&[base, child_on]), "t", true, ProjectExplicit);

    // A task-level setting in a later layer overrides the project's.
    let child_task = r#"{"name":"app","tasks":{"t":{"exec":"echo","cache":{"enabled":true}}}}"#;
    expect(&compose(&[base, child_task]), "t", true, TaskExplicit);
}

#[test]
fn a_setting_reached_through_extends_is_the_task_s_own() {
    let p = compose(&[r#"{"name":"a","tasks":{
            "tpl":{"base":true,"exec":"x","cache":{"enabled":false}},
            "real":{"extends":"tpl"},
            "plain":"echo"}}"#]);
    expect(&p, "real", false, TaskExplicit);
    expect(&p, "plain", true, HostFallback);
}

#[test]
fn a_base_project_runs_no_tasks_so_records_none() {
    let p = compose(&[r#"{"name":"b","base":true,"tasks":{"t":"echo"}}"#]);
    assert!(p.task_cache_enabled.is_empty());
}

#[test]
fn every_task_has_an_origin_and_it_agrees_with_the_effective_cache() {
    let p = compose(&[r#"{"name":"a","cache":{"enabled":false},"tasks":{
            "a":"x","b":{"exec":"x","cache":{"enabled":true}},
            "c":{"exec":"x","cache":{"key":{"defaults":false}}}}}"#]);
    assert_eq!(p.task_cache.len(), p.task_cache_enabled.len());
    for task in p.task_cache.keys() {
        assert_eq!(
            decided(&p, task).value,
            legacy_value(&p, task),
            "the origin changed the value of {task}"
        );
    }
}

#[test]
fn source_file_is_not_user_configuration() {
    let input = r#"{"cache":{"enabled":true,"enabled_file":"spoofed.yaml"}}"#;
    assert!(serde_json::from_str::<ProjectLayer>(input).is_err());
}
