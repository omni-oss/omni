//! List and dictionary operators keep their order across task inheritance: a
//! `replace` in one declaration is not undone by an `append` in a later one
//! when the task inherits a template or the project settings.

use merge::Merge as _;
use omni_configurations::{
    FinalizedProject, LoadedLayer, ProjectLayer, TaskLayer,
};
use serde_json::{Value, json};

fn compose(docs: &[Value]) -> FinalizedProject {
    let mut layers = docs.iter().map(|doc| {
        LoadedLayer::unattributed(
            serde_json::from_value::<ProjectLayer>(doc.clone()).unwrap(),
        )
    });
    let mut merged = layers.next().unwrap();
    for layer in layers {
        merged.merge(layer);
    }
    merged.finalize_composed().unwrap()
}

fn run(
    project: &FinalizedProject,
) -> omni_configurations::TaskConfigurationLongForm {
    project.config.tasks.as_map()["run"]
        .clone()
        .into_long_form("run")
}

fn names(
    list: &config_utils::ListConfig<
        omni_configurations::TaskDependencyConfiguration,
    >,
) -> Vec<String> {
    list.iter().map(|dep| dep.to_string()).collect()
}

/// A template with every inherited field set, a task that extends it and
/// clears those fields, and a later declaration that appends to them.
fn template_then_reset_then_append() -> FinalizedProject {
    let cleared = json!({
        "extends": "tpl",
        "dependencies": {"replace": []},
        "with": {"replace": []},
        "args": {"items": {"replace": []}},
        "meta": {"items": {"replace": []}},
        "env": {"vars": {"replace": {}}},
        "cache": {
            "key": {"env": {"replace": []}, "files": {"replace": []}},
            "output": {"files": {"replace": []}}
        }
    });
    let appended = json!({
        "dependencies": {"append": ["new"]},
        "with": {"append": ["new"]},
        "args": {"items": {"append": ["new"]}},
        "meta": {"items": {"append": ["new"]}},
        "env": {"vars": {"C": "3"}},
        "cache": {
            "key": {"env": {"append": ["NEW"]}, "files": {"append": ["new-in"]}},
            "output": {"files": {"append": ["new-out"]}}
        }
    });
    compose(&[
        json!({"name": "app", "tasks": {
            "tpl": {
                "base": true,
                "dependencies": ["old"],
                "with": ["old"],
                "args": {"items": ["old"]},
                "meta": {"items": ["old"]},
                "env": {"vars": {"A": "1"}},
                "cache": {
                    "key": {"env": ["OLD"], "files": ["old-in"]},
                    "output": {"files": ["old-out"]}
                }
            },
            "run": cleared
        }}),
        json!({"tasks": {"run": appended}}),
    ])
}

#[test]
fn a_replace_survives_a_later_append_across_task_inheritance() {
    let task = run(&template_then_reset_then_append());

    assert_eq!(names(&task.dependencies), ["new"]);
    assert_eq!(names(&task.with), ["new"]);
    let args = task.args.to_map();
    assert_eq!(args["items"].clone().into_json(), json!(["new"]));
    let vars = task.env.vars.unwrap().to_map_to_inner();
    assert_eq!(vars.len(), 1);
    assert_eq!(vars["C"], "3");
}

#[test]
fn the_effective_meta_and_cache_keep_the_replace_too() {
    let project = template_then_reset_then_append();
    let task = run(&project);

    assert_eq!(
        task.meta.0.as_map()["items"].clone().into_json(),
        json!(["new"])
    );
    let meta = &project.task_meta["run"];
    assert_eq!(meta.0.as_map()["items"].clone().into_json(), json!(["new"]));

    let cache = &project.task_cache["run"];
    assert_eq!(cache.key.env.to_vec_to_inner(), ["NEW"]);
    let files =
        |globs: omni_glob_config::MergeGlobConfig<omni_types::OmniPath>| {
            globs
                .normalize()
                .include
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        };
    assert_eq!(files(cache.key.files.clone()), ["new-in"]);
    assert_eq!(files(cache.output.files.clone()), ["new-out"]);
}

#[test]
fn project_cache_is_replayed_without_any_task_extends() {
    let project = compose(&[
        json!({"name": "app", "cache": {
            "key": {"env": ["PROJECT"], "files": ["project-in"]},
            "output": {"files": ["project-out"]}
        }, "tasks": {"run": {"cache": {
            "key": {"env": {"replace": []}, "files": {"replace": []}},
            "output": {"files": {"replace": []}}
        }}}}),
        json!({"tasks": {"run": {"cache": {
            "key": {"env": {"append": ["NEW"]}, "files": {"append": ["new-in"]}},
            "output": {"files": {"append": ["new-out"]}}
        }}}}),
    ]);

    let cache = &project.task_cache["run"];
    assert_eq!(cache.key.env.to_vec_to_inner(), ["NEW"]);
    let include =
        |globs: omni_glob_config::MergeGlobConfig<omni_types::OmniPath>| {
            globs
                .normalize()
                .include
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        };
    assert_eq!(include(cache.key.files.clone()), ["new-in"]);
    assert_eq!(include(cache.output.files.clone()), ["new-out"]);
}

