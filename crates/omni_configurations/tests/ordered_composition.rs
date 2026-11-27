//! Ordered contributor composition (RFC 0024 F07.3): the later contributor wins,
//! explicit layers beat every contributor, and arrival order never decides.
//! Covers C01 (tiers, through the entry point), C04, C12 and C17.

use std::path::Path;
use std::sync::Arc;

use merge::Merge;
use omni_configurations::{
    CacheEnabledSource, ComposeError, Composed, Contribution, ContributorId,
    DeclarationSource, EnabledDecl, EnabledScope, FinalizedProject,
    LoadedLayer, Origin, ProjectLayer, compose_ordered,
};
use omni_types::OmniPath;
use serde_json::Value;

fn id(name: &str) -> ContributorId {
    ContributorId::new(name)
}

fn contribution(name: &str, json: &str) -> Contribution {
    Contribution {
        id: id(name),
        layer: serde_json::from_str::<ProjectLayer>(json).unwrap(),
        base_dir: OmniPath::new(name),
        enabled: Vec::new(),
    }
}

fn emitted(name: &str, scope: EnabledScope, value: bool) -> EnabledDecl {
    EnabledDecl {
        scope,
        value,
        origin: Arc::new(Origin::new(
            DeclarationSource::contributor(id(name)),
            OmniPath::new(name),
        )),
    }
}

fn applied(file: &str, scope: EnabledScope, value: bool) -> EnabledDecl {
    EnabledDecl {
        scope,
        value,
        origin: Arc::new(Origin::new(
            DeclarationSource::file(OmniPath::new(file)),
            OmniPath::new(Path::new(file).parent().unwrap()),
        )),
    }
}

fn explicit(path: &str, json: &str) -> LoadedLayer {
    LoadedLayer::from_file(
        OmniPath::new(path),
        OmniPath::new(Path::new(path).parent().unwrap()),
        serde_json::from_str::<ProjectLayer>(json).unwrap(),
    )
}

fn order(names: &[&str]) -> Vec<ContributorId> {
    names.iter().map(|n| id(n)).collect()
}

fn try_compose(
    names: &[&str],
    contributions: Vec<Contribution>,
    explicit: Vec<LoadedLayer>,
) -> Result<Composed, ComposeError> {
    compose_ordered(
        OmniPath::new("root"),
        &order(names),
        contributions,
        explicit,
    )
}

fn compose(
    names: &[&str],
    contributions: Vec<Contribution>,
    explicit: Vec<LoadedLayer>,
) -> FinalizedProject {
    try_compose(names, contributions, explicit)
        .unwrap()
        .layer
        .finalize_composed()
        .unwrap()
}

fn named() -> LoadedLayer {
    explicit("app/project.yaml", r#"{"name":"web"}"#)
}

fn description(p: &FinalizedProject) -> String {
    p.config
        .description
        .as_ref()
        .map(|d| d.as_ref().clone())
        .unwrap()
}

fn env_var(p: &FinalizedProject, key: &str) -> Value {
    serde_json::to_value(&p.config.env.vars).unwrap()[key].clone()
}

fn items(
    list: &config_utils::ListConfig<config_utils::Replace<String>>,
) -> Vec<String> {
    list.as_vec().iter().map(|i| i.as_ref().clone()).collect()
}

fn project_contributions() -> Vec<Contribution> {
    vec![
        contribution(
            "frontend",
            r#"{"description":"web frontend","env":{"vars":{"MODE":"frontend","A":"1"}}}"#,
        ),
        contribution(
            "native",
            r#"{"description":"web native","env":{"vars":{"MODE":"native"}}}"#,
        ),
    ]
}

fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut out = Vec::new();
    for i in 0..items.len() {
        let mut rest = items.to_vec();
        let head = rest.remove(i);
        for mut tail in permutations(&rest) {
            tail.insert(0, head.clone());
            out.push(tail);
        }
    }
    out
}

#[test]
fn the_later_contributor_wins_and_disjoint_keys_compose() {
    let p = compose(
        &["frontend", "native"],
        project_contributions(),
        vec![named()],
    );
    assert_eq!(description(&p), "web native");
    assert_eq!(env_var(&p, "MODE"), "native");
    assert_eq!(env_var(&p, "A"), "1");
}

