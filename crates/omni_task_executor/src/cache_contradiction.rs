//! Explicit cache-enablement contradictions between dependent tasks (RFC 0024 F05.2).
//!
//! A cache-enabled task whose executable dependency is not cached is normally
//! demoted and runs uncached. That is only silent when a default, a plugin or
//! the host decided one side. When a person wrote *both* sides, the
//! configuration contradicts itself, and the planner should say so instead.
//! This module finds those pairs. It only reports; the executor decides what to do
//! with them, and nothing here changes a cache key.

use std::collections::HashSet;
use std::fmt;

use omni_configurations::CacheEnabledSource;

/// What a task contributes to the contradiction check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContradictionRole {
    /// No command, or the task is disabled: judged by its own dependencies.
    Transparent,
    /// Never cached whatever its setting; contradicts nothing.
    Persistent,
    /// Has a command and an effective `cache.enabled` decided by `source`.
    Cacheable {
        enabled: bool,
        source: CacheEnabledSource,
    },
}

/// One task of the planned graph.
#[derive(Debug, Clone)]
pub struct ContradictionNode {
    pub name: String,
    pub role: ContradictionRole,
    /// Direct dependencies, in declaration order.
    pub dependencies: Vec<String>,
}

/// An authored `cache.enabled: true` task that depends on an authored
/// `cache.enabled: false` executable task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheContradiction {
    pub dependent: String,
    pub dependent_source: CacheEnabledSource,
    pub dependency: String,
    pub dependency_source: CacheEnabledSource,
}

impl fmt::Display for CacheContradiction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "task '{}' sets cache.enabled to true ({}) but depends on '{}', whose cache.enabled is false ({}); a cached task cannot depend on an explicitly uncached one",
            self.dependent,
            describe(self.dependent_source),
            self.dependency,
            describe(self.dependency_source),
        )
    }
}

fn describe(source: CacheEnabledSource) -> &'static str {
    match source {
        CacheEnabledSource::TaskExplicit => "set on the task",
        CacheEnabledSource::ProjectExplicit => "set on the project",
        CacheEnabledSource::Plugin => "set by a plugin",
        CacheEnabledSource::ActivationDefaults => {
            "set by a plugin activation's defaults"
        }
        CacheEnabledSource::HostFallback => "the host default",
    }
}

/// Every explicit contradiction on an executable edge, in node order and then
/// dependency declaration order.
///
/// Only edges are judged: a dependent is checked against the executable tasks it
/// reaches directly or through transparent nodes. A dependency that is itself
/// demoted because something further down is uncached is not a contradiction
/// with its own dependent; it is the ordinary demotion case.
pub fn find_explicit_contradictions(
    nodes: &[ContradictionNode],
) -> Vec<CacheContradiction> {
    let by_name: std::collections::HashMap<&str, &ContradictionNode> =
        nodes.iter().map(|n| (n.name.as_str(), n)).collect();
    let mut found = Vec::new();

    for node in nodes {
        let ContradictionRole::Cacheable {
            enabled: true,
            source: dependent_source,
        } = node.role
        else {
            continue;
        };
        if !dependent_source.is_authored() {
            continue;
        }

        let mut seen = HashSet::new();
        let mut stack: Vec<&str> =
            node.dependencies.iter().rev().map(String::as_str).collect();
        while let Some(name) = stack.pop() {
            if !seen.insert(name) {
                continue;
            }
            let Some(dep) = by_name.get(name) else {
                continue;
            };
            match dep.role {
                ContradictionRole::Transparent => {
                    stack.extend(
                        dep.dependencies.iter().rev().map(String::as_str),
                    );
                }
                ContradictionRole::Persistent => {}
                ContradictionRole::Cacheable {
                    enabled: false,
                    source,
                } if source.is_authored() => {
                    found.push(CacheContradiction {
                        dependent: node.name.clone(),
                        dependent_source,
                        dependency: dep.name.clone(),
                        dependency_source: source,
                    });
                }
                ContradictionRole::Cacheable { .. } => {}
            }
        }
    }

    found
}

#[cfg(test)]
mod tests {
    use CacheEnabledSource::*;

    use super::*;

    fn cacheable(
        name: &str,
        enabled: bool,
        source: CacheEnabledSource,
        deps: &[&str],
    ) -> ContradictionNode {
        ContradictionNode {
            name: name.into(),
            role: ContradictionRole::Cacheable { enabled, source },
            dependencies: deps.iter().map(|d| d.to_string()).collect(),
        }
    }

    fn transparent(name: &str, deps: &[&str]) -> ContradictionNode {
        ContradictionNode {
            name: name.into(),
            role: ContradictionRole::Transparent,
            dependencies: deps.iter().map(|d| d.to_string()).collect(),
        }
    }

