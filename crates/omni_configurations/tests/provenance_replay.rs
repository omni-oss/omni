//! Values and their origins replay together (RFC 0024 F07.1): the effective
//! `cache.enabled` of each task and the declaration that supplied it.

use std::path::Path;

use merge::Merge;
use omni_configurations::{
    DeclarationSource, FinalizedProject, LayerId, LoadedLayer, ProjectLayer,
};
use omni_types::OmniPath;

fn file(path: &str, json: &str) -> LoadedLayer {
    LoadedLayer::from_file(
        OmniPath::new(path),
        OmniPath::new(Path::new(path).parent().unwrap()),
        serde_json::from_str::<ProjectLayer>(json).unwrap(),
    )
}

fn fileless(id: &str, json: &str) -> LoadedLayer {
    LoadedLayer::with_source(
        DeclarationSource::Fileless {
            id: LayerId::new(id),
        },
        OmniPath::default(),
        serde_json::from_str::<ProjectLayer>(json).unwrap(),
    )
}

fn compose(layers: Vec<LoadedLayer>) -> FinalizedProject {
    let mut it = layers.into_iter();
    let mut acc = it.next().unwrap();
    for next in it {
        acc.merge(next);
    }
    acc.finalize_composed().unwrap()
}

fn deciding_file(p: &FinalizedProject, task: &str) -> Option<String> {
    p.task_cache_enabled_file
        .get(task)
        .map(|f| f.path().unwrap().display().to_string())
}

