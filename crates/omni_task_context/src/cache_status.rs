//! Typed cache eligibility of a task within one run.
//!
//! Derived from the current graph and configuration on every run: it is not
//! persisted and is never a cache-key input.

use std::collections::HashSet;

use omni_types::{TaskKey, TaskKeyRef};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::EnumIs;

/// Whether a task may read, replay or write execution-cache entries.
#[derive(
    Debug, Clone, PartialEq, Eq, EnumIs, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum CacheStatus {
    Eligible,
    /// Caching is disabled for the task. `deciding_source` names the tier that
    /// decided it (`task-explicit`, `project-explicit`, ...); `None` when the
    /// task never reached the origin-aware path (for example it was skipped).
    Disabled {
        deciding_source: Option<String>,
    },
    Persistent,
    NoCommand,
    /// `--no-cache`: lookup is allowed, nothing is persisted.
    WriteOnlyNoCache,
    /// Cache-enabled, but an execution dependency is not cache-eligible, so
    /// the task runs without lookup, replay or persistence.
    Demoted {
        direct_dependency: String,
        root_cause: String,
    },
}

impl CacheStatus {
    /// Whether a dependent task can build a cache key on this task's hash.
    pub fn provides_reusable_hash(&self) -> bool {
        self.is_eligible()
    }
}

/// What the task's own configuration says, before dependencies are considered.
#[derive(Debug, Clone, Copy)]
pub struct OwnFacts {
    pub has_command: bool,
    pub persistent: bool,
    pub cache_enabled: bool,
    pub cache_source: Option<omni_configurations::CacheEnabledSource>,
    pub no_cache: bool,
}

impl OwnFacts {
    pub fn of(context: &crate::TaskContext<'_>, no_cache: bool) -> Self {
        Self {
            has_command: context.node.task_exec_config().is_some(),
            persistent: context.node.persistent(),
            cache_enabled: context
                .cache_info
                .as_ref()
                .is_some_and(|info| info.cache_enabled),
            cache_source: context
                .cache_info
                .as_ref()
                .map(|info| info.cache_enabled_source),
            no_cache,
        }
    }
}

/// How a dependency counts toward its dependents' eligibility.
pub enum DependencyOutcome<'a> {
    /// A task that ran or was replayed, with its derived status.
    Status(&'a CacheStatus),
    /// A commandless or disabled node: judged by its own dependencies.
    Transparent(&'a [TaskKey]),
    /// The dependency errored or never ran, so there is no digest to key on.
    Unreusable,
    /// Not part of this plan; contributes nothing.
    Unknown,
}

pub fn derive_status<'a>(
    own: OwnFacts,
    dependencies: &'a [TaskKey],
    lookup: &impl for<'k> Fn(TaskKeyRef<'k>) -> DependencyOutcome<'a>,
) -> CacheStatus {
    if !own.has_command {
        return CacheStatus::NoCommand;
    }
    if own.persistent {
        return CacheStatus::Persistent;
    }
    if !own.cache_enabled {
        return CacheStatus::Disabled {
            deciding_source: own.cache_source.map(|s| s.as_str().to_string()),
        };
    }
    if let Some((direct, root)) =
        first_uncached_dependency(dependencies, lookup)
    {
        return CacheStatus::Demoted {
            direct_dependency: direct,
            root_cause: root,
        };
    }
    if own.no_cache {
        CacheStatus::WriteOnlyNoCache
    } else {
        CacheStatus::Eligible
    }
}

