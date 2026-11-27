//! Where a task's effective `cache.enabled` came from (RFC 0024 F05.1).
//!
//! The value alone is not enough. Whether a dependent task may be silently run
//! uncached or must be rejected depends on *who* decided each side: two authored
//! settings that contradict each other are an error, while a default or a plugin
//! that disagrees with an authored setting is not (F05.2). This module records
//! the decision and its origin together; it changes no behavior and no cache key.
//!
//! Tiers, highest precedence first: a task's own setting (including one reached
//! through `extends`), the project's setting inherited by the task, a plugin's
//! emitted value, plugin activation `defaults`, and the host fallback. Until the
//! plugin work lands only the task, project and fallback tiers exist; the other
//! two are present so they slot in without reshaping this type.

use serde::{Deserialize, Serialize};

/// The tier that decided a task's `cache.enabled`, ordered from lowest to highest
/// precedence: `HostFallback < ActivationDefaults < Plugin < ProjectExplicit <
/// TaskExplicit`.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum CacheEnabledSource {
    /// Nothing authored or emitted set it; the host's rule applied.
    #[default]
    HostFallback,
    /// A plugin activation's `defaults`. Not produced yet.
    ActivationDefaults,
    /// A value a plugin emitted. Not produced yet.
    Plugin,
    /// The project's `cache.enabled`, inherited by the task.
    ProjectExplicit,
    /// The task's own `cache.enabled`, including one inherited through `extends`.
    TaskExplicit,
}

impl CacheEnabledSource {
    /// The kebab-case name used in serialized output.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HostFallback => "host-fallback",
            Self::ActivationDefaults => "activation-defaults",
            Self::Plugin => "plugin",
            Self::ProjectExplicit => "project-explicit",
            Self::TaskExplicit => "task-explicit",
        }
    }

    /// Whether a person wrote it in configuration. Only authored sources can
    /// make a contradiction an error; every other source demotes instead.
    pub fn is_authored(self) -> bool {
        match self {
            Self::ProjectExplicit | Self::TaskExplicit => true,
            Self::HostFallback | Self::ActivationDefaults | Self::Plugin => {
                false
            }
        }
    }
}

/// A task's effective `cache.enabled` and the tier that decided it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize,
)]
pub struct CacheEnabled {
    pub value: bool,
    pub source: CacheEnabledSource,
}

/// The host fallback for a task written in configuration: cache unless told
/// otherwise.
pub const CONFIGURED_TASK_CACHE_ENABLED_FALLBACK: bool = true;

/// The host fallback for a task a plugin inferred: do not cache unless something
/// says to. Unused until plugins can contribute tasks; callers resolving an
/// inferred task pass this instead of [`CONFIGURED_TASK_CACHE_ENABLED_FALLBACK`].
pub const INFERRED_TASK_CACHE_ENABLED_FALLBACK: bool = false;

impl CacheEnabled {
    /// The highest tier that has a value wins; with none, `host_fallback` applies.
    /// Candidates of different sources may come in any order. Of several
    /// candidates with the same source the last one wins, so callers pass at
    /// most one per source and resolve ties themselves (ordered contributors use
    /// `resolve_plugin_tier`).
    pub fn resolve(
        candidates: impl IntoIterator<Item = (CacheEnabledSource, Option<bool>)>,
        host_fallback: bool,
    ) -> Self {
        candidates
            .into_iter()
            .filter_map(|(source, value)| {
                value.map(|value| CacheEnabled { value, source })
            })
            .max_by_key(|decided| decided.source)
            .unwrap_or(CacheEnabled {
                value: host_fallback,
                source: CacheEnabledSource::HostFallback,
            })
    }

