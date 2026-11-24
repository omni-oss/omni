//! Authored task aliases: decoding, canonical index and inheritance
//! (RFC 0024 F03.4 slice 1).

use merge::Merge;
use omni_configurations::{
    FinalizeError, FinalizedProject, LoadedLayer, ProjectLayer,
    TaskAliasErrorKind, TaskExtensionErrorKind,
};
use serde_json::{Value, json};

fn layer(doc: Value) -> LoadedLayer {
    LoadedLayer {
        layer: serde_json::from_value::<ProjectLayer>(doc).unwrap(),
        ..Default::default()
    }
}

fn compose(docs: Vec<Value>) -> Result<FinalizedProject, FinalizeError> {
    let mut it = docs.into_iter().map(layer);
    let mut acc = it.next().unwrap();
    for next in it {
        acc.merge(next);
    }
    acc.finalize_composed()
}

fn project(tasks: Value) -> Value {
    json!({ "name": "p", "tasks": tasks })
}

fn alias_error(docs: Vec<Value>) -> TaskAliasErrorKind {
    match compose(docs) {
        Err(FinalizeError::TaskAlias(e)) => e.kind(),
        Err(e) => panic!("expected an alias error, got {e}"),
        Ok(_) => panic!("expected an alias error"),
    }
}

fn parse_error(doc: Value) -> String {
    serde_json::from_value::<ProjectLayer>(doc)
        .unwrap_err()
        .to_string()
}

#[test]
fn an_alias_resolves_to_its_canonical_task_and_is_not_a_task() {
    let p = compose(vec![project(json!({
        "build-native": "make",
        "compile": { "alias": "build-native" }
    }))])
    .unwrap();
    assert_eq!(p.aliases.resolve("compile"), Some("build-native"));
    assert!(p.config.tasks.as_map().contains_key("build-native"));
    assert!(!p.config.tasks.as_map().contains_key("compile"));
}

#[test]
fn alias_chains_are_flattened() {
    let p = compose(vec![project(json!({
        "real": "x",
        "a": { "alias": "b" },
        "b": { "alias": "c" },
        "c": { "alias": "real" }
    }))])
    .unwrap();
    for name in ["a", "b", "c"] {
        assert_eq!(p.aliases.resolve(name), Some("real"), "{name}");
    }
}

#[test]
fn alias_cycles_and_missing_targets_are_rejected() {
    assert_eq!(
        alias_error(vec![project(json!({
            "a": { "alias": "b" }, "b": { "alias": "a" }
        }))]),
        TaskAliasErrorKind::Cycle
    );
    assert_eq!(
        alias_error(vec![project(json!({ "a": { "alias": "a" } }))]),
        TaskAliasErrorKind::Cycle
    );
    assert_eq!(
        alias_error(vec![project(json!({ "a": { "alias": "nope" } }))]),
        TaskAliasErrorKind::MissingTarget
    );
}

#[test]
fn a_base_template_cannot_be_a_runnable_alias_target() {
    assert_eq!(
        alias_error(vec![project(json!({
            "tpl": { "base": true, "exec": "x" },
            "a": { "alias": "tpl" }
        }))]),
        TaskAliasErrorKind::BaseTarget
    );
}

#[test]
fn a_commandless_runnable_task_is_a_valid_target() {
    let p = compose(vec![project(json!({
        "group": { "dependencies": [] },
        "g": { "alias": "group" }
    }))])
    .unwrap();
    assert_eq!(p.aliases.resolve("g"), Some("group"));
}

#[test]
fn a_base_project_skips_alias_target_validation() {
    let mut doc = project(json!({ "a": { "alias": "missing" } }));
    doc["base"] = json!(true);
    let p = compose(vec![doc]).unwrap();
    assert!(p.aliases.is_empty());
}

