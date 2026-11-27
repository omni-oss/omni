//! Override records of ordered composition (RFC 0024 F07.3): which earlier
//! contributors a later one overrode, by structured field path, with no values.

use std::path::Path;
use std::sync::Arc;

use omni_configurations::{
    Contribution, ContributorId, DeclarationSource, EnabledDecl, EnabledScope,
    FieldOverride, FieldPath, LoadedLayer, Origin, OverrideTier, ProjectLayer,
    compose_ordered,
};
use omni_types::OmniPath;

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

fn with_enabled(
    name: &str,
    scope: EnabledScope,
    value: bool,
    applied_file: Option<&str>,
) -> Contribution {
    let source = match applied_file {
        Some(file) => DeclarationSource::file(OmniPath::new(file)),
        None => DeclarationSource::contributor(id(name)),
    };
    let mut c = contribution(name, "{}");
    c.enabled.push(EnabledDecl {
        scope,
        value,
        origin: Arc::new(Origin::new(source, OmniPath::new(name))),
    });
    c
}

fn explicit(json: &str) -> LoadedLayer {
    LoadedLayer::from_file(
        OmniPath::new("app/project.yaml"),
        OmniPath::new(Path::new("app")),
        serde_json::from_str::<ProjectLayer>(json).unwrap(),
    )
}

fn overrides(
    names: &[&str],
    contributions: Vec<Contribution>,
    explicit: Vec<LoadedLayer>,
) -> Vec<FieldOverride> {
    let order: Vec<_> = names.iter().map(|n| id(n)).collect();
    compose_ordered(OmniPath::new("root"), &order, contributions, explicit)
        .unwrap()
        .overrides
}

fn project() -> Vec<Contribution> {
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

fn find<'a>(
    all: &'a [FieldOverride],
    field: &FieldPath,
) -> Option<&'a FieldOverride> {
    all.iter().find(|o| &o.field == field)
}

fn ids(names: &[&str]) -> Vec<ContributorId> {
    names.iter().map(|n| id(n)).collect()
}

#[test]
fn c12_the_later_contributor_is_the_winner_and_the_earlier_is_overridden() {
    let all = overrides(&["frontend", "native"], project(), vec![]);
    let description = find(&all, &FieldPath::Description).unwrap();
    assert_eq!(description.winner, id("native"));
    assert_eq!(description.overridden, ids(&["frontend"]));
    assert!(!description.masked_by_explicit);
    assert_eq!(description.root, OmniPath::new("root"));
    let mode = find(&all, &FieldPath::EnvVar("MODE".into())).unwrap();
    assert_eq!(mode.winner, id("native"));
    assert!(find(&all, &FieldPath::EnvVar("A".into())).is_none());
}

#[test]
fn reversing_the_order_flips_winner_and_overridden() {
    let all = overrides(&["native", "frontend"], project(), vec![]);
    let description = find(&all, &FieldPath::Description).unwrap();
    assert_eq!(description.winner, id("frontend"));
    assert_eq!(description.overridden, ids(&["native"]));
}

#[test]
fn arrival_order_does_not_change_the_records() {
    let forward = overrides(&["frontend", "native"], project(), vec![]);
    let mut arrival = project();
    arrival.reverse();
    assert_eq!(overrides(&["frontend", "native"], arrival, vec![]), forward);
}