    /// The tiers that exist without plugins: the task's own setting, then the
    /// project's, then the host fallback.
    pub fn from_authored(
        task: Option<bool>,
        project: Option<bool>,
        host_fallback: bool,
    ) -> Self {
        Self::resolve(
            [
                (CacheEnabledSource::TaskExplicit, task),
                (CacheEnabledSource::ProjectExplicit, project),
            ],
            host_fallback,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use CacheEnabledSource::*;

    fn decided(value: bool, source: CacheEnabledSource) -> CacheEnabled {
        CacheEnabled { value, source }
    }

    #[test]
    fn as_str_matches_the_serialized_name() {
        for source in [
            HostFallback,
            ActivationDefaults,
            Plugin,
            ProjectExplicit,
            TaskExplicit,
        ] {
            assert_eq!(
                serde_json::to_value(source).unwrap(),
                serde_json::Value::String(source.as_str().into())
            );
        }
    }

    #[test]
    fn tiers_are_ordered_lowest_to_highest_precedence() {
        let ordered = [
            HostFallback,
            ActivationDefaults,
            Plugin,
            ProjectExplicit,
            TaskExplicit,
        ];
        assert!(ordered.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn only_authored_tiers_are_authored() {
        assert!(TaskExplicit.is_authored() && ProjectExplicit.is_authored());
        assert!(
            !Plugin.is_authored()
                && !ActivationDefaults.is_authored()
                && !HostFallback.is_authored()
        );
    }

    #[test]
    fn the_task_beats_the_project_beats_the_fallback() {
        let r = CacheEnabled::from_authored;
        assert_eq!(
            r(Some(false), Some(true), true),
            decided(false, TaskExplicit)
        );
        assert_eq!(
            r(Some(true), Some(false), true),
            decided(true, TaskExplicit)
        );
        assert_eq!(r(None, Some(false), true), decided(false, ProjectExplicit));
        assert_eq!(r(None, None, true), decided(true, HostFallback));
        assert_eq!(r(None, None, false), decided(false, HostFallback));
    }

    #[test]
    fn an_explicit_value_equal_to_the_fallback_is_still_explicit() {
        assert_eq!(
            CacheEnabled::from_authored(Some(true), None, true),
            decided(true, TaskExplicit)
        );
    }

    #[test]
    fn plugin_tiers_slot_between_the_project_and_the_fallback() {
        let all = |task, project, plugin, defaults| {
            CacheEnabled::resolve(
                [
                    (HostFallback, None),
                    (ActivationDefaults, defaults),
                    (Plugin, plugin),
                    (ProjectExplicit, project),
                    (TaskExplicit, task),
                ],
                true,
            )
        };
        // A plugin never overrides an authored setting, but beats the defaults.
        assert_eq!(
            all(None, Some(true), Some(false), Some(false)),
            decided(true, ProjectExplicit)
        );
        assert_eq!(
            all(None, None, Some(false), Some(true)),
            decided(false, Plugin)
        );
        assert_eq!(
            all(None, None, None, Some(false)),
            decided(false, ActivationDefaults)
        );
        assert_eq!(
            all(Some(true), None, Some(false), None),
            decided(true, TaskExplicit)
        );
    }

    #[test]
    fn candidate_order_does_not_matter() {
        let forward = CacheEnabled::resolve(
            [(Plugin, Some(true)), (TaskExplicit, Some(false))],
            true,
        );
        let backward = CacheEnabled::resolve(
            [(TaskExplicit, Some(false)), (Plugin, Some(true))],
            true,
        );
        assert_eq!(forward, backward);
        assert_eq!(forward, decided(false, TaskExplicit));
    }

    #[test]
    fn an_inferred_task_defaults_to_uncached_and_a_configured_one_to_cached() {
        let inferred = CacheEnabled::from_authored(
            None,
            None,
            INFERRED_TASK_CACHE_ENABLED_FALLBACK,
        );
        let configured = CacheEnabled::from_authored(
            None,
            None,
            CONFIGURED_TASK_CACHE_ENABLED_FALLBACK,
        );
        assert_eq!(inferred, decided(false, HostFallback));
        assert_eq!(configured, decided(true, HostFallback));
        // An authored setting still wins on an inferred task.
        assert_eq!(
            CacheEnabled::from_authored(
                Some(true),
                None,
                INFERRED_TASK_CACHE_ENABLED_FALLBACK
            ),
            decided(true, TaskExplicit)
        );
    }

    #[test]
    fn of_equal_sources_the_last_candidate_wins() {
        let r = |a, b| {
            CacheEnabled::resolve([(Plugin, Some(a)), (Plugin, Some(b))], true)
        };
        assert_eq!(r(true, false), decided(false, Plugin));
        assert_eq!(r(false, true), decided(true, Plugin));
    }

    #[test]
    fn serializes_with_kebab_case_sources() {
        let json =
            serde_json::to_string(&decided(false, ProjectExplicit)).unwrap();
        assert_eq!(json, r#"{"value":false,"source":"project-explicit"}"#);
    }
}