#[test]
fn project_meta_is_applied_to_a_task_exactly_once() {
    let project = compose(&[json!({
        "name": "app",
        "meta": {"tags": ["a"]},
        "tasks": {
            "plain": "echo",
            "appended": {"meta": {"tags": {"append": ["b"]}}},
            "cleared": {"meta": {"tags": {"replace": []}}}
        }
    })]);

    let tags = |task: &str| {
        project.task_meta[task].0.as_map()["tags"]
            .clone()
            .into_json()
    };
    assert_eq!(tags("plain"), json!(["a"]));
    assert_eq!(tags("appended"), json!(["a", "b"]));
    assert_eq!(tags("cleared"), json!([]));
}

#[test]
fn the_project_files_alone_still_replay_in_order() {
    let project = compose(&[
        json!({"name": "app", "dependencies": ["old"]}),
        json!({"dependencies": {"replace": []}}),
        json!({"dependencies": {"append": ["new"]}}),
    ]);
    assert_eq!(project.config.dependencies.to_vec_inner(), ["new"]);
}

#[test]
fn operations_apply_in_declaration_order_to_the_template_list() {
    let project = compose(&[
        json!({"name": "app", "tasks": {
            "tpl": {"base": true, "dependencies": ["x", "y"]},
            "run": {"extends": "tpl", "dependencies": {"append": ["a"]}}
        }}),
        json!({"tasks": {"run": {"dependencies": {"merge": ["b"]}}}}),
    ]);
    // [x, y] then append a, then merge b over the first entry.
    assert_eq!(names(&run(&project).dependencies), ["b", "y", "a"]);
}

#[test]
fn task_diamond_applies_shared_declarations_once_in_order() {
    let project = compose(&[
        json!({"name": "app", "tasks": {
            "common": {"base": true, "dependencies": {"append": ["common"]}},
            "left": {"base": true, "extends": "common", "dependencies": {"append": ["left"]}},
            "right": {"base": true, "extends": "common", "dependencies": {"append": ["right"]}},
            "run": {"extends": ["left", "right"], "dependencies": {"append": ["run"]}}
        }}),
        json!({"tasks": {
            "left": {"base": true, "dependencies": {"append": ["left-extra"]}},
            "run": {"dependencies": {"append": ["run-extra"]}}
        }}),
    ]);
    assert_eq!(
        names(&run(&project).dependencies),
        ["common", "left", "left-extra", "right", "run", "run-extra"]
    );
}

#[test]
fn project_meta_replacement_works_without_task_extends() {
    let project = compose(&[
        json!({"name": "app", "meta": {"tags": ["old"]}, "tasks": {
            "run": {"meta": {"tags": {"replace": []}}}
        }}),
        json!({"tasks": {"run": {"meta": {"tags": {"append": ["new"]}}}}}),
    ]);
    assert_eq!(
        project.task_meta["run"].0.as_map()["tags"]
            .clone()
            .into_json(),
        json!(["new"])
    );
}

#[test]
fn a_shorthand_adds_its_upstream_dependency_once() {
    let project = compose(&[
        json!({"name": "app", "tasks": {"run": "first"}}),
        json!({"tasks": {"run": "second"}}),
        json!({"tasks": {"run": {"dependencies": {"merge": ["d"]}}}}),
    ]);
    assert_eq!(names(&run(&project).dependencies), ["d"]);
}

#[test]
fn extending_a_shorthand_keeps_the_upstream_dependency_of_its_own_name() {
    let project = compose(&[json!({"name": "app", "tasks": {
        "build": "cargo build",
        "run": {"extends": "build", "exec": "cargo run"}
    }})]);
    assert_eq!(names(&run(&project).dependencies), ["^build"]);
}

#[test]
fn replacing_the_whole_task_forgets_earlier_declarations() {
    let project = compose(&[
        json!({"name": "app", "tasks": {
            "tpl": {"base": true, "dependencies": ["old"]},
            "run": {"extends": "tpl", "dependencies": {"append": ["early"]}}
        }}),
        json!({"tasks": {"replace": {"run": {"dependencies": ["fresh"]}}}}),
    ]);
    assert_eq!(names(&run(&project).dependencies), ["fresh"]);
}

#[test]
fn an_alias_in_between_forgets_earlier_declarations() {
    let long =
        |json: Value| -> TaskLayer { serde_json::from_value(json).unwrap() };
    let mut task = long(json!({"dependencies": ["old"]}));
    task.merge_named(long(json!({"dependencies": {"replace": []}})), "t");
    task.merge_named(
        TaskLayer::Alias(omni_configurations::TaskAliasLayer {
            alias: "other".into(),
        }),
        "t",
    );
    task.merge_named(long(json!({"dependencies": {"append": ["new"]}})), "t");

    let task = task.finalize().unwrap().into_long_form("t");
    let deps = task.dependencies;
    assert_eq!(names(&deps), ["new"]);
}