    fn pair(
        dependent: CacheEnabledSource,
        dependency: CacheEnabledSource,
    ) -> usize {
        find_explicit_contradictions(&[
            cacheable("app#build", true, dependent, &["lib#gen"]),
            cacheable("lib#gen", false, dependency, &[]),
        ])
        .len()
    }

    #[test]
    fn both_sides_authored_is_a_contradiction_for_every_authored_pairing() {
        for dependent in [TaskExplicit, ProjectExplicit] {
            for dependency in [TaskExplicit, ProjectExplicit] {
                assert_eq!(
                    pair(dependent, dependency),
                    1,
                    "{dependent:?} {dependency:?}"
                );
            }
        }
    }

    #[test]
    fn any_non_authored_side_demotes_instead() {
        let non_authored = [HostFallback, ActivationDefaults, Plugin];
        for other in non_authored {
            assert_eq!(pair(TaskExplicit, other), 0, "dependency {other:?}");
            assert_eq!(pair(other, TaskExplicit), 0, "dependent {other:?}");
            assert_eq!(pair(other, other), 0);
        }
    }

    #[test]
    fn a_disabled_dependent_or_an_enabled_dependency_is_not_a_contradiction() {
        assert!(
            find_explicit_contradictions(&[
                cacheable("a", false, TaskExplicit, &["b"]),
                cacheable("b", false, TaskExplicit, &[]),
            ])
            .is_empty()
        );
        assert!(
            find_explicit_contradictions(&[
                cacheable("a", true, TaskExplicit, &["b"]),
                cacheable("b", true, TaskExplicit, &[]),
            ])
            .is_empty()
        );
    }

    #[test]
    fn transparent_nodes_are_looked_through() {
        let found = find_explicit_contradictions(&[
            cacheable("a", true, TaskExplicit, &["agg"]),
            transparent("agg", &["agg2"]),
            transparent("agg2", &["c"]),
            cacheable("c", false, ProjectExplicit, &[]),
        ]);
        assert_eq!(
            found,
            vec![CacheContradiction {
                dependent: "a".into(),
                dependent_source: TaskExplicit,
                dependency: "c".into(),
                dependency_source: ProjectExplicit,
            }]
        );
    }

    #[test]
    fn only_the_executable_edge_is_judged_not_the_chain_below_it() {
        // a (authored true) -> b (fallback true, demoted by c) -> c (authored false)
        let found = find_explicit_contradictions(&[
            cacheable("a", true, TaskExplicit, &["b"]),
            cacheable("b", true, HostFallback, &["c"]),
            cacheable("c", false, TaskExplicit, &[]),
        ]);
        assert!(found.is_empty());
    }

    #[test]
    fn persistent_tasks_never_contradict_and_block_the_walk() {
        let found = find_explicit_contradictions(&[
            cacheable("a", true, TaskExplicit, &["p"]),
            ContradictionNode {
                name: "p".into(),
                role: ContradictionRole::Persistent,
                dependencies: vec!["c".into()],
            },
            cacheable("c", false, TaskExplicit, &[]),
        ]);
        assert!(found.is_empty());
    }

    #[test]
    fn every_pair_is_reported_once_in_deterministic_order() {
        let found = find_explicit_contradictions(&[
            cacheable("a", true, TaskExplicit, &["x", "agg", "y"]),
            transparent("agg", &["y", "z"]),
            cacheable("x", false, TaskExplicit, &[]),
            cacheable("y", false, TaskExplicit, &[]),
            cacheable("z", false, ProjectExplicit, &[]),
            cacheable("b", true, ProjectExplicit, &["z"]),
        ]);
        let pairs: Vec<_> = found
            .iter()
            .map(|c| (c.dependent.as_str(), c.dependency.as_str()))
            .collect();
        assert_eq!(pairs, [("a", "x"), ("a", "y"), ("a", "z"), ("b", "z")]);
    }

    #[test]
    fn unknown_dependencies_and_cycles_do_not_hang_or_report() {
        let found = find_explicit_contradictions(&[
            cacheable("a", true, TaskExplicit, &["ghost", "t"]),
            transparent("t", &["u"]),
            transparent("u", &["t"]),
        ]);
        assert!(found.is_empty());
    }

    #[test]
    fn the_message_names_both_tasks_and_who_decided_each() {
        let message = CacheContradiction {
            dependent: "app#build".into(),
            dependent_source: TaskExplicit,
            dependency: "lib#gen".into(),
            dependency_source: ProjectExplicit,
        }
        .to_string();
        assert!(message.contains("app#build"));
        assert!(message.contains("lib#gen"));
        assert!(message.contains("set on the task"));
        assert!(message.contains("set on the project"));
    }
}