#[test]
fn reversing_the_order_flips_the_winner() {
    let p = compose(
        &["native", "frontend"],
        project_contributions(),
        vec![named()],
    );
    assert_eq!(description(&p), "web frontend");
    assert_eq!(env_var(&p, "MODE"), "frontend");
}

#[test]
fn arrival_order_never_decides() {
    let reference = compose(
        &["frontend", "native"],
        project_contributions(),
        vec![named()],
    );
    for arrival in permutations(&project_contributions()) {
        let p = compose(&["frontend", "native"], arrival, vec![named()]);
        assert_eq!(p, reference);
    }
}

#[test]
fn an_explicit_value_overrides_every_contributor() {
    let p = compose(
        &["frontend", "native"],
        project_contributions(),
        vec![explicit(
            "app/project.yaml",
            r#"{"name":"web","description":"web"}"#,
        )],
    );
    assert_eq!(description(&p), "web");
    assert_eq!(env_var(&p, "MODE"), "native");
}

#[test]
fn an_unrelated_explicit_override_leaves_the_contributors_value() {
    let p = compose(
        &["frontend", "native"],
        project_contributions(),
        vec![explicit(
            "app/project.yaml",
            r#"{"name":"web","tasks":{"t":"echo"}}"#,
        )],
    );
    assert_eq!(description(&p), "web native");
}

