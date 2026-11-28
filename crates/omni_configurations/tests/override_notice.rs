//! The aggregated plugin-override notice (RFC 0024 F07.4): counts only.

use std::path::Path;
use std::sync::Arc;

use omni_configurations::{
    Contribution, ContributorId, DeclarationSource, EnabledDecl, EnabledScope,
    FieldOverride, FieldPath, LoadedLayer, Origin, OverrideTier,
    PluginOverride, ProjectLayer, compose_ordered,
};
use omni_notice::{Notice, NoticeLevel};
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

fn explicit(json: &str) -> LoadedLayer {
    LoadedLayer::from_file(
        OmniPath::new("app/project.yaml"),
        OmniPath::new(Path::new("app")),
        serde_json::from_str::<ProjectLayer>(json).unwrap(),
    )
}

fn compose(
    root: &str,
    names: &[&str],
    contributions: Vec<Contribution>,
    explicit: Vec<LoadedLayer>,
) -> omni_configurations::Composed {
    let order: Vec<_> = names.iter().map(|n| id(n)).collect();
    compose_ordered(OmniPath::new(root), &order, contributions, explicit)
        .unwrap()
}

fn differing() -> Vec<Contribution> {
    vec![
        contribution("frontend", r#"{"description":"SECRET-A"}"#),
        contribution("native", r#"{"description":"SECRET-B"}"#),
    ]
}

fn record(root: &str, field: FieldPath, tier: OverrideTier) -> FieldOverride {
    FieldOverride {
        root: OmniPath::new(root),
        field,
        tier,
        winner: id("b"),
        overridden: vec![id("a")],
        masked_by_explicit: false,
    }
}

#[test]
fn the_notice_contract_is_pinned() {
    let notice = PluginOverride {
        fields: 3,
        projects: 2,
    };
    assert_eq!(notice.code(), "composition.plugin_override");
    assert_eq!(notice.level(), NoticeLevel::Info);
    assert!(!notice.essential());
    assert_eq!(
        notice.to_string(),
        "3 field(s) in 2 project(s) were overridden by a later plugin"
    );
    let fields = notice.fields();
    assert_eq!(fields["fields"], 3);
    assert_eq!(fields["projects"], 2);
    assert_eq!(fields.len(), 2);
}

#[test]
fn a_differing_value_raises_one_field_in_one_project() {
    let composed =
        compose("root", &["frontend", "native"], differing(), vec![]);
    assert_eq!(
        composed.override_notice(),
        Some(PluginOverride {
            fields: 1,
            projects: 1
        })
    );
    let reversed =
        compose("root", &["native", "frontend"], differing(), vec![]);
    assert_eq!(reversed.override_notice(), composed.override_notice());
}

#[test]
fn three_contributors_on_one_key_count_one_field() {
    let composed = compose(
        "root",
        &["a", "b", "c"],
        vec![
            contribution("a", r#"{"env":{"vars":{"MODE":"1"}}}"#),
            contribution("b", r#"{"env":{"vars":{"MODE":"2"}}}"#),
            contribution("c", r#"{"env":{"vars":{"MODE":"3"}}}"#),
        ],
        vec![],
    );
    assert_eq!(
        composed.override_notice(),
        Some(PluginOverride {
            fields: 1,
            projects: 1
        })
    );
}

#[test]
fn equal_values_and_additive_lists_raise_nothing() {
    let equal = compose(
        "root",
        &["a", "b"],
        vec![
            contribution("a", r#"{"description":"same"}"#),
            contribution("b", r#"{"description":"same"}"#),
        ],
        vec![],
    );
    assert_eq!(equal.override_notice(), None);
    let lists = compose(
        "root",
        &["a", "b"],
        vec![
            contribution(
                "a",
                r#"{"dependencies":{"append":["x"]},"cache":{"key":{"env":{"append":["A"]}}}}"#,
            ),
            contribution(
                "b",
                r#"{"dependencies":{"append":["y"]},"cache":{"key":{"env":{"append":["B"]}}}}"#,
            ),
        ],
        vec![],
    );
    assert_eq!(lists.override_notice(), None);
}

#[test]
fn an_explicit_layer_that_decides_the_field_raises_nothing() {
    let composed = compose(
        "root",
        &["frontend", "native"],
        differing(),
        vec![explicit(r#"{"name":"web","description":"web"}"#)],
    );
    assert!(composed.overrides.iter().all(|o| o.masked_by_explicit));
    assert!(!composed.overrides.is_empty());
    assert_eq!(composed.override_notice(), None);

    let meta = compose(
        "root",
        &["a", "b"],
        vec![
            contribution("a", r#"{"meta":{"k":{"x":1}}}"#),
            contribution("b", r#"{"meta":{"k":{"x":2}}}"#),
        ],
        vec![explicit(r#"{"name":"web","meta":{"replace":{"k":"z"}}}"#)],
    );
    assert!(!meta.overrides.is_empty());
    assert_eq!(meta.override_notice(), None);
}

#[test]
fn only_the_unmasked_field_of_a_mix_counts() {
    let composed = compose(
        "root",
        &["a", "b"],
        vec![
            contribution(
                "a",
                r#"{"description":"x","env":{"vars":{"MODE":"1"}}}"#,
            ),
            contribution(
                "b",
                r#"{"description":"y","env":{"vars":{"MODE":"2"}}}"#,
            ),
        ],
        vec![explicit(r#"{"name":"web","description":"web"}"#)],
    );
    assert_eq!(
        composed.override_notice(),
        Some(PluginOverride {
            fields: 1,
            projects: 1
        })
    );
}

fn enabled(name: &str, source: DeclarationSource, value: bool) -> Contribution {
    let mut c = contribution(name, "{}");
    c.enabled.push(EnabledDecl {
        scope: EnabledScope::Project,
        value,
        origin: Arc::new(Origin::new(source, OmniPath::new(name))),
    });
    c
}

#[test]
fn a_fileless_contributor_counts_like_a_file_backed_one() {
    // Plugin-emitted declarations have no file at all.
    let emitted = compose(
        "root",
        &["frontend", "native"],
        vec![
            enabled(
                "frontend",
                DeclarationSource::contributor(id("frontend")),
                true,
            ),
            enabled(
                "native",
                DeclarationSource::contributor(id("native")),
                false,
            ),
        ],
        vec![],
    );
    assert_eq!(
        emitted.override_notice(),
        Some(PluginOverride {
            fields: 1,
            projects: 1
        })
    );
    // Applied declarations: one from a file, one without.
    let applied = compose(
        "root",
        &["frontend", "native"],
        vec![
            enabled(
                "frontend",
                DeclarationSource::file(OmniPath::new("a/omni.yaml")),
                true,
            ),
            enabled("native", DeclarationSource::default(), false),
        ],
        vec![],
    );
    assert_eq!(applied.override_notice(), emitted.override_notice());
}

#[test]
fn counts_distinct_fields_and_projects_across_compositions() {
    let a = compose("root-a", &["frontend", "native"], differing(), vec![]);
    let b = compose("root-b", &["frontend", "native"], differing(), vec![]);
    let notice = PluginOverride::from_overrides(
        a.overrides.iter().chain(b.overrides.iter()),
    );
    assert_eq!(
        notice,
        Some(PluginOverride {
            fields: 2,
            projects: 2
        })
    );

    let two_fields = [
        record("r", FieldPath::Description, OverrideTier::Emitted),
        record("r", FieldPath::EnvVar("A".into()), OverrideTier::Emitted),
    ];
    assert_eq!(
        PluginOverride::from_overrides(&two_fields),
        Some(PluginOverride {
            fields: 2,
            projects: 1
        })
    );
}

#[test]
fn emitted_and_applied_records_of_one_field_count_once() {
    let both = [
        record("r", FieldPath::ProjectCacheEnabled, OverrideTier::Emitted),
        record("r", FieldPath::ProjectCacheEnabled, OverrideTier::Applied),
    ];
    assert_eq!(
        PluginOverride::from_overrides(&both),
        Some(PluginOverride {
            fields: 1,
            projects: 1
        })
    );
}

#[test]
fn masked_or_empty_records_count_nothing() {
    let mut masked = record("r", FieldPath::Description, OverrideTier::Emitted);
    masked.masked_by_explicit = true;
    let mut empty = record("r", FieldPath::OutputLogs, OverrideTier::Emitted);
    empty.overridden.clear();
    assert_eq!(PluginOverride::from_overrides(&[masked, empty]), None);
    assert_eq!(PluginOverride::from_overrides(&[]), None);
}

#[test]
fn the_message_never_carries_values_ids_or_paths() {
    let composed = compose(
        "sentinel-root",
        &["frontend", "native"],
        differing(),
        vec![],
    );
    let text = composed.override_notice().unwrap().to_string();
    for leak in [
        "SECRET",
        "frontend",
        "native",
        "sentinel-root",
        "description",
    ] {
        assert!(!text.contains(leak), "{text}");
    }
}

#[test]
fn asking_for_the_notice_does_not_change_composition() {
    let composed =
        compose("root", &["frontend", "native"], differing(), vec![]);
    let before = (composed.overrides.clone(), format!("{:?}", composed.layer));
    let _ = composed.override_notice();
    assert_eq!(
        before,
        (composed.overrides.clone(), format!("{:?}", composed.layer))
    );
}