#[test]
fn an_omitted_setting_leaves_the_lower_file_the_winner() {
    let p = compose(vec![
        file(
            "lib/base.yaml",
            r#"{"name":"a","cache":{"enabled":false},"tasks":{"t":"x"}}"#,
        ),
        file("app/p.yaml", r#"{"cache":{"remote":false}}"#),
    ]);
    assert_eq!(deciding_file(&p, "t").as_deref(), Some("lib/base.yaml"));
}

#[test]
fn an_equal_value_takes_the_later_file() {
    let p = compose(vec![
        file(
            "lib/base.yaml",
            r#"{"name":"a","cache":{"enabled":true},"tasks":{"t":"x"}}"#,
        ),
        file("app/p.yaml", r#"{"cache":{"enabled":true}}"#),
    ]);
    assert_eq!(deciding_file(&p, "t").as_deref(), Some("app/p.yaml"));
    assert!(p.task_cache_enabled["t"].value);
}

#[test]
fn a_fileless_replacement_does_not_report_the_lower_file() {
    let p = compose(vec![
        file(
            "lib/base.yaml",
            r#"{"name":"a","cache":{"enabled":true},"tasks":{"t":"x"}}"#,
        ),
        fileless("native", r#"{"cache":{"enabled":false}}"#),
    ]);
    assert!(!p.task_cache_enabled["t"].value);
    assert_eq!(deciding_file(&p, "t"), None);
    let origin = &p.task_cache_enabled_origin["t"];
    assert_eq!(
        origin.source,
        DeclarationSource::Fileless {
            id: LayerId::new("native")
        }
    );
}

#[test]
fn a_file_after_a_fileless_declaration_wins_back_the_file() {
    let p = compose(vec![
        fileless(
            "native",
            r#"{"name":"a","cache":{"enabled":false},"tasks":{"t":"x"}}"#,
        ),
        file("app/p.yaml", r#"{"cache":{"enabled":true}}"#),
    ]);
    assert_eq!(deciding_file(&p, "t").as_deref(), Some("app/p.yaml"));
}

#[test]
fn project_and_task_declarations_pick_their_own_files() {
    let p = compose(vec![
        file(
            "lib/base.yaml",
            r#"{"name":"a","cache":{"enabled":false},"tasks":{
                "inherits":"x",
                "own":{"exec":"x","cache":{"enabled":true}},
                "later":{"exec":"x"}}}"#,
        ),
        file(
            "app/p.yaml",
            r#"{"tasks":{"later":{"cache":{"enabled":true}},"inherits":{"cache":{"remote":false}}}}"#,
        ),
    ]);
    assert_eq!(
        deciding_file(&p, "inherits").as_deref(),
        Some("lib/base.yaml")
    );
    assert_eq!(deciding_file(&p, "own").as_deref(), Some("lib/base.yaml"));
    assert_eq!(deciding_file(&p, "later").as_deref(), Some("app/p.yaml"));
}

#[test]
fn a_shorthand_transition_keeps_the_declaring_files() {
    // short -> long -> short -> long: only the long forms declare `enabled`.
    let p = compose(vec![
        file("a.yaml", r#"{"name":"p","tasks":{"t":"one","u":"one"}}"#),
        file(
            "b/b.yaml",
            r#"{"tasks":{"t":{"cache":{"enabled":false}},"u":{"exec":"u"}}}"#,
        ),
        file("c/c.yaml", r#"{"tasks":{"t":"two"}}"#),
        file("d/d.yaml", r#"{"tasks":{"u":{"cache":{"enabled":true}}}}"#),
    ]);
    assert_eq!(deciding_file(&p, "t").as_deref(), Some("b/b.yaml"));
    assert_eq!(deciding_file(&p, "u").as_deref(), Some("d/d.yaml"));
    assert!(!p.task_cache_enabled["t"].value);
}

#[test]
fn task_extends_keeps_the_template_s_file() {
    let p = compose(vec![
        file(
            "lib/base.yaml",
            r#"{"name":"p","tasks":{"tpl":{"base":true,"exec":"x","cache":{"enabled":false}}}}"#,
        ),
        file("app/p.yaml", r#"{"tasks":{"real":{"extends":"tpl"}}}"#),
    ]);
    assert_eq!(deciding_file(&p, "real").as_deref(), Some("lib/base.yaml"));
    assert!(!p.task_cache_enabled_origin.contains_key("tpl"));
}

#[test]
fn a_later_declaration_overrides_an_extended_template_s_file() {
    let p = compose(vec![
        file(
            "lib/base.yaml",
            r#"{"name":"p","tasks":{"tpl":{"base":true,"exec":"x","cache":{"enabled":false}}}}"#,
        ),
        file(
            "app/p.yaml",
            r#"{"tasks":{"real":{"extends":"tpl","cache":{"enabled":true}}}}"#,
        ),
    ]);
    assert_eq!(deciding_file(&p, "real").as_deref(), Some("app/p.yaml"));
}

#[test]
fn names_with_a_hash_are_keyed_structurally() {
    let p = compose(vec![file(
        "a.yaml",
        r#"{"name":"a#b","tasks":{"c":{"exec":"x","cache":{"enabled":true}},"b#c":{"exec":"x","cache":{"enabled":false}}}}"#,
    )]);
    assert!(p.task_cache_enabled["c"].value);
    assert!(!p.task_cache_enabled["b#c"].value);
    assert_eq!(deciding_file(&p, "c").as_deref(), Some("a.yaml"));
    assert_eq!(deciding_file(&p, "b#c").as_deref(), Some("a.yaml"));
}

#[test]
fn a_host_fallback_has_no_origin() {
    let p = compose(vec![file("a.yaml", r#"{"name":"a","tasks":{"t":"x"}}"#)]);
    assert!(!p.task_cache_enabled_origin.contains_key("t"));
}

#[test]
fn an_origin_keeps_its_base_directory_apart_from_its_source() {
    let p = compose(vec![
        file(
            "lib/base.yaml",
            r#"{"name":"a","cache":{"enabled":true},"tasks":{"t":"x"}}"#,
        ),
        file("app/p.yaml", r#"{"description":"d"}"#),
    ]);
    let origin = &p.task_cache_enabled_origin["t"];
    assert_eq!(origin.base_dir, OmniPath::new("lib"));
}

#[test]
fn provenance_is_not_serialized_or_authored() {
    let layer = file(
        "a.yaml",
        r#"{"name":"a","cache":{"enabled":true},"tasks":{"t":{"exec":"x","cache":{"enabled":true}}}}"#,
    );
    let serialized = serde_json::to_value(&layer.layer).unwrap();
    assert_eq!(
        serialized,
        serde_json::json!({
            "name":"a","cache":{"enabled":true},
            "tasks":{"t":{"exec":"x","cache":{"enabled":true}}}
        })
    );
    let finalized = compose(vec![layer]);
    let config = serde_json::to_value(&finalized.config).unwrap();
    assert!(!config.to_string().contains("a.yaml"));
}