/// The first uncached dependency in declaration order, as
/// `(direct dependency, root cause)` display labels. Transparent nodes are
/// looked through; the walk is by identity and labels are rendered only for the
/// result. A demotion's own `root_cause` text is carried over as written.
fn first_uncached_dependency<'a>(
    dependencies: &'a [TaskKey],
    lookup: &impl for<'k> Fn(TaskKeyRef<'k>) -> DependencyOutcome<'a>,
) -> Option<(String, String)> {
    let mut seen: HashSet<TaskKeyRef<'a>> = HashSet::new();
    let mut stack = dependencies
        .iter()
        .rev()
        .map(|d| (d.as_ref(), d.as_ref()))
        .collect::<Vec<_>>();

    while let Some((direct, name)) = stack.pop() {
        if !seen.insert(name) {
            continue;
        }
        match lookup(name) {
            DependencyOutcome::Status(s) if s.provides_reusable_hash() => {}
            DependencyOutcome::Status(CacheStatus::Demoted {
                root_cause,
                ..
            }) => return Some((direct.to_string(), root_cause.clone())),
            DependencyOutcome::Status(_) => {
                return Some((direct.to_string(), name.to_string()));
            }
            DependencyOutcome::Transparent(deps) => {
                stack.extend(deps.iter().rev().map(|d| (direct, d.as_ref())));
            }
            DependencyOutcome::Unreusable => {
                return Some((direct.to_string(), name.to_string()));
            }
            DependencyOutcome::Unknown => {}
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use omni_types::TaskMap;

    use super::*;

    fn facts() -> OwnFacts {
        OwnFacts {
            has_command: true,
            persistent: false,
            cache_enabled: true,
            cache_source: None,
            no_cache: false,
        }
    }

    fn key(name: &str) -> TaskKey {
        TaskKey::new("p", name)
    }

    fn keys(v: &[&str]) -> Vec<TaskKey> {
        v.iter().map(|s| key(s)).collect()
    }

    fn label(name: &str) -> String {
        format!("p#{name}")
    }

    struct Graph {
        statuses: TaskMap<CacheStatus>,
        transparent: TaskMap<Vec<TaskKey>>,
    }

    impl Graph {
        fn outcome(&self, key: TaskKeyRef<'_>) -> DependencyOutcome<'_> {
            if let Some(s) = self.statuses.get_ref(key) {
                DependencyOutcome::Status(s)
            } else if let Some(d) = self.transparent.get_ref(key) {
                DependencyOutcome::Transparent(d)
            } else {
                DependencyOutcome::Unknown
            }
        }
    }

    fn graph(
        statuses: &[(&str, CacheStatus)],
        transparent: &[(&str, &[&str])],
    ) -> Graph {
        let mut g = Graph {
            statuses: TaskMap::new(),
            transparent: TaskMap::new(),
        };
        for (n, s) in statuses {
            g.statuses.insert("p", n, s.clone());
        }
        for (n, d) in transparent {
            g.transparent.insert("p", n, keys(d));
        }
        g
    }

    fn derive(own: OwnFacts, deps: &[&str], g: &Graph) -> CacheStatus {
        let deps = keys(deps);
        derive_status(own, &deps, &|n| g.outcome(n))
    }

    fn demoted(direct: &str, root: &str) -> CacheStatus {
        CacheStatus::Demoted {
            direct_dependency: label(direct),
            root_cause: label(root),
        }
    }

    #[test]
    fn own_facts_decide_before_dependencies() {
        let g = graph(&[], &[]);
        let mut f = facts();
        f.has_command = false;
        assert_eq!(derive(f, &[], &g), CacheStatus::NoCommand);
        let mut f = facts();
        f.persistent = true;
        assert_eq!(derive(f, &[], &g), CacheStatus::Persistent);
        let mut f = facts();
        f.cache_enabled = false;
        assert_eq!(
            derive(f, &[], &g),
            CacheStatus::Disabled {
                deciding_source: None
            }
        );
        f.cache_source =
            Some(omni_configurations::CacheEnabledSource::ProjectExplicit);
        assert_eq!(
            derive(f, &[], &g),
            CacheStatus::Disabled {
                deciding_source: Some("project-explicit".into())
            }
        );
        let mut f = facts();
        f.no_cache = true;
        assert_eq!(derive(f, &[], &g), CacheStatus::WriteOnlyNoCache);
        assert_eq!(derive(facts(), &[], &g), CacheStatus::Eligible);
    }

    #[test]
    fn uncached_dependency_demotes() {
        for dep in [
            CacheStatus::Disabled {
                deciding_source: None,
            },
            CacheStatus::Persistent,
            CacheStatus::WriteOnlyNoCache,
        ] {
            let g = graph(&[("a", dep)], &[]);
            assert_eq!(derive(facts(), &["a"], &g), demoted("a", "a"));
        }
    }

    #[test]
    fn demotion_propagates_along_a_chain_with_the_root_cause() {
        let g = graph(
            &[
                (
                    "a",
                    CacheStatus::Disabled {
                        deciding_source: None,
                    },
                ),
                ("b", demoted("a", "a")),
            ],
            &[],
        );
        assert_eq!(derive(facts(), &["b"], &g), demoted("b", "a"));
    }

    #[test]
    fn eligible_and_unknown_dependencies_do_not_demote() {
        let g = graph(&[("a", CacheStatus::Eligible)], &[]);
        assert_eq!(
            derive(facts(), &["a", "not-in-plan"], &g),
            CacheStatus::Eligible
        );
    }

    #[test]
    fn aggregate_nodes_are_transparent() {
        let g = graph(
            &[("c", CacheStatus::Persistent)],
            &[("agg", &["c"]), ("agg2", &["agg"])],
        );
        assert_eq!(derive(facts(), &["agg2"], &g), demoted("agg2", "c"));

        let g = graph(&[("c", CacheStatus::Eligible)], &[("agg", &["c"])]);
        assert_eq!(derive(facts(), &["agg"], &g), CacheStatus::Eligible);
    }

    #[test]
    fn errored_dependency_is_unreusable() {
        let deps = keys(&["a"]);
        let d =
            derive_status(facts(), &deps, &|_| DependencyOutcome::Unreusable);
        assert_eq!(d, demoted("a", "a"));
    }

    #[test]
    fn diamonds_are_visited_once() {
        let g = graph(
            &[("leaf", CacheStatus::Eligible)],
            &[("l", &["leaf"]), ("r", &["leaf"])],
        );
        assert_eq!(derive(facts(), &["l", "r"], &g), CacheStatus::Eligible);
    }

    #[test]
    fn formerly_colliding_pairs_demote_independently() {
        let ab = TaskKey::new("a#b", "c");
        let a = TaskKey::new("a", "b#c");
        let mut statuses = TaskMap::new();
        statuses.insert("a#b", "c", CacheStatus::Eligible);
        statuses.insert("a", "b#c", CacheStatus::Persistent);
        let lookup = |k: TaskKeyRef<'_>| match statuses.get_ref(k) {
            Some(s) => DependencyOutcome::Status(s),
            None => DependencyOutcome::Unknown,
        };
        assert_eq!(
            derive_status(facts(), std::slice::from_ref(&ab), &lookup),
            CacheStatus::Eligible
        );
        assert_eq!(
            derive_status(facts(), std::slice::from_ref(&a), &lookup),
            CacheStatus::Demoted {
                direct_dependency: r"a#b\#c".into(),
                root_cause: r"a#b\#c".into(),
            }
        );
    }
}
