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
    LoadedLayer::fileless(
        LayerId::new(id),
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
    let serialized = serde_json::to_value(layer.layer()).unwrap();
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

fn deciding(p: &FinalizedProject, task: &str) -> Option<String> {
    p.task_cache_enabled_origin
        .get(task)
        .map(|origin| match &origin.source {
            DeclarationSource::File { path } => {
                format!("file:{}", path.path().unwrap().display())
            }
            DeclarationSource::Fileless { id } => {
                format!("fileless:{}", id.as_str())
            }
        })
}

fn assert_task(
    p: &FinalizedProject,
    task: &str,
    value: bool,
    source: Option<&str>,
) {
    assert_eq!(p.task_cache_enabled[task].value, value, "{task} value");
    assert_eq!(deciding(p, task).as_deref(), source, "{task} origin");
}

#[test]
fn file_fileless_file_stack_at_project_level() {
    let base = r#"{"name":"a","cache":{"enabled":false},"tasks":{"t":"x"}}"#;
    let set = |layers: [&str; 3]| {
        compose(vec![
            file("lib/a.yaml", layers[0]),
            fileless("mid", layers[1]),
            file("app/c.yaml", layers[2]),
        ])
    };
    let p = set([
        base,
        r#"{"cache":{"enabled":true}}"#,
        r#"{"cache":{"enabled":false}}"#,
    ]);
    assert_task(&p, "t", false, Some("file:app/c.yaml"));
    let p = set([
        base,
        r#"{"cache":{"enabled":true}}"#,
        r#"{"description":"d"}"#,
    ]);
    assert_task(&p, "t", true, Some("fileless:mid"));
    let p = set([base, r#"{"description":"d"}"#, r#"{"description":"d"}"#]);
    assert_task(&p, "t", false, Some("file:lib/a.yaml"));
}

#[test]
fn fileless_file_fileless_stack_at_project_level() {
    let set = |layers: [&str; 3]| {
        compose(vec![
            file("lib/a.yaml", r#"{"name":"a","tasks":{"t":"x"}}"#),
            fileless("mid", layers[0]),
            file("app/c.yaml", layers[1]),
            fileless("top", layers[2]),
        ])
    };
    let on = r#"{"cache":{"enabled":true}}"#;
    let off = r#"{"cache":{"enabled":false}}"#;
    let none = r#"{"description":"d"}"#;
    assert_task(&set([on, off, on]), "t", true, Some("fileless:top"));
    assert_task(&set([on, off, none]), "t", false, Some("file:app/c.yaml"));
    assert_task(&set([on, none, none]), "t", true, Some("fileless:mid"));
    assert_task(&set([none, none, none]), "t", true, None);
}

#[test]
fn three_layer_stacks_at_task_level() {
    let t = |enabled: Option<bool>| match enabled {
        Some(v) => {
            format!(r#"{{"tasks":{{"t":{{"cache":{{"enabled":{v}}}}}}}}}"#)
        }
        None => r#"{"tasks":{"t":{"cache":{"remote":false}}}}"#.to_string(),
    };
    let run = |kinds: [bool; 3], vals: [Option<bool>; 3]| {
        let mut layers = vec![];
        for (i, (is_file, v)) in kinds.iter().zip(vals).enumerate() {
            let body = t(v);
            layers.push(if *is_file {
                file(&format!("d{i}/f{i}.yaml"), &body)
            } else {
                fileless(&format!("l{i}"), &body)
            });
        }
        layers.insert(
            0,
            file("root/r.yaml", r#"{"name":"a","tasks":{"t":"x"}}"#),
        );
        compose(layers)
    };
    let ffl = [true, false, true];
    let flf = [false, true, false];
    for kinds in [ffl, flf] {
        let label = |i: usize| {
            if kinds[i] {
                format!("file:d{i}/f{i}.yaml")
            } else {
                format!("fileless:l{i}")
            }
        };
        let p = run(kinds, [Some(false), Some(true), Some(false)]);
        assert_task(&p, "t", false, Some(&label(2)));
        let p = run(kinds, [Some(false), Some(true), None]);
        assert_task(&p, "t", true, Some(&label(1)));
        let p = run(kinds, [Some(false), None, None]);
        assert_task(&p, "t", false, Some(&label(0)));
        let p = run(kinds, [None, None, None]);
        assert_task(&p, "t", true, None);
    }
}

#[test]
fn multi_hop_task_extends_keeps_the_declaring_origin() {
    let p = compose(vec![
        file(
            "lib/base.yaml",
            r#"{"name":"p","tasks":{
                "a":{"base":true,"exec":"x","cache":{"enabled":false}},
                "b":{"base":true,"extends":"a"}}}"#,
        ),
        file("app/p.yaml", r#"{"tasks":{"c":{"extends":"b"}}}"#),
    ]);
    assert_task(&p, "c", false, Some("file:lib/base.yaml"));

    let p = compose(vec![
        file(
            "lib/base.yaml",
            r#"{"name":"p","tasks":{
                "a":{"base":true,"exec":"x","cache":{"enabled":false}}}}"#,
        ),
        file(
            "mid/m.yaml",
            r#"{"tasks":{"b":{"base":true,"extends":"a","cache":{"enabled":true}}}}"#,
        ),
        fileless("top", r#"{"tasks":{"c":{"extends":"b"}}}"#),
    ]);
    assert_task(&p, "c", true, Some("file:mid/m.yaml"));
}

#[test]
fn diamond_task_extends_picks_the_last_declaration_in_order() {
    let tasks = |left: &str, right: &str| {
        compose(vec![
            file(
                "lib/base.yaml",
                r#"{"name":"p","tasks":{
                    "root":{"base":true,"exec":"x","cache":{"enabled":false}}}}"#,
            ),
            file(
                "lib/left.yaml",
                &format!(
                    r#"{{"tasks":{{"l":{{"base":true,"extends":"root"{left}}}}}}}"#
                ),
            ),
            file(
                "lib/right.yaml",
                &format!(
                    r#"{{"tasks":{{"r":{{"base":true,"extends":"root"{right}}}}}}}"#
                ),
            ),
            file("app/p.yaml", r#"{"tasks":{"d":{"extends":["l","r"]}}}"#),
        ])
    };
    let on = r#","cache":{"enabled":true}"#;
    let off = r#","cache":{"enabled":false}"#;
    // Only one arm sets it: that arm's file decides.
    assert_task(&tasks(on, ""), "d", true, Some("file:lib/left.yaml"));
    assert_task(&tasks("", on), "d", true, Some("file:lib/right.yaml"));
    // Both arms set it: the later arm wins, even with equal values.
    assert_task(&tasks(on, off), "d", false, Some("file:lib/right.yaml"));
    assert_task(&tasks(off, on), "d", true, Some("file:lib/right.yaml"));
    assert_task(&tasks(on, on), "d", true, Some("file:lib/right.yaml"));
    // Neither arm sets it: the shared root's origin survives the diamond.
    assert_task(&tasks("", ""), "d", false, Some("file:lib/base.yaml"));
}

#[test]
fn tasks_replace_drops_an_earlier_task_s_origin_with_its_value() {
    let p = compose(vec![
        file(
            "lib/base.yaml",
            r#"{"name":"p","tasks":{
                "gone":{"exec":"x","cache":{"enabled":false}},
                "kept":{"exec":"x","cache":{"enabled":false}}}}"#,
        ),
        file(
            "app/p.yaml",
            r#"{"tasks":{"replace":{"kept":{"exec":"y"}}}}"#,
        ),
    ]);
    assert!(!p.task_cache_enabled.contains_key("gone"));
    assert!(!p.task_cache_enabled_origin.contains_key("gone"));
    assert_task(&p, "kept", true, None);
}
