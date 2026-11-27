//! Which earlier contributors a later one overrode (RFC 0024 F07.3), as
//! structured data for the notice (F07.4) and inspection (F07.5). Records hold
//! contributor ids and field paths only, never values.

use std::collections::{BTreeMap, BTreeSet};

use config_utils::{DictConfig, DynValue};
use omni_task_output_logs::OutputLogsSplit;
use omni_types::OmniPath;

use crate::{
    Contribution, ContributorId, EnabledScope, LoadedLayer, ProjectLayer,
};

/// The tracked fields. Closed: other fields compose by replay without records.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FieldPath {
    Description,
    EnvVar(String),
    Meta(Vec<String>),
    ProjectCacheEnabled,
    TaskCacheEnabled(String),
    CacheKeyDefaults,
    OutputLogs,
}

/// Which declaration tier a record compares. Only `cache.enabled` has an applied
/// tier; every other field is `Emitted`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OverrideTier {
    Emitted,
    Applied,
}

/// A later contributor's value for `field` that differs from earlier ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldOverride {
    /// The project root the contributors composed at.
    pub root: OmniPath,
    pub field: FieldPath,
    pub tier: OverrideTier,
    pub winner: ContributorId,
    /// Earlier contributors whose value differs from the winning one, in order.
    pub overridden: Vec<ContributorId>,
    /// An explicit layer (or a higher authored tier for `cache.enabled`)
    /// decides the field regardless, so the override changes nothing.
    pub masked_by_explicit: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum Leaf {
    Str(String),
    Bool(bool),
    Dyn(DynValue),
    /// A dictionary or list node inside `meta`: lets a type change be seen.
    DictNode,
    ListNode,
    Logs(OutputLogsSplit),
}

impl Leaf {
    /// Whether this value is overridden by the effective one.
    fn differs_from(&self, effective: &Leaf) -> bool {
        match (self, effective) {
            (Leaf::Logs(mine), Leaf::Logs(effective)) => {
                [(mine.new, effective.new), (mine.cached, effective.cached)]
                    .into_iter()
                    .any(|(mine, effective)| {
                        mine.is_some()
                            && effective.is_some()
                            && mine != effective
                    })
            }
            _ => self != effective,
        }
    }
}

fn meta_leaves(
    out: &mut Vec<(FieldPath, Leaf)>,
    path: &mut Vec<String>,
    dict: &DictConfig<DynValue>,
) {
    for (key, value) in dict.as_map() {
        path.push(key.clone());
        match value {
            DynValue::Dict(inner) => {
                out.push((FieldPath::Meta(path.clone()), Leaf::DictNode));
                meta_leaves(out, path, inner);
            }
            DynValue::List(_) => {
                out.push((FieldPath::Meta(path.clone()), Leaf::ListNode));
            }
            other => {
                out.push((
                    FieldPath::Meta(path.clone()),
                    Leaf::Dyn(other.clone()),
                ));
            }
        }
        path.pop();
    }
}

fn leaves(layer: &ProjectLayer) -> Vec<(FieldPath, Leaf)> {
    let mut out = Vec::new();
    if let Some(description) = &layer.description {
        out.push((
            FieldPath::Description,
            Leaf::Str(description.as_ref().clone()),
        ));
    }
    if let Some(vars) = layer.env.as_ref().and_then(|e| e.vars.as_ref()) {
        for (key, value) in vars.as_map() {
            out.push((
                FieldPath::EnvVar(key.clone()),
                Leaf::Str(value.as_ref().clone()),
            ));
        }
    }
    if let Some(meta) = &layer.meta {
        meta_leaves(&mut out, &mut Vec::new(), &meta.0);
    }
    if let Some(defaults) = layer
        .cache
        .as_ref()
        .and_then(|c| c.key.as_ref())
        .and_then(|k| k.defaults)
    {
        out.push((FieldPath::CacheKeyDefaults, Leaf::Bool(defaults)));
    }
    if let Some(logs) = &layer.output_logs {
        out.push((FieldPath::OutputLogs, Leaf::Logs(logs.to_split())));
    }
    out
}

/// What the explicit layers (and applied declarations) decide regardless of
/// contributor order.
#[derive(Default)]
struct Masks {
    description: bool,
    env_replaced: bool,
    env_keys: BTreeSet<String>,
    meta_replaced: bool,
    /// Paths an explicit layer sets to a non-dictionary value.
    meta_set: BTreeSet<Vec<String>>,
    meta_replaced_at: BTreeSet<Vec<String>>,
    key_defaults: bool,
    output_logs: bool,
    tasks_replaced: bool,
    explicit_project_enabled: bool,
    explicit_tasks_enabled: BTreeSet<String>,
    authored_project_enabled: bool,
    authored_tasks_enabled: BTreeSet<String>,
}

fn meta_masks(
    masks: &mut Masks,
    path: &mut Vec<String>,
    dict: &DictConfig<DynValue>,
) {
    if matches!(dict, DictConfig::Replace { .. }) {
        masks.meta_replaced_at.insert(path.clone());
    }
    for (key, value) in dict.as_map() {
        path.push(key.clone());
        match value {
            DynValue::Dict(inner) => meta_masks(masks, path, inner),
            _ => {
                masks.meta_set.insert(path.clone());
            }
        }
        path.pop();
    }
}

