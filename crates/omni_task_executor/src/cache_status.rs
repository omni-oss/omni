//! Typed cache eligibility of a task within one run.
//!
//! Derived from the current graph and configuration on every run: it is not
//! persisted and is never a cache-key input.

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
    /// Caching is disabled for the task. `deciding_source` is reserved for
    /// origin-aware tiers; it is `None` until configuration can supply it.
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
pub(crate) struct OwnFacts {
    pub has_command: bool,
    pub persistent: bool,
    pub cache_enabled: bool,
    pub no_cache: bool,
}

/// How a dependency counts toward its dependents' eligibility.
pub(crate) enum DependencyOutcome<'a> {
    /// A task that ran or was replayed, with its derived status.
    Status(&'a CacheStatus),
    /// A commandless or disabled node: judged by its own dependencies.
    Transparent(&'a [String]),
    /// Not part of this plan, or never ran; contributes nothing.
    Unknown,
}

pub(crate) fn derive_status<'a>(
    own: OwnFacts,
    dependencies: &[String],
    lookup: &impl Fn(&str) -> DependencyOutcome<'a>,
) -> CacheStatus {
    if !own.has_command {
        return CacheStatus::NoCommand;
    }
    if own.persistent {
        return CacheStatus::Persistent;
    }
    if !own.cache_enabled {
        return CacheStatus::Disabled {
            deciding_source: None,
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
/// `(direct dependency, root cause)`. Transparent nodes are looked through.
fn first_uncached_dependency<'a>(
    dependencies: &[String],
    lookup: &impl Fn(&str) -> DependencyOutcome<'a>,
) -> Option<(String, String)> {
    let mut seen = std::collections::HashSet::new();
    let mut stack = dependencies
        .iter()
        .rev()
        .map(|d| (d.as_str(), d.as_str()))
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
                stack.extend(deps.iter().rev().map(|d| (direct, d.as_str())));
            }
            DependencyOutcome::Unknown => {}
        }
    }

    None
}

#[cfg(test)]
mod tests {

    use maps::UnorderedMap;

    use super::*;

    fn facts() -> OwnFacts {
        OwnFacts {
            has_command: true,
            persistent: false,
            cache_enabled: true,
            no_cache: false,
        }
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    struct Graph {
        statuses: UnorderedMap<String, CacheStatus>,
        transparent: UnorderedMap<String, Vec<String>>,
    }

    impl Graph {
        fn outcome(&self, name: &str) -> DependencyOutcome<'_> {
            if let Some(s) = self.statuses.get(name) {
                DependencyOutcome::Status(s)
            } else if let Some(d) = self.transparent.get(name) {
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
        Graph {
            statuses: statuses
                .iter()
                .map(|(n, s)| (n.to_string(), s.clone()))
                .collect(),
            transparent: transparent
                .iter()
                .map(|(n, d)| (n.to_string(), names(d)))
                .collect(),
        }
    }

    fn derive(own: OwnFacts, deps: &[&str], g: &Graph) -> CacheStatus {
        derive_status(own, &names(deps), &|n| g.outcome(n))
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
            assert_eq!(
                derive(facts(), &["a"], &g),
                CacheStatus::Demoted {
                    direct_dependency: "a".into(),
                    root_cause: "a".into()
                }
            );
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
                (
                    "b",
                    CacheStatus::Demoted {
                        direct_dependency: "a".into(),
                        root_cause: "a".into(),
                    },
                ),
            ],
            &[],
        );
        assert_eq!(
            derive(facts(), &["b"], &g),
            CacheStatus::Demoted {
                direct_dependency: "b".into(),
                root_cause: "a".into()
            }
        );
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
        assert_eq!(
            derive(facts(), &["agg2"], &g),
            CacheStatus::Demoted {
                direct_dependency: "agg2".into(),
                root_cause: "c".into()
            }
        );

        let g = graph(&[("c", CacheStatus::Eligible)], &[("agg", &["c"])]);
        assert_eq!(derive(facts(), &["agg"], &g), CacheStatus::Eligible);
    }

    #[test]
    fn diamonds_are_visited_once() {
        let g = graph(
            &[("leaf", CacheStatus::Eligible)],
            &[("l", &["leaf"]), ("r", &["leaf"])],
        );
        assert_eq!(derive(facts(), &["l", "r"], &g), CacheStatus::Eligible);
    }
}
