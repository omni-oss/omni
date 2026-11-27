//! The one builder of a task's [`EffectiveTaskConfig`] (RFC 0024 F06.3).
//!
//! Every site that computes a cache key reads the digest of this value,
//! computed once per task from the finalized node, the **unexpanded**
//! [`CacheInfo`] and the task meta, so the executor, the dry run, the cache
//! store, stale pruning and the project hashers cannot disagree.

use std::collections::{BTreeMap, BTreeSet};

use omni_collector::effective_task::{
    EffectiveTaskConfig, EffectiveValue, Enabled, InputPolicy, OutputPolicy,
    PathRule,
};
use omni_config_types::TeraExprBoolean;
use omni_configurations::MetaConfiguration;
use omni_core::TaskExecutionNode;
use omni_glob::GlobPatterns;
use omni_types::{OmniPath, Root, TaskKey};

use crate::CacheInfo;

/// Builds the effective config of one finalized task.
///
/// - Dependencies and siblings are formatted with the injective
///   [`TaskKey::label`], never `format!("{project}#{task}")`.
/// - `cache_info` is `None` for a task with no cache information; its cache
///   rules are then the empty defaults (such a task never reaches a key).
/// - `args` is the **configured** set; the merged invocation overrides reach the
///   key through the execution info instead.
/// - The retry policy is the configured one; a policy that does not resolve
///   (the run fails before any key matters) digests as the default policy.
pub fn build(
    node: &TaskExecutionNode,
    cache_info: Option<&CacheInfo>,
    meta: Option<&MetaConfiguration>,
) -> EffectiveTaskConfig {
    let labels = |keys: &[TaskKey]| {
        keys.iter()
            .map(|k| k.label().into_string())
            .collect::<BTreeSet<_>>()
    };

    let mut config = EffectiveTaskConfig {
        command: node.task_exec_config().cloned(),
        retry_command: node.task_retry_exec_config().cloned(),
        enabled: match node.enabled() {
            TeraExprBoolean::Boolean(b) => Enabled::Bool(*b),
            TeraExprBoolean::Expr(expr) => {
                Enabled::Expression(expr.as_str().to_string())
            }
        },
        interactive: node.interactive(),
        persistent: node.persistent(),
        retry: node.retry().clone().into_policy().unwrap_or_default(),
        meta: meta_values(meta),
        dependencies: labels(node.dependencies()),
        siblings: labels(node.siblings()),
        ..Default::default()
    };

    if let Some(info) = cache_info {
        config.args = info
            .args
            .iter()
            .map(|(k, v)| (k.clone(), EffectiveValue::from(v.clone())))
            .collect();
        let layer = |m: &maps::Map<String, String>| {
            m.iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<BTreeMap<_, _>>()
        };
        config.env_workspace = layer(&info.env.workspace);
        config.env_project = layer(&info.env.project);
        config.env_task = layer(&info.env.task);
        config.input = InputPolicy {
            key_defaults: info.key_defaults,
            include: rules(&info.key_input_files),
            exclude: rules_exclude(&info.key_input_files),
            env_keys: info.key_env_keys.iter().cloned().collect(),
        };
        config.output = OutputPolicy {
            include: rules(&info.cache_output_files),
            exclude: rules_exclude(&info.cache_output_files),
            capture_logs: info.cache_logs,
        };
    }

    config
}

fn meta_values(
    meta: Option<&MetaConfiguration>,
) -> BTreeMap<String, EffectiveValue> {
    match meta.and_then(|m| serde_json::to_value(m).ok()) {
        Some(serde_json::Value::Object(map)) => map
            .into_iter()
            .map(|(k, v)| (k, EffectiveValue::from(v)))
            .collect(),
        _ => BTreeMap::new(),
    }
}

fn rules(patterns: &GlobPatterns<OmniPath>) -> Vec<PathRule> {
    patterns.include.iter().filter_map(rule).collect()
}