impl Masks {
    fn from_explicit(explicit: &[LoadedLayer]) -> Self {
        let mut masks = Masks::default();
        for loaded in explicit {
            let layer = loaded.layer();
            masks.description |= layer.description.is_some();
            if let Some(vars) = layer.env.as_ref().and_then(|e| e.vars.as_ref())
            {
                if matches!(vars, DictConfig::Replace { .. }) {
                    masks.env_replaced = true;
                }
                masks.env_keys.extend(vars.as_map().keys().cloned());
            }
            if let Some(meta) = &layer.meta {
                if matches!(meta.0, DictConfig::Replace { .. }) {
                    masks.meta_replaced = true;
                }
                meta_masks(&mut masks, &mut Vec::new(), &meta.0);
            }
            if let Some(cache) = &layer.cache {
                masks.key_defaults |=
                    cache.key.as_ref().is_some_and(|k| k.defaults.is_some());
                masks.explicit_project_enabled |= cache.enabled.is_some();
            }
            masks.output_logs |= layer.output_logs.is_some();
            if let Some(tasks) = &layer.tasks {
                masks.tasks_replaced |=
                    matches!(tasks, DictConfig::Replace { .. });
                for (name, task) in tasks.as_map() {
                    if task.declares_cache_enabled() {
                        masks.explicit_tasks_enabled.insert(name.clone());
                    }
                }
            }
        }
        masks.authored_project_enabled = masks.explicit_project_enabled;
        masks.authored_tasks_enabled = masks.explicit_tasks_enabled.clone();
        masks
    }

    fn masks(&self, field: &FieldPath, tier: OverrideTier) -> bool {
        match field {
            FieldPath::Description => self.description,
            FieldPath::EnvVar(key) => {
                self.env_replaced || self.env_keys.contains(key)
            }
            FieldPath::Meta(path) => {
                self.meta_replaced
                    || self
                        .meta_replaced_at
                        .iter()
                        .any(|at| path.len() > at.len() && path.starts_with(at))
                    || (1..=path.len())
                        .any(|n| self.meta_set.contains(&path[..n]))
            }
            FieldPath::CacheKeyDefaults => self.key_defaults,
            FieldPath::OutputLogs => self.output_logs,
            FieldPath::ProjectCacheEnabled => match tier {
                OverrideTier::Emitted => self.authored_project_enabled,
                OverrideTier::Applied => self.explicit_project_enabled,
            },
            FieldPath::TaskCacheEnabled(task) => match tier {
                OverrideTier::Emitted => {
                    self.authored_project_enabled
                        || self.authored_tasks_enabled.contains(task)
                        || self.tasks_replaced
                }
                OverrideTier::Applied => {
                    self.explicit_tasks_enabled.contains(task)
                        || self.tasks_replaced
                }
            },
        }
    }
}

type Key = (FieldPath, OverrideTier);

/// Records for contributors already sorted in composition order.
pub(crate) fn compute(
    root: &OmniPath,
    contributions: &[Contribution],
    explicit: &[LoadedLayer],
) -> Vec<FieldOverride> {
    let mut masks = Masks::from_explicit(explicit);
    let mut groups: BTreeMap<Key, Vec<(ContributorId, Leaf)>> = BTreeMap::new();
    for contribution in contributions {
        for (field, leaf) in leaves(&contribution.layer) {
            groups
                .entry((field, OverrideTier::Emitted))
                .or_default()
                .push((contribution.id.clone(), leaf));
        }
        for decl in &contribution.enabled {
            let applied = decl.origin.source.contributor_id().is_none();
            let (field, tier) = match (&decl.scope, applied) {
                (EnabledScope::Project, false) => {
                    (FieldPath::ProjectCacheEnabled, OverrideTier::Emitted)
                }
                (EnabledScope::Project, true) => {
                    masks.authored_project_enabled = true;
                    (FieldPath::ProjectCacheEnabled, OverrideTier::Applied)
                }
                (EnabledScope::Task(name), false) => (
                    FieldPath::TaskCacheEnabled(name.clone()),
                    OverrideTier::Emitted,
                ),
                (EnabledScope::Task(name), true) => {
                    masks.authored_tasks_enabled.insert(name.clone());
                    (
                        FieldPath::TaskCacheEnabled(name.clone()),
                        OverrideTier::Applied,
                    )
                }
            };
            groups
                .entry((field, tier))
                .or_default()
                .push((contribution.id.clone(), Leaf::Bool(decl.value)));
        }
    }
    groups
        .into_iter()
        .filter_map(|((field, tier), entries)| {
            let (winner, last) = entries.last()?.clone();
            let effective = match last {
                Leaf::Logs(_) => entries.iter().fold(
                    OutputLogsSplit::default(),
                    |mut acc, (_, leaf)| {
                        if let Leaf::Logs(split) = leaf {
                            merge::Merge::merge(&mut acc, *split);
                        }
                        acc
                    },
                ),
                _ => OutputLogsSplit::default(),
            };
            let effective = match &last {
                Leaf::Logs(_) => Leaf::Logs(effective),
                other => other.clone(),
            };
            let mut overridden: Vec<ContributorId> = Vec::new();
            for (id, leaf) in &entries {
                if leaf.differs_from(&effective) && !overridden.contains(id) {
                    overridden.push(id.clone());
                }
            }
            overridden.retain(|id| id != &winner);
            if overridden.is_empty() {
                return None;
            }
            let masked_by_explicit = masks.masks(&field, tier);
            Some(FieldOverride {
                root: root.clone(),
                field,
                tier,
                winner,
                overridden,
                masked_by_explicit,
            })
        })
        .collect()
}
