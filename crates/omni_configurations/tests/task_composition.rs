//! Field-wise task composition and atomic alias transitions (RFC 0024 §17.9).

use config_utils::{DictConfig, Replace};
use merge::Merge;
use omni_command_config::CommandConfig;
use omni_configurations::{
    LoadedLayer, ProjectLayer, TaskAliasLayer, TaskConfiguration,
    TaskDependencyConfiguration, TaskExtensionErrorKind, TaskLayer,
};

fn layer(json: &str) -> LoadedLayer {
    LoadedLayer {
        layer: serde_json::from_str::<ProjectLayer>(json).unwrap(),
        ..Default::default()
    }
}

fn compose(docs: &[&str]) -> LoadedLayer {
    let mut it = docs.iter().map(|d| layer(d));
    let mut acc = it.next().unwrap();
    for next in it {
        acc.merge(next);
    }
    acc
}

fn project(tasks: &[&str]) -> String {
    format!(r#"{{"name":"p","tasks":{{{}}}}}"#, tasks.join(","))
}

/// Effective task `t` after composing the documents.
fn task(docs: &[&str]) -> TaskConfiguration {
    compose(docs)
        .finalize_composed()
        .unwrap()
        .config
        .tasks
        .as_map()["t"]
        .clone()
}

fn long(
    task: TaskConfiguration,
) -> omni_configurations::TaskConfigurationLongForm {
    task.into_long_form("t")
}

fn deps(task: TaskConfiguration) -> Vec<TaskDependencyConfiguration> {
    long(task).dependencies.into_vec()
}

fn upstream(name: &str) -> TaskDependencyConfiguration {
    TaskDependencyConfiguration::Upstream { task: name.into() }
}

fn own(name: &str) -> TaskDependencyConfiguration {
    TaskDependencyConfiguration::Own { task: name.into() }
}

fn shell(cmd: &str) -> Option<Replace<CommandConfig>> {
    Some(Replace::new(CommandConfig::Shell(cmd.into())))
}

fn doc(task: &str) -> String {
    project(&[&format!(r#""t":{task}"#)])
}

#[test]
fn short_over_short_replaces_the_command_and_stays_short() {
    let t = task(&[&doc(r#""a""#), &doc(r#""b""#)]);
    assert_eq!(
        t,
        TaskConfiguration::ShortForm(CommandConfig::Shell("b".into()))
    );
    assert_eq!(deps(t), vec![upstream("t")]);
}

#[test]
fn long_override_of_short_keeps_command_and_implicit_dependency() {
    let t = task(&[
        &doc(r#""cargo build""#),
        &doc(r#"{"env":{"vars":{"A":"1"}},"cache":{"enabled":false}}"#),
    ]);
    let l = long(t);
    assert_eq!(l.exec, shell("cargo build"));
    assert_eq!(l.dependencies.into_vec(), vec![upstream("t")]);
    assert_eq!(l.cache.enabled, Some(Replace::new(false)));
    assert_eq!(
        l.env.vars.unwrap().to_map_to_inner().get("A"),
        Some(&"1".to_string())
    );
}

#[test]
fn long_override_command_replaces_but_keeps_the_dependency() {
    let l = long(task(&[&doc(r#""old""#), &doc(r#"{"exec":"new"}"#)]));
    assert_eq!(l.exec, shell("new"));
    assert_eq!(l.dependencies.into_vec(), vec![upstream("t")]);
}

#[test]
fn plain_dependency_list_replaces_the_implicit_dependency() {
    let t = task(&[&doc(r#""x""#), &doc(r#"{"dependencies":["other"]}"#)]);
    assert_eq!(deps(t), vec![own("other")]);
}

#[test]
fn appended_dependencies_follow_the_implicit_dependency() {
    let t = task(&[
        &doc(r#""x""#),
        &doc(r#"{"dependencies":{"append":["other"]}}"#),
    ]);
    assert_eq!(deps(t), vec![upstream("t"), own("other")]);
}

#[test]
fn prepended_dependencies_precede_the_implicit_dependency() {
    let t = task(&[
        &doc(r#""x""#),
        &doc(r#"{"dependencies":{"prepend":["other"]}}"#),
    ]);
    assert_eq!(deps(t), vec![own("other"), upstream("t")]);
}

#[test]
fn short_override_of_long_keeps_fields_and_adds_the_dependency() {
    let t = task(&[
        &doc(r#"{"exec":"old","env":{"vars":{"A":"1"}},
            "cache":{"enabled":false},"dependencies":["d"]}"#),
        &doc(r#""new""#),
    ]);
    let l = long(t);
    assert_eq!(l.exec, shell("new"));
    assert_eq!(l.cache.enabled, Some(Replace::new(false)));
    assert!(l.env.vars.is_some());
    // The implicit dependency is appended to the base's list.
    assert_eq!(l.dependencies.into_vec(), vec![own("d"), upstream("t")]);
}

#[test]
fn short_override_of_long_with_appendable_dependencies_adds_it() {
    let t = task(&[
        &doc(r#"{"exec":"old","dependencies":{"append":["d"]}}"#),
        &doc(r#""new""#),
    ]);
    assert_eq!(deps(t), vec![own("d"), upstream("t")]);
}

#[test]
fn long_over_long_is_unchanged() {
    let l = long(task(&[
        &doc(r#"{"exec":"a","enabled":false,"env":{"vars":{"A":"1"}}}"#),
        &doc(r#"{"exec":"b","env":{"vars":{"B":"2"}}}"#),
    ]));
    assert_eq!(l.exec, shell("b"));
    assert_eq!(l.enabled, Some(false.into()));
    let vars = l.env.vars.unwrap().to_map_to_inner();
    assert_eq!(vars.len(), 2);
}

#[test]
fn dictionary_replace_discards_the_base_entries() {
    let l = long(task(&[
        &doc(r#"{"exec":"a","env":{"vars":{"A":"1"}}}"#),
        &doc(r#"{"env":{"vars":{"replace":{"B":"2"}}}}"#),
    ]));
    let vars = l.env.vars.unwrap().to_map_to_inner();
    assert_eq!(vars.len(), 1);
    assert_eq!(vars.get("B"), Some(&"2".to_string()));
}

#[test]
fn three_layers_apply_in_order() {
    let t = task(&[
        &doc(r#""a""#),
        &doc(r#"{"dependencies":{"append":["x"]}}"#),
        &doc(r#"{"dependencies":{"prepend":["y"]}}"#),
    ]);
    assert_eq!(deps(t), vec![own("y"), upstream("t"), own("x")]);
}

#[test]
fn commandless_tasks_stay_valid() {
    let t =
        task(&[&doc(r#"{"description":"d"}"#), &doc(r#"{"enabled":true}"#)]);
    assert_eq!(long(t.clone()).exec, None);
    assert!(t.get_task("t").exec.is_none());
}

#[test]
fn child_extends_over_a_base_short_task() {
    let docs = [
        project(&[
            r#""tpl":{"base":true,"exec":"tpl","env":{"vars":{"X":"1"}}}"#,
            r#""t":"plain""#,
        ]),
        project(&[r#""t":{"extends":"tpl"}"#]),
    ];
    let l = long(task(&[&docs[0], &docs[1]]));
    // The shorthand's own command wins over the extended template's; the
    // template contributes the fields the shorthand leaves absent.
    assert_eq!(l.exec, shell("plain"));
    assert_eq!(l.dependencies.into_vec(), vec![upstream("t")]);
    assert!(l.env.vars.is_some());
}

#[test]
fn extends_of_unknown_task_is_reported() {
    let err = compose(&[&doc(r#"{"extends":"missing"}"#)])
        .finalize_composed()
        .unwrap_err();
    assert!(err.to_string().contains("missing"), "{err}");
}

#[test]
fn extension_cycles_are_reported() {
    let err = compose(&[&project(&[
        r#""a":{"extends":"b"}"#,
        r#""b":{"extends":"a"}"#,
    ])])
    .finalize_composed()
    .unwrap_err();
    assert!(err.to_string().to_lowercase().contains("cycl"), "{err}");
    let _ = TaskExtensionErrorKind::Cycle;
}

#[test]
fn base_marker_is_per_declaration() {
    let docs = [doc(r#"{"base":true,"exec":"a"}"#), doc(r#"{"exec":"b"}"#)];
    let finalized = compose(&[&docs[0], &docs[1]]).finalize_composed().unwrap();
    assert!(finalized.config.tasks.as_map().contains_key("t"));
}

#[test]
fn mixed_alias_and_task_declarations_are_rejected_at_parse() {
    for bad in [r#"{"alias":"x","exec":"y"}"#, r#"{"alias":"x"}"#] {
        assert!(
            serde_json::from_str::<ProjectLayer>(&project(&[&format!(
                r#""t":{bad}"#
            )]))
            .is_err(),
            "{bad}"
        );
    }
}

// Alias transitions. Aliases cannot be written yet, so they are built in code.

fn alias(target: &str) -> TaskLayer {
    TaskLayer::Alias(TaskAliasLayer {
        alias: target.into(),
    })
}

fn with_task(t: TaskLayer) -> LoadedLayer {
    let mut l = LoadedLayer::default();
    l.layer.tasks = Some(DictConfig::value(
        [("t".to_string(), t)].into_iter().collect(),
    ));
    l
}

fn effective(base: TaskLayer, over: TaskLayer) -> TaskLayer {
    let mut l = with_task(base);
    l.merge(with_task(over));
    l.layer.tasks.unwrap().as_map()["t"].clone()
}

fn normal(json: &str) -> TaskLayer {
    serde_json::from_str(json).unwrap()
}

#[test]
fn normal_to_alias_keeps_nothing_of_the_normal_task() {
    let rich = normal(
        r#"{"exec":"x","env":{"vars":{"A":"1"}},"cache":{"enabled":false},
        "dependencies":["d"]}"#,
    );
    assert_eq!(effective(rich, alias("n::build")), alias("n::build"));
    assert_eq!(
        effective(normal(r#""x""#), alias("n::build")),
        alias("n::build")
    );
}

#[test]
fn alias_to_normal_starts_a_distinct_definition() {
    let over = normal(r#"{"description":"only"}"#);
    assert_eq!(effective(alias("n::build"), over.clone()), over);
    let short = normal(r#""cmd""#);
    assert_eq!(effective(alias("n::build"), short.clone()), short);
}

#[test]
fn alias_over_alias_takes_the_higher_target() {
    assert_eq!(effective(alias("a"), alias("b")), alias("b"));
}

#[test]
fn a_normal_task_after_an_alias_after_a_normal_task_forgets_the_first() {
    let mut l =
        with_task(normal(r#"{"exec":"first","env":{"vars":{"A":"1"}}}"#));
    l.merge(with_task(alias("x")));
    l.merge(with_task(normal(r#"{"description":"d"}"#)));
    let TaskLayer::LongForm(t) = l.layer.tasks.unwrap().as_map()["t"].clone()
    else {
        panic!("long form")
    };
    assert!(t.exec.is_none() && t.env.is_none());
}

#[test]
fn aliases_survive_task_extension_resolution() {
    let mut l = with_task(alias("x"));
    if let Some(tasks) = l.layer.tasks.as_mut() {
        tasks
            .as_map_mut()
            .insert("tpl".into(), normal(r#"{"base":true,"exec":"tpl"}"#));
        tasks
            .as_map_mut()
            .insert("u".into(), normal(r#"{"extends":"tpl"}"#));
    }
    l.layer.name = Some("p".into());
    let finalized = l.finalize_composed().unwrap();
    let tasks = finalized.config.tasks.as_map();
    assert!(tasks.contains_key("u") && !tasks.contains_key("tpl"));
    // An alias has no eager task form before F03.
    assert!(!tasks.contains_key("t"));
}