fn rules_exclude(patterns: &GlobPatterns<OmniPath>) -> Vec<PathRule> {
    patterns.exclude.iter().filter_map(rule).collect()
}

/// A path rule anchored at its root, in the portable `/` form. An unrooted
/// relative path is project-relative, like `@project/` (the collector resolves
/// it the same way). An absolute unrooted path (the config-file ids that
/// `cache.key.defaults` adds) is left out on purpose: it would tie the digest
/// to the checkout location, and the collector already hashes the content of
/// those files under their workspace-relative name.
fn rule(path: &OmniPath) -> Option<PathRule> {
    let root = match path.root() {
        Some(root) => root,
        None if path.unresolved_path().is_absolute() => return None,
        None => Root::Project,
    };
    let portable = path.to_portable();
    Some(PathRule {
        root,
        pattern: portable.unresolved_path().to_string_lossy().into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use omni_command_config::CommandConfig;
    use omni_hasher::impls::DefaultHash;

    use super::*;

    fn node_with(
        key: TaskKey,
        deps: Vec<TaskKey>,
        siblings: Vec<TaskKey>,
        enabled: TeraExprBoolean,
    ) -> TaskExecutionNode {
        TaskExecutionNode::new(
            key,
            Some(CommandConfig::Shell("echo".to_string())),
            None,
            PathBuf::from("/ws/p"),
            deps,
            enabled,
            false,
            false,
            Default::default(),
        )
        .with_siblings(siblings)
    }

    fn node() -> TaskExecutionNode {
        node_with(
            TaskKey::new("p", "t"),
            vec![],
            vec![],
            TeraExprBoolean::Boolean(true),
        )
    }

    fn info() -> CacheInfo {
        CacheInfo {
            cache_enabled: true,
            cache_enabled_source: Default::default(),
            cache_remote: true,
            key_defaults: true,
            key_env_keys: vec![],
            key_input_files: Default::default(),
            cache_output_files: Default::default(),
            cache_logs: false,
            args: Default::default(),
            env: Default::default(),
        }
    }

    fn digest(
        node: &TaskExecutionNode,
        info: &CacheInfo,
        meta: Option<&MetaConfiguration>,
    ) -> DefaultHash {
        build(node, Some(info), meta).digest()
    }

    #[test]
    fn it_is_deterministic() {
        assert_eq!(
            digest(&node(), &info(), None),
            digest(&node(), &info(), None)
        );
    }

    #[test]
    fn included_cache_fields_change_the_digest() {
        let base = digest(&node(), &info(), None);
        let mut variants: Vec<(&str, CacheInfo)> = vec![];
        let mut v = info();
        v.key_defaults = false;
        variants.push(("key_defaults", v));
        let mut v = info();
        v.key_env_keys = vec!["A".into()];
        variants.push(("key env keys", v));
        let mut v = info();
        v.cache_logs = true;
        variants.push(("capture logs", v));
        let mut v = info();
        v.args.insert("k".into(), serde_json::json!("v"));
        variants.push(("configured args", v));
        let mut v = info();
        v.env.workspace.insert("X".into(), "1".into());
        variants.push(("workspace env definitions", v));
        let mut v = info();
        v.env.project.insert("X".into(), "1".into());
        variants.push(("project env definitions", v));
        let mut v = info();
        v.env.task.insert("X".into(), "1".into());
        variants.push(("task env definitions", v));
        let mut v = info();
        v.key_input_files.include = vec![OmniPath::new("src/**")];
        variants.push(("input include", v));
        let mut v = info();
        v.key_input_files.exclude = vec![OmniPath::new("src/**")];
        variants.push(("input exclude", v));
        let mut v = info();
        v.cache_output_files.include = vec![OmniPath::new("dist/**")];
        variants.push(("output include", v));
        let mut v = info();
        v.cache_output_files.exclude = vec![OmniPath::new("dist/**")];
        variants.push(("output exclude", v));
        for (name, variant) in variants {
            assert_ne!(base, digest(&node(), &variant, None), "{name}");
        }
    }

    #[test]
    fn presentation_and_cache_enablement_do_not_change_the_digest() {
        let base = digest(&node(), &info(), None);
        let mut v = info();
        v.cache_enabled = false;
        v.cache_remote = false;
        v.cache_enabled_source =
            omni_configurations::CacheEnabledSource::TaskExplicit;
        assert_eq!(base, digest(&node(), &v, None));
    }

    #[test]
    fn included_node_fields_change_the_digest() {
        let base = digest(&node(), &info(), None);
        let other = [
            node_with(
                TaskKey::new("p", "t"),
                vec![TaskKey::new("p", "d")],
                vec![],
                TeraExprBoolean::Boolean(true),
            ),
            node_with(
                TaskKey::new("p", "t"),
                vec![],
                vec![TaskKey::new("p", "s")],
                TeraExprBoolean::Boolean(true),
            ),
            node_with(
                TaskKey::new("p", "t"),
                vec![],
                vec![],
                TeraExprBoolean::Boolean(false),
            ),
            node_with(
                TaskKey::new("p", "t"),
                vec![],
                vec![],
                TeraExprBoolean::Expr("{{ env.CI }}".into()),
            ),
        ];
        for n in &other {
            assert_ne!(base, digest(n, &info(), None));
        }
        // Dependencies and siblings are different fields.
        assert_ne!(
            digest(&other[0], &info(), None),
            digest(&other[1], &info(), None)
        );
    }

    #[test]
    fn a_boolean_and_an_expression_are_different_definitions() {
        let on = digest(&node(), &info(), None);
        let expr = node_with(
            TaskKey::new("p", "t"),
            vec![],
            vec![],
            TeraExprBoolean::Expr("true".into()),
        );
        assert_ne!(on, digest(&expr, &info(), None));
    }

    #[test]
    fn dependencies_and_siblings_use_the_injective_label() {
        let ab_c = node_with(
            TaskKey::new("p", "t"),
            vec![TaskKey::new("a#b", "c")],
            vec![],
            TeraExprBoolean::Boolean(true),
        );
        let a_bc = node_with(
            TaskKey::new("p", "t"),
            vec![TaskKey::new("a", "b#c")],
            vec![],
            TeraExprBoolean::Boolean(true),
        );
        assert_ne!(digest(&ab_c, &info(), None), digest(&a_bc, &info(), None));
        let config = build(&ab_c, Some(&info()), None);
        assert_eq!(
            config.dependencies.into_iter().collect::<Vec<_>>(),
            vec![TaskKey::new("a#b", "c").label().into_string()]
        );
        let sibling = node_with(
            TaskKey::new("p", "t"),
            vec![],
            vec![TaskKey::new("a#b", "c")],
            TeraExprBoolean::Boolean(true),
        );
        assert_eq!(
            build(&sibling, Some(&info()), None)
                .siblings
                .into_iter()
                .collect::<Vec<_>>(),
            vec![TaskKey::new("a#b", "c").label().into_string()]
        );
    }

    #[test]
    fn meta_reaches_the_digest() {
        let meta = |flavor: &str| {
            serde_json::from_value::<MetaConfiguration>(
                serde_json::json!({ "flavor": flavor }),
            )
            .unwrap()
        };
        let (one, two) = (meta("one"), meta("two"));
        let none = digest(&node(), &info(), None);
        let with_one = digest(&node(), &info(), Some(&one));
        assert_ne!(none, with_one);
        assert_ne!(with_one, digest(&node(), &info(), Some(&two)));
        assert_eq!(with_one, digest(&node(), &info(), Some(&meta("one"))));
    }

    #[test]
    fn env_definitions_are_raw_and_order_independent() {
        let mut a = info();
        a.env.project.insert("A".into(), "{{ x }}".into());
        a.env.project.insert("B".into(), "2".into());
        let mut b = info();
        b.env.project.insert("B".into(), "2".into());
        b.env.project.insert("A".into(), "{{ x }}".into());
        assert_eq!(digest(&node(), &a, None), digest(&node(), &b, None));
        let mut c = a.clone();
        c.env.project.insert("A".into(), "{{ y }}".into());
        assert_ne!(digest(&node(), &a, None), digest(&node(), &c, None));
    }

    #[test]
    fn the_three_env_layers_are_all_in_the_digest_and_unmerged() {
        let with =
            |ws: &[(&str, &str)], pr: &[(&str, &str)], t: &[(&str, &str)]| {
                let mut v = info();
                for (k, x) in ws {
                    v.env.workspace.insert((*k).into(), (*x).into());
                }
                for (k, x) in pr {
                    v.env.project.insert((*k).into(), (*x).into());
                }
                for (k, x) in t {
                    v.env.task.insert((*k).into(), (*x).into());
                }
                digest(&node(), &v, None)
            };
        let base =
            with(&[("X", "w")], &[("X", "p1"), ("Y", "${X}")], &[("T", "t")]);
        // Every layer's definitions reach the digest.
        assert_ne!(
            base,
            with(&[("X", "w2")], &[("X", "p1"), ("Y", "${X}")], &[("T", "t")])
        );
        assert_ne!(
            base,
            with(&[("X", "w")], &[("X", "p2"), ("Y", "${X}")], &[("T", "t")])
        );
        assert_ne!(
            base,
            with(&[("X", "w")], &[("X", "p1"), ("Y", "${X}")], &[("T", "t2")])
        );
        // A shadowed project value (workspace wins the merge) is still keyed.
        assert_ne!(
            with(&[("X", "w")], &[("X", "p1")], &[]),
            with(&[("X", "w")], &[("X", "p2")], &[])
        );
        // The same merged result with different attribution is a different
        // definition (`Y: ${X}` reads the project's `X`, not the workspace's).
        assert_ne!(
            with(&[("X", "v")], &[], &[]),
            with(&[], &[("X", "v")], &[])
        );
        assert_ne!(
            with(&[], &[("X", "v")], &[]),
            with(&[], &[], &[("X", "v")])
        );
        // An identical layering digests the same.
        assert_eq!(
            base,
            with(&[("X", "w")], &[("Y", "${X}"), ("X", "p1")], &[("T", "t")])
        );
    }

    #[test]
    fn a_relative_unrooted_rule_is_project_rooted_and_portable() {
        let with = |path: OmniPath| {
            let mut v = info();
            v.key_input_files.include = vec![path];
            digest(&node(), &v, None)
        };
        assert_eq!(
            with(OmniPath::new("src/**")),
            with(OmniPath::new_rooted("src/**", Root::Project))
        );
        assert_ne!(
            with(OmniPath::new_rooted("src/**", Root::Project)),
            with(OmniPath::new_rooted("src/**", Root::Workspace))
        );
    }

    #[test]
    fn an_absolute_unrooted_rule_does_not_tie_the_digest_to_a_checkout() {
        let abs = |dir: &str| {
            let mut v = info();
            v.key_input_files.include = vec![OmniPath::new(
                std::env::temp_dir().join(dir).join("project.omni.yaml"),
            )];
            digest(&node(), &v, None)
        };
        assert_eq!(abs("one"), abs("two"));
        assert_eq!(abs("one"), digest(&node(), &info(), None));
    }

    #[test]
    fn no_cache_info_digests_as_the_empty_defaults() {
        assert_eq!(
            build(&node(), None, None).digest(),
            build(
                &node(),
                Some(&CacheInfo {
                    key_defaults: false,
                    ..info()
                }),
                None
            )
            .digest()
        );
    }
}
