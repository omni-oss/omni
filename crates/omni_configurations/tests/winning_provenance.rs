//! Winning and overridden `cache.enabled` provenance for inspection (RFC 0024
//! F07.5): which contributor declared the winning value, where an overridden
//! one was declared, and which contributors lost.

use std::path::Path;
use std::sync::Arc;

use omni_configurations::{
    Contribution, ContributorId, DeclarationSource, EnabledDecl, EnabledScope,
    LoadedLayer, Origin, ProjectLayer, attach_cache_enabled_overrides,
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

/// A path from parts, so no test asserts a separator literal.
fn path(parts: &[&str]) -> OmniPath {
    let mut path = std::path::PathBuf::new();
    for part in parts {
        path.push(part);
    }
    OmniPath::new(path)
}

fn with_enabled(
    name: &str,
    scope: EnabledScope,
    value: bool,
    applied_file: Option<&[&str]>,
) -> Contribution {
    let source = match applied_file {
        Some(parts) => DeclarationSource::file(path(parts)),
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

fn project() -> LoadedLayer {
    explicit(r#"{"name":"web","tasks":{"t":"echo"}}"#)
}

fn finalized(json: &str) -> omni_configurations::FinalizedProject {
    explicit(json).finalize_composed().unwrap()
}

#[test]
fn a_plugin_override_records_both_contributors_without_a_file() {
    let composed = compose_ordered(
        OmniPath::new(Path::new("app")),
        &[id("frontend"), id("native")],
        vec![
            with_enabled("frontend", EnabledScope::Project, true, None),
            with_enabled("native", EnabledScope::Project, false, None),
        ],
        vec![project()],
    )
    .unwrap();
    assert_eq!(composed.override_locations.len(), 1);
    let location = &composed.override_locations[0];
    assert_eq!(location.contributor, id("frontend"));
    assert_eq!(
        location.origin.source.contributor_id(),
        Some(&id("frontend"))
    );
    assert!(location.origin.source.file_path().is_none());
}

#[test]
fn an_applied_override_records_the_applied_file() {
    let composed = compose_ordered(
        OmniPath::new(Path::new("app")),
        &[id("frontend"), id("native")],
        vec![
            with_enabled(
                "frontend",
                EnabledScope::Project,
                true,
                Some(&["frontend", "omni.yaml"]),
            ),
            with_enabled(
                "native",
                EnabledScope::Project,
                false,
                Some(&["native", "omni.yaml"]),
            ),
        ],
        vec![project()],
    )
    .unwrap();
    assert_eq!(composed.override_locations.len(), 1);
    assert_eq!(
        composed.override_locations[0]
            .origin
            .source
            .file_path()
            .map(|p| p.path().unwrap().to_path_buf()),
        Some(Path::new("frontend").join("omni.yaml"))
    );
}

#[test]
fn a_fileless_applied_override_has_no_file() {
    let mut fileless = contribution("frontend", "{}");
    fileless.enabled.push(EnabledDecl {
        scope: EnabledScope::Project,
        value: true,
        origin: Arc::new(Origin::new(
            DeclarationSource::default(),
            OmniPath::new("frontend"),
        )),
    });
    let composed = compose_ordered(
        OmniPath::new(Path::new("app")),
        &[id("frontend"), id("native")],
        vec![
            fileless,
            with_enabled(
                "native",
                EnabledScope::Project,
                false,
                Some(&["native", "omni.yaml"]),
            ),
        ],
        vec![project()],
    )
    .unwrap();
    assert_eq!(composed.override_locations.len(), 1);
    assert!(
        composed.override_locations[0]
            .origin
            .source
            .file_path()
            .is_none()
    );
}

#[test]
fn locations_cover_only_cache_enabled_and_never_values() {
    let composed = compose_ordered(
        OmniPath::new(Path::new("app")),
        &[id("frontend"), id("native")],
        vec![
            contribution(
                "frontend",
                r#"{"description":"SENTINEL_A","dependencies":{"append":["x"]}}"#,
            ),
            contribution(
                "native",
                r#"{"description":"SENTINEL_B","dependencies":{"append":["y"]}}"#,
            ),
        ],
        vec![project()],
    )
    .unwrap();
    assert!(!composed.overrides.is_empty());
    assert!(composed.override_locations.is_empty());
}

/// Sentinel configuration values sit next to a real `cache.enabled` override;
/// the location carries the contributor and its origin only.
#[test]
fn a_location_carries_no_configuration_value() {
    let mut frontend = Contribution {
        layer: serde_json::from_str::<ProjectLayer>(
            r#"{"description":"SENTINEL_A","env":{"vars":{"MODE":"SENTINEL_MODE"}}}"#,
        )
        .unwrap(),
        ..with_enabled(
            "frontend",
            EnabledScope::Project,
            true,
            Some(&["frontend", "omni.yaml"]),
        )
    };
    frontend.enabled.push(EnabledDecl {
        scope: EnabledScope::Project,
        value: true,
        origin: Arc::new(Origin::new(
            DeclarationSource::file(path(&["frontend", "omni.yaml"])),
            OmniPath::new("frontend"),
        )),
    });
    let mut native = Contribution {
        layer: serde_json::from_str::<ProjectLayer>(
            r#"{"description":"SENTINEL_B"}"#,
        )
        .unwrap(),
        ..with_enabled(
            "native",
            EnabledScope::Project,
            false,
            Some(&["native", "omni.yaml"]),
        )
    };
    native.enabled.push(EnabledDecl {
        scope: EnabledScope::Project,
        value: false,
        origin: Arc::new(Origin::new(
            DeclarationSource::file(path(&["native", "omni.yaml"])),
            OmniPath::new("native"),
        )),
    });
    let composed = compose_ordered(
        OmniPath::new(Path::new("app")),
        &[id("frontend"), id("native")],
        vec![frontend, native],
        vec![project()],
    )
    .unwrap();
    assert_eq!(composed.override_locations.len(), 1);
    let location = format!("{:?}", composed.override_locations[0]);
    for leak in ["SENTINEL", "true", "false", "description", "MODE"] {
        assert!(!location.contains(leak), "{leak} in {location}");
    }
    assert!(
        location.contains("frontend"),
        "the contributor id is kept: {location}"
    );
}

#[test]
fn the_last_differing_declaration_supplies_the_location() {
    let mut frontend = with_enabled(
        "frontend",
        EnabledScope::Project,
        false,
        Some(&["frontend", "omni.yaml"]),
    );
    frontend.enabled.push(EnabledDecl {
        scope: EnabledScope::Project,
        value: true,
        origin: Arc::new(Origin::new(
            DeclarationSource::file(OmniPath::new("frontend/late.yaml")),
            OmniPath::new("frontend"),
        )),
    });
    let composed = compose_ordered(
        OmniPath::new(Path::new("app")),
        &[id("frontend"), id("native")],
        vec![
            frontend,
            with_enabled(
                "native",
                EnabledScope::Project,
                false,
                Some(&["native", "omni.yaml"]),
            ),
        ],
        vec![project()],
    )
    .unwrap();
    assert_eq!(
        composed.override_locations[0]
            .origin
            .source
            .file_path()
            .map(|p| p.path().unwrap().to_path_buf()),
        Some(Path::new("frontend").join("late.yaml"))
    );
}

#[test]
fn attach_fills_the_plugin_decision_from_emitted_records_only() {
    let composed = compose_ordered(
        OmniPath::new(Path::new("app")),
        &[id("frontend"), id("native")],
        vec![
            with_enabled("frontend", EnabledScope::Project, true, None),
            with_enabled("native", EnabledScope::Project, false, None),
        ],
        vec![project()],
    )
    .unwrap();
    let mut project = composed.layer.clone().finalize_composed().unwrap();
    attach_cache_enabled_overrides(
        &mut project,
        &composed.overrides,
        &composed.override_locations,
    );
    let overridden = &project.task_cache_enabled_overridden["t"];
    assert_eq!(overridden.len(), 1);
    assert_eq!(overridden[0].contributor, id("frontend"));
    assert!(overridden[0].file.is_none());
    assert_eq!(
        project.task_cache_enabled_scope["t"],
        omni_configurations::CacheScope::Project
    );
}

#[test]
fn attach_uses_the_applied_file_of_an_applied_override() {
    let composed = compose_ordered(
        OmniPath::new(Path::new("app")),
        &[id("frontend"), id("native")],
        vec![
            with_enabled(
                "frontend",
                EnabledScope::Task("t".into()),
                true,
                Some(&["frontend", "omni.yaml"]),
            ),
            with_enabled(
                "native",
                EnabledScope::Task("t".into()),
                false,
                Some(&["native", "omni.yaml"]),
            ),
        ],
        vec![project()],
    )
    .unwrap();
    let mut project = composed.layer.clone().finalize_composed().unwrap();
    attach_cache_enabled_overrides(
        &mut project,
        &composed.overrides,
        &composed.override_locations,
    );
    let overridden = &project.task_cache_enabled_overridden["t"];
    assert_eq!(overridden[0].contributor, id("frontend"));
    assert_eq!(
        overridden[0]
            .file
            .as_ref()
            .map(|f| f.path().unwrap().to_path_buf()),
        Some(Path::new("frontend").join("omni.yaml"))
    );
}

#[test]
fn attach_skips_an_explicitly_masked_record() {
    let composed = compose_ordered(
        OmniPath::new(Path::new("app")),
        &[id("frontend"), id("native")],
        vec![
            with_enabled("frontend", EnabledScope::Project, true, None),
            with_enabled("native", EnabledScope::Project, false, None),
        ],
        vec![explicit(
            r#"{"name":"web","tasks":{"t":"echo"},"cache":{"enabled":true}}"#,
        )],
    )
    .unwrap();
    assert!(composed.overrides.iter().all(|o| o.masked_by_explicit));
    let mut project = composed.layer.clone().finalize_composed().unwrap();
    attach_cache_enabled_overrides(
        &mut project,
        &composed.overrides,
        &composed.override_locations,
    );
    assert!(project.task_cache_enabled_overridden.is_empty());
}

#[test]
fn attach_skips_a_record_of_another_project_root() {
    let composed = compose_ordered(
        OmniPath::new(Path::new("elsewhere")),
        &[id("frontend"), id("native")],
        vec![
            with_enabled("frontend", EnabledScope::Project, true, None),
            with_enabled("native", EnabledScope::Project, false, None),
        ],
        vec![project()],
    )
    .unwrap();
    let mut project = composed.layer.clone().finalize_composed().unwrap();
    attach_cache_enabled_overrides(
        &mut project,
        &composed.overrides,
        &composed.override_locations,
    );
    assert!(project.task_cache_enabled_overridden.is_empty());
}

#[test]
fn attach_matches_the_root_when_composed_without_explicit_layers() {
    let composed = compose_ordered(
        OmniPath::new(Path::new("app")),
        &[id("frontend"), id("native")],
        vec![
            with_enabled("frontend", EnabledScope::Project, true, None),
            Contribution {
                layer: serde_json::from_str::<ProjectLayer>(
                    r#"{"name":"web","tasks":{"t":"echo"}}"#,
                )
                .unwrap(),
                ..with_enabled("native", EnabledScope::Project, false, None)
            },
        ],
        vec![],
    )
    .unwrap();
    let mut project = composed.layer.clone().finalize_composed().unwrap();
    attach_cache_enabled_overrides(
        &mut project,
        &composed.overrides,
        &composed.override_locations,
    );
    assert_eq!(project.dir, OmniPath::new(Path::new("app")));
    assert!(!project.task_cache_enabled_overridden.is_empty());
}

#[test]
fn an_explicit_only_project_has_no_overridden_entries() {
    let finalized = finalized(
        r#"{"name":"web","tasks":{"t":"echo"},"cache":{"enabled":true}}"#,
    );
    assert!(finalized.task_cache_enabled_overridden.is_empty());
}

#[test]
fn attach_lists_several_overridden_contributors_in_order() {
    let composed = compose_ordered(
        OmniPath::new(Path::new("app")),
        &[id("a"), id("b"), id("c")],
        vec![
            with_enabled("a", EnabledScope::Project, true, None),
            with_enabled("b", EnabledScope::Project, true, None),
            with_enabled("c", EnabledScope::Project, false, None),
        ],
        vec![project()],
    )
    .unwrap();
    let mut project = composed.layer.clone().finalize_composed().unwrap();
    attach_cache_enabled_overrides(
        &mut project,
        &composed.overrides,
        &composed.override_locations,
    );
    let overridden = &project.task_cache_enabled_overridden["t"];
    let ids: Vec<_> =
        overridden.iter().map(|o| o.contributor.as_str()).collect();
    assert_eq!(ids, ["a", "b"]);
}

#[test]
fn a_task_scope_plugin_decision_matches_only_its_task_record() {
    // Project values agree, so only the task-scope record exists. The task
    // that inherits the project value must not receive it.
    let composed = compose_ordered(
        OmniPath::new(Path::new("app")),
        &[id("a"), id("b")],
        vec![
            Contribution {
                enabled: vec![
                    EnabledDecl {
                        scope: EnabledScope::Project,
                        value: true,
                        origin: Arc::new(Origin::new(
                            DeclarationSource::contributor(id("a")),
                            OmniPath::new("a"),
                        )),
                    },
                    EnabledDecl {
                        scope: EnabledScope::Task("t".into()),
                        value: true,
                        origin: Arc::new(Origin::new(
                            DeclarationSource::contributor(id("a")),
                            OmniPath::new("a"),
                        )),
                    },
                ],
                ..contribution("a", "{}")
            },
            Contribution {
                enabled: vec![
                    EnabledDecl {
                        scope: EnabledScope::Project,
                        value: true,
                        origin: Arc::new(Origin::new(
                            DeclarationSource::contributor(id("b")),
                            OmniPath::new("b"),
                        )),
                    },
                    EnabledDecl {
                        scope: EnabledScope::Task("t".into()),
                        value: false,
                        origin: Arc::new(Origin::new(
                            DeclarationSource::contributor(id("b")),
                            OmniPath::new("b"),
                        )),
                    },
                ],
                ..contribution("b", "{}")
            },
        ],
        vec![explicit(
            r#"{"name":"web","tasks":{"t":"echo","u":"echo"}}"#,
        )],
    )
    .unwrap();
    let mut project = composed.layer.clone().finalize_composed().unwrap();
    // `t` is decided by the plugin task slot, `u` by the plugin project slot.
    assert_eq!(
        project.task_cache_enabled_scope["t"],
        omni_configurations::CacheScope::Task
    );
    assert_eq!(
        project.task_cache_enabled_scope["u"],
        omni_configurations::CacheScope::Project
    );
    attach_cache_enabled_overrides(
        &mut project,
        &composed.overrides,
        &composed.override_locations,
    );
    assert_eq!(project.task_cache_enabled_overridden["t"].len(), 1);
    assert!(project.task_cache_enabled_overridden.get("u").is_none());
}

#[test]
fn an_applied_tier_decision_matches_applied_records_only() {
    // Two applied files disagree on the project value; the explicit task
    // decision of `t` is not the applied record's decision.
    let composed = compose_ordered(
        OmniPath::new(Path::new("app")),
        &[id("a"), id("b")],
        vec![
            with_enabled(
                "a",
                EnabledScope::Project,
                true,
                Some(&["a", "omni.yaml"]),
            ),
            with_enabled(
                "b",
                EnabledScope::Project,
                false,
                Some(&["b", "omni.yaml"]),
            ),
        ],
        vec![explicit(
            r#"{"name":"web","tasks":{"t":{"exec":"echo","cache":{"enabled":true}},"u":"echo"}}"#,
        )],
    )
    .unwrap();
    let mut project = composed.layer.clone().finalize_composed().unwrap();
    attach_cache_enabled_overrides(
        &mut project,
        &composed.overrides,
        &composed.override_locations,
    );
    // `u` inherits the applied project value; `t` declares its own.
    let inherited = &project.task_cache_enabled_overridden["u"];
    assert_eq!(inherited.len(), 1);
    assert!(project.task_cache_enabled_overridden.get("t").is_none());

    // An unmasked Emitted record of the same field must not be attributed to
    // an Applied-tier decision; only the Applied record is.
    let emitted = omni_configurations::FieldOverride {
        root: project.dir.clone(),
        field: omni_configurations::FieldPath::ProjectCacheEnabled,
        tier: omni_configurations::OverrideTier::Emitted,
        winner: id("emitter"),
        overridden: vec![id("other-emitter")],
        masked_by_explicit: false,
    };
    let mut with_emitted = composed.layer.clone().finalize_composed().unwrap();
    attach_cache_enabled_overrides(&mut with_emitted, &[emitted.clone()], &[]);
    assert!(
        with_emitted
            .task_cache_enabled_overridden
            .get("u")
            .is_none(),
        "an Emitted record is not the decision of an Applied tier"
    );
    attach_cache_enabled_overrides(
        &mut with_emitted,
        &[composed.overrides[0].clone(), emitted],
        &composed.override_locations,
    );
    let ids: Vec<_> = with_emitted.task_cache_enabled_overridden["u"]
        .iter()
        .map(|o| o.contributor.as_str())
        .collect();
    assert_eq!(ids, ["a"], "only the Applied contributor is attached");
}

/// F07.3 limit, pinned: an explicit task that inherits `cache.enabled`
/// through `extends` is not seen by the mask, so an applied record says it
/// overrode, although the explicit value decided. F07.5 reports it as the
/// plan says; a real fix tracks the decision in finalization.
#[test]
fn an_inherited_explicit_decision_is_attributed_the_known_limit() {
    let composed = compose_ordered(
        OmniPath::new(Path::new("app")),
        &[id("a"), id("b")],
        vec![
            with_enabled(
                "a",
                EnabledScope::Task("t".into()),
                true,
                Some(&["a", "omni.yaml"]),
            ),
            with_enabled(
                "b",
                EnabledScope::Task("t".into()),
                false,
                Some(&["b", "omni.yaml"]),
            ),
        ],
        vec![explicit(
            r#"{"name":"web","tasks":{"base":{"exec":"echo","cache":{"enabled":true}},"t":{"exec":"echo","extends":["base"]}}}"#,
        )],
    )
    .unwrap();
    let mut project = composed.layer.clone().finalize_composed().unwrap();
    assert_eq!(
        project.task_cache_enabled["t"].source,
        omni_configurations::CacheEnabledSource::TaskExplicit
    );
    assert!(
        composed.overrides.iter().all(|o| !o.masked_by_explicit),
        "the mask does not see the inherited declaration"
    );
    attach_cache_enabled_overrides(
        &mut project,
        &composed.overrides,
        &composed.override_locations,
    );
    assert_eq!(
        project.task_cache_enabled_overridden["t"].len(),
        1,
        "the known limit: the contributors are listed although the explicit          inherited value decided"
    );
}