#[test]
fn a_base_layer_alias_is_validated_once_composed_into_a_concrete_project() {
    let mut base = project(json!({ "a": { "alias": "t" } }));
    base["base"] = json!(true);
    let concrete = json!({ "name": "c", "tasks": { "t": "x" } });
    let p = compose(vec![base.clone(), concrete]).unwrap();
    assert_eq!(p.aliases.resolve("a"), Some("t"));
    let empty = json!({ "name": "c" });
    assert_eq!(
        alias_error(vec![base, empty]),
        TaskAliasErrorKind::MissingTarget
    );
}

#[test]
fn a_discarded_alias_skips_target_validation() {
    let p = compose(vec![
        project(json!({ "a": { "alias": "missing" } })),
        project(json!({ "a": "now a task" })),
    ])
    .unwrap();
    assert!(p.aliases.is_empty());
    assert!(p.config.tasks.as_map().contains_key("a"));
    let p = compose(vec![
        project(json!({ "a": { "alias": "missing" } })),
        json!({ "name": "p", "tasks": { "replace": { "t": "x" } } }),
    ])
    .unwrap();
    assert!(p.aliases.is_empty());
}

#[test]
fn normal_and_alias_replacements_are_atomic() {
    let p = compose(vec![
        project(json!({ "t": { "exec": "x", "description": "d" }, "u": "u" })),
        project(json!({ "t": { "alias": "u" } })),
    ])
    .unwrap();
    assert_eq!(p.aliases.resolve("t"), Some("u"));
    assert!(!p.config.tasks.as_map().contains_key("t"));
    let p = compose(vec![
        project(json!({ "t": { "alias": "u" }, "u": "u" })),
        project(json!({ "t": { "description": "only" } })),
    ])
    .unwrap();
    assert!(p.aliases.is_empty());
    assert!(p.config.tasks.as_map().contains_key("t"));
}

#[test]
fn alias_to_alias_replacement_takes_the_higher_target() {
    let p = compose(vec![
        project(json!({ "x": "x", "y": "y", "a": { "alias": "x" } })),
        project(json!({ "a": { "alias": "y" } })),
    ])
    .unwrap();
    assert_eq!(p.aliases.resolve("a"), Some("y"));
}

#[test]
fn mixed_alias_and_task_objects_are_rejected_even_when_later_discarded() {
    for mixed in [
        json!({ "alias": "x", "exec": "y" }),
        json!({ "exec": "y", "alias": "x" }),
        json!({ "alias": "x", "base": true }),
    ] {
        let err = parse_error(project(json!({ "t": mixed.clone() })));
        assert!(err.contains("alias") || err.contains("unknown"), "{err}");
        let err = parse_error(json!({
            "name": "p",
            "tasks": { "replace": { "t": mixed } }
        }));
        assert!(!err.is_empty());
    }
    assert!(
        serde_json::from_value::<ProjectLayer>(project(
            json!({ "t": { "alias": 1 } })
        ))
        .is_err()
    );
}

#[test]
fn extends_resolves_through_an_alias_to_the_canonical_task() {
    let p = compose(vec![project(json!({
        "real": { "exec": "make", "description": "d" },
        "short": { "alias": "real" },
        "child": { "extends": "short", "description": "child" }
    }))])
    .unwrap();
    let child = p.config.tasks.as_map()["child"]
        .clone()
        .into_long_form("child");
    assert!(child.exec.is_some(), "the child inherits `real`'s command");
    assert!(p.config.tasks.as_map().contains_key("real"));
}

#[test]
fn extends_through_an_alias_to_itself_is_a_visible_cycle() {
    let err = match compose(vec![project(json!({
        "a": { "extends": "me", "exec": "x" },
        "me": { "alias": "a" }
    }))]) {
        Err(FinalizeError::TaskExtension(e)) => e.kind(),
        _ => panic!("expected an extension error"),
    };
    assert_eq!(err, TaskExtensionErrorKind::Cycle);
}