#[test]
fn an_explicit_value_masks_the_override_but_still_lists_it() {
    let all = overrides(
        &["frontend", "native"],
        project(),
        vec![explicit(r#"{"name":"web","description":"web"}"#)],
    );
    let description = find(&all, &FieldPath::Description).unwrap();
    assert!(description.masked_by_explicit);
    assert_eq!(description.overridden, ids(&["frontend"]));
    let mode = find(&all, &FieldPath::EnvVar("MODE".into())).unwrap();
    assert!(!mode.masked_by_explicit);
}

#[test]
fn an_unrelated_explicit_override_masks_nothing() {
    let all = overrides(
        &["frontend", "native"],
        project(),
        vec![explicit(r#"{"name":"web","tasks":{"t":"echo"}}"#)],
    );
    assert!(all.iter().all(|o| !o.masked_by_explicit));
}

#[test]
fn an_explicit_dictionary_replace_masks_every_key() {
    let all = overrides(
        &["frontend", "native"],
        project(),
        vec![explicit(r#"{"name":"web","env":{"vars":{"replace":{}}}}"#)],
    );
    let mode = find(&all, &FieldPath::EnvVar("MODE".into())).unwrap();
    assert!(mode.masked_by_explicit);
}

#[test]
fn equal_values_with_distinct_origins_record_nothing() {
    let same = vec![
        contribution("a", r#"{"description":"same"}"#),
        contribution("b", r#"{"description":"same"}"#),
    ];
    assert!(overrides(&["a", "b"], same, vec![]).is_empty());
}

#[test]
fn only_contributors_that_differ_from_the_winner_are_overridden() {
    let three = vec![
        contribution("a", r#"{"description":"x"}"#),
        contribution("b", r#"{"description":"y"}"#),
        contribution("c", r#"{"description":"x"}"#),
    ];
    let all = overrides(&["a", "b", "c"], three, vec![]);
    let description = find(&all, &FieldPath::Description).unwrap();
    assert_eq!(description.winner, id("c"));
    assert_eq!(description.overridden, ids(&["b"]));
}

fn meta(name: &str, json: &str) -> Contribution {
    contribution(name, &format!(r#"{{"meta":{json}}}"#))
}

fn meta_path(parts: &[&str]) -> FieldPath {
    FieldPath::Meta(parts.iter().map(|p| p.to_string()).collect())
}

#[test]
fn disjoint_nested_meta_keys_compose_without_a_record() {
    let all = overrides(
        &["a", "b"],
        vec![meta("a", r#"{"n":{"a":1}}"#), meta("b", r#"{"n":{"b":2}}"#)],
        vec![],
    );
    assert!(all.is_empty());
}

#[test]
fn an_overlapping_nested_scalar_is_recorded_at_its_nested_path() {
    let all = overrides(
        &["a", "b"],
        vec![meta("a", r#"{"n":{"a":1}}"#), meta("b", r#"{"n":{"a":2}}"#)],
        vec![],
    );
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].field, meta_path(&["n", "a"]));
    assert_eq!(all[0].winner, id("b"));
}

#[test]
fn a_type_change_in_meta_is_an_override() {
    let all = overrides(
        &["a", "b"],
        vec![meta("a", r#"{"n":{"a":1}}"#), meta("b", r#"{"n":5}"#)],
        vec![],
    );
    assert_eq!(find(&all, &meta_path(&["n"])).unwrap().winner, id("b"));
    let scalar = overrides(
        &["a", "b"],
        vec![meta("a", r#"{"v":1}"#), meta("b", r#"{"v":"1"}"#)],
        vec![],
    );
    assert_eq!(scalar.len(), 1);
}

#[test]
fn an_explicit_meta_value_masks_nested_overrides() {
    let all = overrides(
        &["a", "b"],
        vec![meta("a", r#"{"n":{"a":1}}"#), meta("b", r#"{"n":{"a":2}}"#)],
        vec![explicit(r#"{"name":"w","meta":{"n":7}}"#)],
    );
    assert!(all[0].masked_by_explicit);
    let sibling = overrides(
        &["a", "b"],
        vec![meta("a", r#"{"n":{"a":1}}"#), meta("b", r#"{"n":{"a":2}}"#)],
        vec![explicit(r#"{"name":"w","meta":{"n":{"other":7}}}"#)],
    );
    assert!(!sibling[0].masked_by_explicit);
}

fn logs(name: &str, json: &str) -> Contribution {
    contribution(name, &format!(r#"{{"output_logs":{json}}}"#))
}

#[test]
fn output_logs_are_compared_per_facet() {
    let same = overrides(
        &["a", "b"],
        vec![
            logs("a", r#""all""#),
            logs("b", r#"{"new":"all","cached":"all"}"#),
        ],
        vec![],
    );
    assert!(same.is_empty());
    let disjoint = overrides(
        &["a", "b"],
        vec![
            logs("a", r#"{"new":"all"}"#),
            logs("b", r#"{"cached":"never"}"#),
        ],
        vec![],
    );
    assert!(disjoint.is_empty());
    let conflicting = overrides(
        &["a", "b"],
        vec![
            logs("a", r#"{"new":"all"}"#),
            logs("b", r#"{"new":"never"}"#),
        ],
        vec![],
    );
    assert_eq!(conflicting[0].field, FieldPath::OutputLogs);
}

#[test]
fn cache_key_defaults_are_tracked() {
    let all = overrides(
        &["a", "b"],
        vec![
            contribution("a", r#"{"cache":{"key":{"defaults":true}}}"#),
            contribution("b", r#"{"cache":{"key":{"defaults":false}}}"#),
        ],
        vec![],
    );
    assert_eq!(all[0].field, FieldPath::CacheKeyDefaults);
}

fn tasked() -> LoadedLayer {
    explicit(r#"{"name":"w","tasks":{"t":"x"}}"#)
}

#[test]
fn cache_enabled_overrides_compare_within_a_tier_and_scope() {
    let project = |n, v| with_enabled(n, EnabledScope::Project, v, None);
    let all = overrides(
        &["a", "b"],
        vec![project("a", true), project("b", false)],
        vec![tasked()],
    );
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].field, FieldPath::ProjectCacheEnabled);
    assert_eq!(all[0].tier, OverrideTier::Emitted);
    assert_eq!(all[0].winner, id("b"));
    assert!(!all[0].masked_by_explicit);
}

#[test]
fn a_tier_difference_is_not_an_override() {
    let all = overrides(
        &["a", "b"],
        vec![
            with_enabled("a", EnabledScope::Task("t".into()), true, None),
            with_enabled("b", EnabledScope::Project, false, None),
        ],
        vec![tasked()],
    );
    assert!(all.is_empty());
}

#[test]
fn authored_values_mask_a_plugin_cache_enabled_override() {
    let task = |n, v| with_enabled(n, EnabledScope::Task("t".into()), v, None);
    let pair = || vec![task("a", true), task("b", false)];
    let explicit_project =
        explicit(r#"{"name":"w","cache":{"enabled":true},"tasks":{"t":"x"}}"#);
    let all = overrides(&["a", "b"], pair(), vec![explicit_project]);
    assert!(all[0].masked_by_explicit);
    let explicit_task = explicit(
        r#"{"name":"w","tasks":{"t":{"exec":"x","cache":{"enabled":true}}}}"#,
    );
    assert!(
        overrides(&["a", "b"], pair(), vec![explicit_task])[0]
            .masked_by_explicit
    );
    let mut applied = pair();
    applied.push(with_enabled(
        "c",
        EnabledScope::Project,
        true,
        Some("apply/c.yaml"),
    ));
    let all = overrides(&["a", "b", "c"], applied, vec![tasked()]);
    let task_override =
        find(&all, &FieldPath::TaskCacheEnabled("t".into())).unwrap();
    assert!(task_override.masked_by_explicit);
}

#[test]
fn applied_values_are_compared_among_themselves() {
    let applied =
        |n, v| with_enabled(n, EnabledScope::Project, v, Some("apply/x.yaml"));
    let all = overrides(
        &["a", "b"],
        vec![applied("a", true), applied("b", false)],
        vec![tasked()],
    );
    assert_eq!(all[0].tier, OverrideTier::Applied);
    assert!(!all[0].masked_by_explicit);
    let masked = overrides(
        &["a", "b"],
        vec![applied("a", true), applied("b", false)],
        vec![explicit(
            r#"{"name":"w","cache":{"enabled":true},"tasks":{"t":"x"}}"#,
        )],
    );
    assert!(masked[0].masked_by_explicit);
}

fn nested(name: &str, n: &str) -> Contribution {
    meta(name, &format!(r#"{{"n":{n}}}"#))
}

#[test]
fn an_explicit_dictionary_replace_at_the_exact_path_masks_it() {
    let all = overrides(
        &["a", "b"],
        vec![nested("a", r#"{"a":1}"#), nested("b", "5")],
        vec![explicit(r#"{"name":"w","meta":{"n":{"replace":{}}}}"#)],
    );
    assert!(find(&all, &meta_path(&["n"])).unwrap().masked_by_explicit);
}

#[test]
fn an_explicit_meta_replace_masks_every_meta_override() {
    let all = overrides(
        &["a", "b"],
        vec![nested("a", "1"), nested("b", "2")],
        vec![explicit(r#"{"name":"w","meta":{"replace":{}}}"#)],
    );
    assert!(all[0].masked_by_explicit);
}

#[test]
fn an_explicit_dictionary_replaces_a_scalar_winner_but_merges_into_a_dictionary()
 {
    let scalar = overrides(
        &["a", "b"],
        vec![nested("a", "1"), nested("b", "5")],
        vec![explicit(r#"{"name":"w","meta":{"n":{"x":1}}}"#)],
    );
    assert!(
        find(&scalar, &meta_path(&["n"]))
            .unwrap()
            .masked_by_explicit
    );
    let dict = overrides(
        &["a", "b"],
        vec![nested("a", "1"), nested("b", r#"{"x":1}"#)],
        vec![explicit(r#"{"name":"w","meta":{"n":{"y":1}}}"#)],
    );
    assert!(!find(&dict, &meta_path(&["n"])).unwrap().masked_by_explicit);
}

#[test]
fn a_nested_record_is_dropped_when_a_later_contributor_makes_the_parent_a_scalar()
 {
    let all = overrides(
        &["a", "b", "c"],
        vec![
            nested("a", r#"{"x":1}"#),
            nested("b", r#"{"x":2}"#),
            nested("c", "5"),
        ],
        vec![],
    );
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].field, meta_path(&["n"]));
    assert_eq!(all[0].overridden, ids(&["a", "b"]));
}

#[test]
fn output_logs_masks_need_every_overridden_facet_set_explicitly() {
    let pair = || {
        vec![
            logs("a", r#"{"new":"all"}"#),
            logs("b", r#"{"new":"never"}"#),
        ]
    };
    let other_facet = overrides(
        &["a", "b"],
        pair(),
        vec![explicit(
            r#"{"name":"w","output_logs":{"cached":"failed"}}"#,
        )],
    );
    assert!(!other_facet[0].masked_by_explicit);
    let same_facet = overrides(
        &["a", "b"],
        pair(),
        vec![explicit(r#"{"name":"w","output_logs":{"new":"failed"}}"#)],
    );
    assert!(same_facet[0].masked_by_explicit);
}

#[test]
fn the_output_logs_winner_is_the_last_contributor_that_set_an_overridden_facet()
{
    let all = overrides(
        &["a", "b", "c"],
        vec![
            logs("a", r#"{"new":"all"}"#),
            logs("b", r#"{"new":"never"}"#),
            logs("c", r#"{"cached":"failed"}"#),
        ],
        vec![],
    );
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].winner, id("b"));
    assert_eq!(all[0].overridden, ids(&["a"]));
}

#[test]
fn an_explicit_tasks_replace_masks_task_cache_enabled_overrides() {
    let task = |n, v| with_enabled(n, EnabledScope::Task("t".into()), v, None);
    let all = overrides(
        &["a", "b"],
        vec![task("a", true), task("b", false)],
        vec![explicit(r#"{"name":"w","tasks":{"replace":{"t":"x"}}}"#)],
    );
    assert!(all[0].masked_by_explicit);
}

#[test]
fn output_logs_overridden_contributors_keep_composition_order() {
    let all = overrides(
        &["a", "b", "c"],
        vec![
            logs("a", r#"{"cached":"all"}"#),
            logs("b", r#"{"new":"all"}"#),
            logs("c", r#"{"new":"never","cached":"never"}"#),
        ],
        vec![],
    );
    assert_eq!(all[0].overridden, ids(&["a", "b"]));
}

#[test]
fn an_explicit_additive_list_does_not_mask_a_meta_override() {
    let all = overrides(
        &["a", "b"],
        vec![nested("a", "1"), nested("b", r#"{"append":[2]}"#)],
        vec![explicit(r#"{"name":"w","meta":{"n":{"append":[3]}}}"#)],
    );
    assert!(!all[0].masked_by_explicit);
}

#[test]
fn records_never_carry_values() {
    let all = overrides(
        &["p1", "p2"],
        vec![
            contribution(
                "p1",
                r#"{"description":"SECRET-ONE","env":{"vars":{"K":"SECRET-ONE"}}}"#,
            ),
            contribution(
                "p2",
                r#"{"description":"SECRET-TWO","env":{"vars":{"K":"SECRET-TWO"}}}"#,
            ),
        ],
        vec![],
    );
    assert_eq!(all.len(), 2);
    let text = format!("{all:?}");
    assert!(!text.contains("SECRET"), "{text}");
}