#[test]
fn identical_values_coalesce() {
    let same = vec![
        contribution("a", r#"{"description":"same"}"#),
        contribution("b", r#"{"description":"same"}"#),
    ];
    let p = compose(&["a", "b"], same, vec![named()]);
    assert_eq!(description(&p), "same");
}

#[test]
fn composition_errors_name_the_contributor() {
    let one = || vec![contribution("a", "{}")];
    assert_eq!(
        try_compose(&["a", "a"], one(), vec![]).unwrap_err(),
        ComposeError::DuplicateInOrder { id: "a".into() }
    );
    assert_eq!(
        try_compose(&["a", "b"], one(), vec![]).unwrap_err(),
        ComposeError::MissingContribution { id: "b".into() }
    );
    assert_eq!(
        try_compose(&["b"], one(), vec![]).unwrap_err(),
        ComposeError::UnknownContributor { id: "a".into() }
    );
    let twice = vec![contribution("a", "{}"), contribution("a", "{}")];
    assert_eq!(
        try_compose(&["a"], twice, vec![]).unwrap_err(),
        ComposeError::DuplicateContribution { id: "a".into() }
    );
}

fn deps(
    names: &[&str],
    contributions: Vec<Contribution>,
    explicit: Vec<LoadedLayer>,
) -> Vec<String> {
    items(&compose(names, contributions, explicit).config.dependencies)
}

fn c04() -> Vec<Contribution> {
    vec![
        contribution("frontend", r#"{"dependencies":{"append":["d"]}}"#),
        contribution("native", r#"{"dependencies":{"append":["b","c"]}}"#),
    ]
}

#[test]
fn c04_activation_local_results_concatenate_in_list_order() {
    assert_eq!(
        deps(&["frontend", "native"], c04(), vec![named()]),
        ["d", "b", "c"]
    );
    assert_eq!(
        deps(&["native", "frontend"], c04(), vec![named()]),
        ["b", "c", "d"]
    );
}

#[test]
fn c04_an_explicit_replace_removes_every_combined_edge() {
    let cleared = explicit(
        "app/project.yaml",
        r#"{"name":"web","dependencies":{"replace":[]}}"#,
    );
    assert!(deps(&["frontend", "native"], c04(), vec![cleared]).is_empty());
}

fn key_env(p: &FinalizedProject) -> Vec<String> {
    items(&p.config.cache.key.env)
}

fn c17() -> Vec<Contribution> {
    vec![contribution(
        "p",
        r#"{"cache":{"key":{"env":{"append":["A","A"]}}}}"#,
    )]
}

#[test]
fn c17_repetitions_survive_composition() {
    let p = compose(&["p"], c17(), vec![named()]);
    assert_eq!(key_env(&p), ["A", "A"]);
}

#[test]
fn c17_positional_explicit_operations_see_the_original_entries() {
    let merged = explicit(
        "app/project.yaml",
        r#"{"name":"web","cache":{"key":{"env":{"merge":["B"]}}}}"#,
    );
    let p = compose(&["p"], c17(), vec![merged]);
    assert_eq!(key_env(&p), ["B", "A"]);
}

#[test]
fn same_key_tasks_compose_like_ordinary_layers() {
    let docs = [
        r#"{"tasks":{"t":{"exec":"a","description":"da","dependencies":{"append":["x"]}}}}"#,
        r#"{"tasks":{"t":{"exec":"b","description":"db"}}}"#,
        r#"{"tasks":{"t":"short"}}"#,
    ];
    let contributions = || {
        vec![
            contribution("a", docs[0]),
            contribution("b", docs[1]),
            contribution("c", docs[2]),
        ]
    };
    let ordered = compose(&["a", "b", "c"], contributions(), vec![named()]);
    let mut fold = LoadedLayer::unattributed(
        serde_json::from_str::<ProjectLayer>(docs[0]).unwrap(),
    );
    for doc in &docs[1..] {
        fold.merge(LoadedLayer::unattributed(
            serde_json::from_str::<ProjectLayer>(doc).unwrap(),
        ));
    }
    fold.merge(named());
    let folded = fold.finalize_composed().unwrap();
    assert_eq!(
        serde_json::to_value(&ordered.config.tasks).unwrap(),
        serde_json::to_value(&folded.config.tasks).unwrap()
    );
}

#[test]
fn without_contributors_the_result_is_the_plain_fold() {
    let layers = || {
        vec![
            explicit(
                "lib/base.yaml",
                r#"{"name":"a","cache":{"enabled":true},"tasks":{"t":"x"}}"#,
            ),
            explicit("app/p.yaml", r#"{"cache":{"remote":false}}"#),
        ]
    };
    let ordered = compose(&[], vec![], layers());
    let mut it = layers().into_iter();
    let mut acc = it.next().unwrap();
    for next in it {
        acc.merge(next);
    }
    assert_eq!(ordered, acc.finalize_composed().unwrap());
}

#[test]
fn with_no_explicit_layer_the_project_root_is_the_given_root() {
    let composed = try_compose(
        &["a"],
        vec![contribution("a", r#"{"name":"web"}"#)],
        vec![],
    )
    .unwrap();
    assert_eq!(composed.layer.dir(), &OmniPath::new("root"));
}

fn forbidden(json: &str) -> ComposeError {
    try_compose(&["a"], vec![contribution("a", json)], vec![named()])
        .unwrap_err()
}

fn field_of(err: ComposeError) -> String {
    match err {
        ComposeError::ForbiddenForm {
            field, contributor, ..
        } => {
            assert_eq!(contributor, "a");
            field
        }
        other => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn destructive_or_host_owned_forms_are_rejected_by_field() {
    let cases = [
        (r#"{"dependencies":["x"]}"#, "dependencies"),
        (r#"{"dependencies":{"replace":["x"]}}"#, "dependencies"),
        (r#"{"dependencies":{"prepend":["x"]}}"#, "dependencies"),
        (r#"{"dependencies":{"merge":["x"]}}"#, "dependencies"),
        (r#"{"env":{"vars":{"replace":{"A":"1"}}}}"#, "env.vars"),
        (r#"{"meta":{"replace":{"a":1}}}"#, "meta"),
        (r#"{"cache":{"enabled":true}}"#, "cache.enabled"),
        (r#"{"cache":{"remote":false}}"#, "cache.remote"),
        (r#"{"cache":{"key":{"env":["A"]}}}"#, "cache.key.env"),
        (r#"{"cache":{"key":{"files":"src/**"}}}"#, "cache.key.files"),
        (
            r#"{"cache":{"key":{"files":["src/**"]}}}"#,
            "cache.key.files",
        ),
        (
            r#"{"cache":{"key":{"files":{"include":"src/**"}}}}"#,
            "cache.key.files",
        ),
        (
            r#"{"cache":{"output":{"files":["dist/**"]}}}"#,
            "cache.output.files",
        ),
        (r#"{"redact":{}}"#, "redact"),
        (r#"{"tasks":{"replace":{"t":"x"}}}"#, "tasks"),
        (
            r#"{"tasks":{"t":{"exec":"x","args":{"replace":{}}}}}"#,
            "tasks.t.args",
        ),
        (
            r#"{"tasks":{"t":{"exec":"x","dependencies":["u"]}}}"#,
            "tasks.t.dependencies",
        ),
        (
            r#"{"tasks":{"t":{"exec":"x","cache":{"enabled":true}}}}"#,
            "tasks.t.cache.enabled",
        ),
        (
            r#"{"tasks":{"t":{"exec":"x","cache":{"remote":true}}}}"#,
            "tasks.t.cache.remote",
        ),
        (
            r#"{"tasks":{"t":{"exec":"x","redact":{}}}}"#,
            "tasks.t.redact",
        ),
    ];
    for (json, field) in cases {
        assert_eq!(field_of(forbidden(json)), field, "{json}");
    }
}

#[test]
fn additive_forms_are_accepted() {
    let ok = contribution(
        "a",
        r#"{"dependencies":{"append":["x"]},
            "env":{"vars":{"A":"1"}},
            "cache":{"key":{"env":{"append":["A"]},
                            "files":{"include":{"append":["src/**"]}}}}}"#,
    );
    assert!(try_compose(&["a"], vec![ok], vec![named()]).is_ok());
}

#[test]
fn a_later_explicit_replace_cannot_hide_a_forbidden_form() {
    let cleared = explicit(
        "app/project.yaml",
        r#"{"name":"web","dependencies":{"replace":[]}}"#,
    );
    let bad = contribution("a", r#"{"dependencies":["x"]}"#);
    assert!(matches!(
        try_compose(&["a"], vec![bad], vec![cleared]).unwrap_err(),
        ComposeError::ForbiddenForm { .. }
    ));
}

fn with_decl(decl: EnabledDecl, contributor: &str) -> Contribution {
    let mut c = contribution(contributor, "{}");
    c.enabled.push(decl);
    c
}

#[test]
fn a_declaration_for_an_unknown_task_or_alias_is_an_error() {
    let task = explicit(
        "app/project.yaml",
        r#"{"name":"web","tasks":{"t":"x","a":{"alias":"t"}}}"#,
    );
    for name in ["missing", "a"] {
        let decl = emitted("p", EnabledScope::Task(name.into()), true);
        assert_eq!(
            try_compose(&["p"], vec![with_decl(decl, "p")], vec![task.clone()])
                .unwrap_err(),
            ComposeError::UnknownTask {
                contributor: "p".into(),
                task: name.into(),
            }
        );
    }
}

#[test]
fn a_declaration_carrying_another_contributors_origin_is_an_error() {
    let decl = emitted("other", EnabledScope::Project, true);
    assert_eq!(
        try_compose(&["p"], vec![with_decl(decl, "p")], vec![named()])
            .unwrap_err(),
        ComposeError::ForeignOrigin {
            contributor: "p".into()
        }
    );
}

fn tasked() -> LoadedLayer {
    explicit("app/project.yaml", r#"{"name":"web","tasks":{"t":"x"}}"#)
}

fn decided(
    names: &[&str],
    contributions: Vec<Contribution>,
    explicit: Vec<LoadedLayer>,
) -> (bool, CacheEnabledSource) {
    let p = compose(names, contributions, explicit);
    let d = p.task_cache_enabled["t"];
    (d.value, d.source)
}

#[test]
fn c01_through_the_entry_point() {
    use CacheEnabledSource::*;
    let task = |n: &str, v| emitted(n, EnabledScope::Task("t".into()), v);
    let project = |n: &str, v| emitted(n, EnabledScope::Project, v);
    // An earlier plugin's task value beats a later plugin's project value.
    assert_eq!(
        decided(
            &["a", "b"],
            vec![
                with_decl(task("a", true), "a"),
                with_decl(project("b", false), "b")
            ],
            vec![tasked()],
        ),
        (true, Plugin)
    );
    // Within a tier the later wins, and reversing flips it.
    let two = || {
        vec![
            with_decl(project("a", true), "a"),
            with_decl(project("b", false), "b"),
        ]
    };
    assert_eq!(decided(&["a", "b"], two(), vec![tasked()]), (false, Plugin));
    assert_eq!(decided(&["b", "a"], two(), vec![tasked()]), (true, Plugin));
    // An authored project value beats a plugin task value (row 7).
    let authored = explicit(
        "app/project.yaml",
        r#"{"name":"web","cache":{"enabled":false},"tasks":{"t":"x"}}"#,
    );
    assert_eq!(
        decided(
            &["a"],
            vec![with_decl(task("a", true), "a")],
            vec![authored]
        ),
        (false, ProjectExplicit)
    );
}

#[test]
fn an_applied_task_value_from_a_contributor_beats_an_explicit_project_value() {
    let authored = explicit(
        "app/project.yaml",
        r#"{"name":"web","cache":{"enabled":false},"tasks":{"t":"x"}}"#,
    );
    let decl = applied("apply/a.yaml", EnabledScope::Task("t".into()), true);
    assert_eq!(
        decided(&["a"], vec![with_decl(decl, "a")], vec![authored]),
        (true, CacheEnabledSource::TaskExplicit)
    );
}

#[test]
fn task_level_forms_are_guarded_too() {
    let cases = [
        (
            r#"{"tasks":{"t":{"exec":"x","with":["u"]}}}"#,
            "tasks.t.with",
        ),
        (
            r#"{"tasks":{"t":{"exec":"x","env":{"vars":{"replace":{}}}}}}"#,
            "tasks.t.env.vars",
        ),
        (
            r#"{"tasks":{"t":{"exec":"x","meta":{"replace":{}}}}}"#,
            "tasks.t.meta",
        ),
        (
            r#"{"tasks":{"t":{"exec":"x","cache":{"key":{"env":["A"]}}}}}"#,
            "tasks.t.cache.key.env",
        ),
        (
            r#"{"tasks":{"t":{"exec":"x","cache":{"key":{"files":"a"}}}}}"#,
            "tasks.t.cache.key.files",
        ),
        (
            r#"{"tasks":{"t":{"exec":"x","cache":{"output":{"files":["a"]}}}}}"#,
            "tasks.t.cache.output.files",
        ),
    ];
    for (json, field) in cases {
        assert_eq!(field_of(forbidden(json)), field, "{json}");
    }
}

#[test]
fn a_contribution_layer_that_was_already_composed_is_still_guarded() {
    let mut layer = LoadedLayer::unattributed(
        serde_json::from_str::<ProjectLayer>(r#"{"tasks":{"t":{"exec":"x"}}}"#)
            .unwrap(),
    );
    layer.merge(LoadedLayer::unattributed(
        serde_json::from_str::<ProjectLayer>(
            r#"{"tasks":{"t":{"cache":{"enabled":true}}}}"#,
        )
        .unwrap(),
    ));
    let contribution = Contribution {
        id: id("a"),
        layer: layer.layer().clone(),
        base_dir: OmniPath::new("a"),
        enabled: Vec::new(),
    };
    assert!(matches!(
        try_compose(&["a"], vec![contribution], vec![named()]).unwrap_err(),
        ComposeError::ForbiddenForm { field, .. } if field == "tasks.t.cache.enabled"
    ));
}

#[test]
fn a_declaration_for_a_task_dropped_by_an_explicit_layer_is_ignored() {
    let mut c = contribution("p", r#"{"tasks":{"t":"x"}}"#);
    c.enabled
        .push(emitted("p", EnabledScope::Task("t".into()), true));
    let dropped = explicit(
        "app/project.yaml",
        r#"{"name":"web","tasks":{"replace":{"u":"y"}}}"#,
    );
    let p = compose(&["p"], vec![c], vec![dropped]);
    assert!(!p.task_cache_enabled.contains_key("t"));
}

#[test]
fn alias_cycles_surface_when_the_composed_layer_is_finalized() {
    let cyclic = explicit(
        "app/project.yaml",
        r#"{"name":"web","tasks":{"a":{"alias":"b"},"b":{"alias":"a"}}}"#,
    );
    let composed = try_compose(&[], vec![], vec![cyclic]).unwrap();
    assert!(composed.layer.finalize_composed().is_err());
}