#[test]
fn escaped_names_in_keys_and_targets_decode_to_raw_names() {
    let p = compose(vec![project(json!({
        "build\\#all": "x",
        "lint:fix": "y",
        "star": { "alias": "build\\#all" },
        "fix": { "alias": "lint:fix" },
        "caret": { "alias": "build\\#all" },
        "\\^up": "z",
        "w\\\\x": "w"
    }))])
    .unwrap();
    let tasks = p.config.tasks.as_map();
    assert!(tasks.contains_key("build#all"));
    assert!(tasks.contains_key("lint:fix"));
    assert!(tasks.contains_key("^up"));
    assert!(tasks.contains_key("w\\x"));
    assert_eq!(p.aliases.resolve("star"), Some("build#all"));
    assert_eq!(p.aliases.resolve("fix"), Some("lint:fix"));
}

#[test]
fn unsupported_reserved_syntax_is_reported_with_the_original_spelling() {
    for (key, needle) in [
        ("a#b", "a\\#b"),
        ("native::build", "native\\:\\:build"),
        ("^up", "\\^up"),
        ("a\\q", "unsupported escape"),
    ] {
        let err = parse_error(project(json!({ key: "x" })));
        assert!(err.contains(&format!("`{key}`")), "{err}");
        assert!(err.contains(needle), "{err}");
    }
    let err = match compose(vec![project(json!({
        "t": "x",
        "a": { "alias": "native::t" }
    }))]) {
        Err(FinalizeError::TaskAlias(e)) => e.to_string(),
        _ => panic!("expected an alias error"),
    };
    assert!(err.contains("native::t") && err.contains("\\:\\:"), "{err}");
    assert_eq!(
        alias_error(vec![project(json!({
            "t": "x", "a": { "alias": "other#t" }
        }))]),
        TaskAliasErrorKind::InvalidTarget
    );
}

#[test]
fn extends_operands_are_decoded_at_lookup() {
    let p = compose(vec![project(json!({
        "a\\#b": { "exec": "x" },
        "c": { "extends": "a\\#b" }
    }))])
    .unwrap();
    assert!(p.config.tasks.as_map().contains_key("c"));
    let err = parse_error(project(json!({ "a#b": { "exec": "x" } })));
    assert!(err.contains("same-project") || err.contains("another project"));
}

#[test]
fn operator_named_entries_live_inside_an_explicit_envelope() {
    let p = compose(vec![project(json!({
        "merge": { "merge": "x", "replace": { "alias": "merge" } }
    }))])
    .unwrap();
    assert!(p.config.tasks.as_map().contains_key("merge"));
    assert_eq!(p.aliases.resolve("replace"), Some("merge"));
}

#[test]
fn a_mixed_operator_envelope_is_rejected() {
    assert!(
        serde_json::from_value::<ProjectLayer>(project(json!({
            "merge": { "a": "x" }, "b": "y"
        })))
        .is_err()
    );
}

#[test]
fn two_spellings_of_one_canonical_key_are_a_duplicate_in_every_envelope() {
    for tasks in [
        json!({ "lint:fix": "a", "lint\\:fix": "b" }),
        json!({ "merge": { "lint:fix": "a", "lint\\:fix": "b" } }),
        json!({ "replace": { "lint:fix": "a", "lint\\:fix": "b" } }),
    ] {
        let err = parse_error(project(tasks));
        assert!(err.contains("both name"), "{err}");
    }
}

#[test]
fn across_layers_the_ordered_rules_apply_to_the_canonical_key() {
    let p = compose(vec![
        project(json!({ "lint:fix": { "exec": "a", "description": "d" } })),
        project(json!({ "lint\\:fix": { "exec": "b" } })),
    ])
    .unwrap();
    let t = p.config.tasks.as_map()["lint:fix"]
        .clone()
        .into_long_form("lint:fix");
    assert!(t.description.is_some(), "merged field by field");
}
