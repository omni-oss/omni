//! Read-only cache-eligibility inspection (RFC 0024 F09).
//!
//! Reports, from configuration alone, each planned task's typed cache status and
//! every explicit `cache.enabled` contradiction. Nothing runs: no command, no
//! `enabled` expression, no task environment, no cache lookup. Statuses come
//! from the same `derive_status` the executor uses and apply its check order
//! (no command, then disabled). They can differ where a template `enabled` condition would change the outcome at
//! execution time (listed in `unevaluated_conditions`) and for `--no-cache`,
//! which the inspection does not model.

use std::collections::HashMap;
use std::path::PathBuf;

use omni_configurations::CacheEnabledSource;
use omni_context::LoadedContext;
use omni_execution_plan::{ExecutionPlanProvider, MetaErrorOptions};
use omni_task_context::{
    StaticEnabled,
    cache_status::{CacheStatus, DependencyOutcome, OwnFacts, derive_status},
    classify_enabled,
};
use serde::{Deserialize, Serialize};

use crate::{
    CacheContradiction, ContradictionNode, ContradictionRole, ExecutionConfig,
    TaskExecutorError, TaskExecutorSys,
    execution_plan_provider::ContextExecutionPlanProvider,
    find_explicit_contradictions,
};

/// One task of the planned graph, as configuration describes it.
#[derive(Debug, Clone)]
pub struct InspectionNode {
    pub name: String,
    pub has_command: bool,
    pub persistent: bool,
    pub enabled: StaticEnabled,
    pub cache_enabled: bool,
    pub cache_source: CacheEnabledSource,
    pub dependencies: Vec<String>,
    pub cache_enabled_file: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectedTask {
    pub task: String,
    #[serde(flatten)]
    pub status: CacheStatus,
    /// The tier that decided `cache.enabled`, kebab-case.
    pub cache_enabled_source: String,
    /// `enabled` is a template expression only known at execution time, so the
    /// task may be skipped there and the status here is as-if enabled.
    pub condition_unevaluated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectedContradiction {
    pub dependent: String,
    pub dependent_source: String,
    pub dependent_file: Option<PathBuf>,
    pub dependency: String,
    pub dependency_source: String,
    pub dependency_file: Option<PathBuf>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CacheInspection {
    pub tasks: Vec<InspectedTask>,
    pub contradictions: Vec<InspectedContradiction>,
    /// Tasks whose `enabled` condition was not evaluated.
    pub unevaluated_conditions: Vec<String>,
}

/// Derive the inspection from nodes in dependency order.
pub fn inspect_nodes(
    nodes: &[InspectionNode],
    no_cache: bool,
    ignore_dependencies: bool,
) -> CacheInspection {
    let mut statuses: HashMap<String, CacheStatus> = HashMap::new();
    let mut transparent: HashMap<String, Vec<String>> = HashMap::new();
    let mut tasks = Vec::with_capacity(nodes.len());

    for node in nodes {
        let deps: &[String] = if ignore_dependencies {
            &[]
        } else {
            &node.dependencies
        };
        let status =
            if node.has_command && node.enabled == StaticEnabled::Disabled {
                CacheStatus::Disabled {
                    deciding_source: None,
                }
            } else {
                let own = OwnFacts {
                    has_command: node.has_command,
                    persistent: node.persistent,
                    cache_enabled: node.cache_enabled,
                    cache_source: Some(node.cache_source),
                    no_cache,
                };
                let lookup = |name: &str| {
                    if let Some(s) = statuses.get(name) {
                        DependencyOutcome::Status(s)
                    } else if let Some(d) = transparent.get(name) {
                        DependencyOutcome::Transparent(d)
                    } else {
                        DependencyOutcome::Unknown
                    }
                };
                derive_status(own, deps, &lookup)
            };

        if !node.has_command || node.enabled == StaticEnabled::Disabled {
            transparent.insert(node.name.clone(), deps.to_vec());
        } else {
            statuses.insert(node.name.clone(), status.clone());
        }

        tasks.push(InspectedTask {
            task: node.name.clone(),
            status,
            cache_enabled_source: node.cache_source.as_str().to_string(),
            condition_unevaluated: node.enabled == StaticEnabled::Conditional,
        });
    }

    let contradiction_nodes = nodes
        .iter()
        .map(|n| ContradictionNode {
            name: n.name.clone(),
            role: if !n.has_command || n.enabled == StaticEnabled::Disabled {
                ContradictionRole::Transparent
            } else if n.persistent {
                ContradictionRole::Persistent
            } else {
                ContradictionRole::Cacheable {
                    enabled: n.cache_enabled,
                    source: n.cache_source,
                }
            },
            dependencies: if ignore_dependencies {
                vec![]
            } else {
                n.dependencies.clone()
            },
        })
        .collect::<Vec<_>>();
    let files: HashMap<&str, &Option<PathBuf>> = nodes
        .iter()
        .map(|n| (n.name.as_str(), &n.cache_enabled_file))
        .collect();

    let contradictions = find_explicit_contradictions(&contradiction_nodes)
        .into_iter()
        .map(|c: CacheContradiction| InspectedContradiction {
            dependent_file: files
                .get(c.dependent.as_str())
                .and_then(|f| (*f).clone()),
            dependency_file: files
                .get(c.dependency.as_str())
                .and_then(|f| (*f).clone()),
            message: c.to_string(),
            dependent_source: c.dependent_source.as_str().to_string(),
            dependency_source: c.dependency_source.as_str().to_string(),
            dependent: c.dependent,
            dependency: c.dependency,
        })
        .collect();

    let unevaluated_conditions = tasks
        .iter()
        .filter(|t| t.condition_unevaluated)
        .map(|t| t.task.clone())
        .collect();

    CacheInspection {
        tasks,
        contradictions,
        unevaluated_conditions,
    }
}

/// Plan `config`'s selection and inspect it without running anything.
pub fn inspect_cache_eligibility<TSys: TaskExecutorSys>(
    context: &LoadedContext<TSys>,
    config: &ExecutionConfig,
) -> Result<CacheInspection, TaskExecutorError> {
    let provider = ContextExecutionPlanProvider::new(context)
        .with_meta_error_options(MetaErrorOptions {
            on_error: config.meta_on_error(),
            limits: omni_execution_plan::MetaFilterErrorLimits::with_samples(
                config.meta_error_samples(),
            ),
        });
    let plan = provider.get_execution_plan(
        config.call(),
        config.project_filters().as_slice(),
        config
            .dir_filters()
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .as_slice(),
        config.meta_filter().as_deref(),
        config.scm_affected_filter().as_ref(),
        config.ignore_dependencies(),
        config.with_dependents(),
    )?;

    let nodes = plan
        .iter()
        .flatten()
        .map(|node| {
            let info =
                context.get_cache_info(node.project_name(), node.task_name());
            let source =
                info.map(|i| i.cache_enabled_source).unwrap_or_default();
            InspectionNode {
                name: node.full_task_name().to_string(),
                has_command: node.task_exec_config().is_some(),
                persistent: node.persistent(),
                enabled: classify_enabled(node.enabled()),
                cache_enabled: info.is_some_and(|i| i.cache_enabled),
                cache_source: source,
                dependencies: node.dependencies().to_vec(),
                cache_enabled_file: context
                    .cache_enabled_file(node.project_name(), node.task_name())
                    .map(|p| {
                        p.strip_prefix(context.root_dir())
                            .unwrap_or(p)
                            .to_path_buf()
                    }),
            }
        })
        .collect::<Vec<_>>();

    Ok(inspect_nodes(&nodes, false, config.ignore_dependencies()))
}

#[cfg(test)]
mod tests {
    use CacheEnabledSource::*;

    use super::*;

    fn node(
        name: &str,
        enabled: bool,
        source: CacheEnabledSource,
        deps: &[&str],
    ) -> InspectionNode {
        InspectionNode {
            name: name.into(),
            has_command: true,
            persistent: false,
            enabled: StaticEnabled::Enabled,
            cache_enabled: enabled,
            cache_source: source,
            dependencies: deps.iter().map(|s| s.to_string()).collect(),
            cache_enabled_file: Some(PathBuf::from(format!("{name}.yaml"))),
        }
    }

    fn status<'a>(i: &'a CacheInspection, t: &str) -> &'a CacheStatus {
        &i.tasks.iter().find(|x| x.task == t).unwrap().status
    }

    #[test]
    fn reports_every_status_variant_without_running() {
        let mut nocmd = node("nocmd", true, HostFallback, &[]);
        nocmd.has_command = false;
        let mut persistent = node("persist", true, HostFallback, &[]);
        persistent.persistent = true;
        let mut off = node("off", true, HostFallback, &[]);
        off.enabled = StaticEnabled::Disabled;
        let mut nocmd_off = node("nocmd_off", true, HostFallback, &[]);
        nocmd_off.has_command = false;
        nocmd_off.enabled = StaticEnabled::Disabled;
        let nodes = [
            nocmd_off,
            nocmd,
            persistent,
            off,
            node("eligible", true, HostFallback, &[]),
            node("uncached", false, TaskExplicit, &[]),
            node("demoted", true, HostFallback, &["uncached"]),
        ];
        let i = inspect_nodes(&nodes, false, false);
        assert_eq!(status(&i, "nocmd"), &CacheStatus::NoCommand);
        assert_eq!(status(&i, "nocmd_off"), &CacheStatus::NoCommand);
        assert_eq!(status(&i, "persist"), &CacheStatus::Persistent);
        assert_eq!(
            status(&i, "off"),
            &CacheStatus::Disabled {
                deciding_source: None
            }
        );
        assert_eq!(status(&i, "eligible"), &CacheStatus::Eligible);
        assert_eq!(
            status(&i, "uncached"),
            &CacheStatus::Disabled {
                deciding_source: Some("task-explicit".into())
            }
        );
        assert_eq!(
            status(&i, "demoted"),
            &CacheStatus::Demoted {
                direct_dependency: "uncached".into(),
                root_cause: "uncached".into()
            }
        );
        assert!(i.contradictions.is_empty());
    }

    #[test]
    fn no_cache_gives_write_only_status() {
        let i =
            inspect_nodes(&[node("a", true, HostFallback, &[])], true, false);
        assert_eq!(status(&i, "a"), &CacheStatus::WriteOnlyNoCache);
    }

    #[test]
    fn lists_authored_contradiction_with_both_files_and_does_not_fail() {
        let nodes = [
            node("dep", false, TaskExplicit, &[]),
            node("app", true, ProjectExplicit, &["dep"]),
        ];
        let i = inspect_nodes(&nodes, false, false);
        assert_eq!(i.contradictions.len(), 1);
        let c = &i.contradictions[0];
        assert_eq!(c.dependent, "app");
        assert_eq!(c.dependent_file, Some(PathBuf::from("app.yaml")));
        assert_eq!(c.dependency_file, Some(PathBuf::from("dep.yaml")));
        assert_eq!(c.dependency_source, "task-explicit");
        assert!(matches!(status(&i, "app"), CacheStatus::Demoted { .. }));
    }

    #[test]
    fn non_authored_side_is_demotion_not_contradiction() {
        let nodes = [
            node("dep", false, TaskExplicit, &[]),
            node("app", true, HostFallback, &["dep"]),
        ];
        assert!(
            inspect_nodes(&nodes, false, false)
                .contradictions
                .is_empty()
        );
    }

    #[test]
    fn definitely_false_dependency_is_transparent_and_conditions_are_flagged() {
        let mut off = node("off", false, TaskExplicit, &[]);
        off.enabled = StaticEnabled::Disabled;
        let mut cond = node("cond", true, TaskExplicit, &["off"]);
        cond.enabled = StaticEnabled::Conditional;
        let i = inspect_nodes(&[off, cond], false, false);
        assert!(i.contradictions.is_empty());
        assert_eq!(status(&i, "cond"), &CacheStatus::Eligible);
        assert_eq!(i.unevaluated_conditions, vec!["cond".to_string()]);
    }

    #[test]
    fn conditional_dependency_counts_as_executable() {
        let mut dep = node("dep", false, TaskExplicit, &[]);
        dep.enabled = StaticEnabled::Conditional;
        let app = node("app", true, TaskExplicit, &["dep"]);
        let i = inspect_nodes(&[dep, app], false, false);
        assert_eq!(i.contradictions.len(), 1);
    }

    #[test]
    fn ignore_dependencies_drops_edges() {
        let nodes = [
            node("dep", false, TaskExplicit, &[]),
            node("app", true, TaskExplicit, &["dep"]),
        ];
        let i = inspect_nodes(&nodes, false, true);
        assert!(i.contradictions.is_empty());
        assert_eq!(status(&i, "app"), &CacheStatus::Eligible);
    }

    #[test]
    fn serializes_with_stable_field_names() {
        let i =
            inspect_nodes(&[node("a", true, HostFallback, &[])], false, false);
        let v = serde_json::to_value(&i.tasks[0]).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "task": "a",
                "status": "eligible",
                "cache_enabled_source": "host-fallback",
                "condition_unevaluated": false
            })
        );
    }
}
