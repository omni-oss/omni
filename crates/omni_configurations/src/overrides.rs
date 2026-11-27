//! Which earlier contributors a later one overrode (RFC 0024 F07.3), as
//! structured data for the notice (F07.4) and inspection (F07.5). Records hold
//! contributor ids and field paths only, never values.

use std::collections::{BTreeMap, BTreeSet};

use config_utils::{DictConfig, DynValue, ListConfig};
use omni_task_output_logs::OutputLogsSplit;
use omni_types::OmniPath;

use crate::{
    Contribution, ContributorId, EnabledScope, LoadedLayer, ProjectLayer,
    TaskLayer,
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
        self != effective
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
    /// Paths an explicit layer sets to a dictionary.
    meta_dict_at: BTreeSet<Vec<String>>,
    /// Paths an explicit layer sets to an additive list. It merges into a list
    /// and replaces any other value.
    meta_list_at: BTreeSet<Vec<String>>,
    tasks_aliased: BTreeSet<String>,
    key_defaults: bool,
    logs_new: bool,
    logs_cached: bool,
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
            DynValue::Dict(inner) => {
                masks.meta_dict_at.insert(path.clone());
                meta_masks(masks, path, inner);
            }
            // An explicit additive list merges into the contributors' list.
            DynValue::List(
                ListConfig::Append { .. }
                | ListConfig::Prepend { .. }
                | ListConfig::Merge { .. },
            ) => {
                masks.meta_list_at.insert(path.clone());
            }
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
            if let Some(logs) = &layer.output_logs {
                let split = logs.to_split();
                masks.logs_new |= split.new.is_some();
                masks.logs_cached |= split.cached.is_some();
            }
            if let Some(tasks) = &layer.tasks {
                masks.tasks_replaced |=
                    matches!(tasks, DictConfig::Replace { .. });
                for (name, task) in tasks.as_map() {
                    if matches!(task, TaskLayer::Alias(_)) {
                        masks.tasks_aliased.insert(name.clone());
                    }
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

    /// Whether explicit configuration decides the field regardless of the
    /// contributors. `winner_is_dict` says whether the winning `meta` node is a
    /// dictionary (an explicit dictionary merges into it; over anything else it
    /// replaces it). `logs` lists the facets that are overridden.
    fn masks(
        &self,
        field: &FieldPath,
        tier: OverrideTier,
        winner_is_dict: bool,
        winner_is_list: bool,
        logs: (bool, bool),
    ) -> bool {
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
                        .any(|at| path.starts_with(at))
                    || (1..=path.len())
                        .any(|n| self.meta_set.contains(&path[..n]))
                    || (!winner_is_dict && self.meta_dict_at.contains(path))
                    || (1..path.len())
                        .any(|n| self.meta_list_at.contains(&path[..n]))
                    || (!winner_is_list && self.meta_list_at.contains(path))
            }
            FieldPath::CacheKeyDefaults => self.key_defaults,
            FieldPath::OutputLogs => {
                let (new, cached) = logs;
                (new || cached)
                    && (!new || self.logs_new)
                    && (!cached || self.logs_cached)
            }
            FieldPath::ProjectCacheEnabled => match tier {
                OverrideTier::Emitted => self.authored_project_enabled,
                OverrideTier::Applied => self.explicit_project_enabled,
            },
            FieldPath::TaskCacheEnabled(task) => match tier {
                OverrideTier::Emitted => {
                    self.authored_project_enabled
                        || self.authored_tasks_enabled.contains(task)
                        || self.tasks_replaced
                        || self.tasks_aliased.contains(task)
                }
                OverrideTier::Applied => {
                    self.explicit_tasks_enabled.contains(task)
                        || self.tasks_replaced
                        || self.tasks_aliased.contains(task)
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
    let mut records: Vec<(FieldOverride, bool)> = groups
        .into_iter()
        .filter_map(|((field, tier), entries)| {
            let found = decide(&entries)?;
            let masked_by_explicit = masks.masks(
                &field,
                tier,
                found.winner_is_dict,
                found.winner_is_list,
                found.logs_facets,
            );
            Some((
                FieldOverride {
                    root: root.clone(),
                    field,
                    tier,
                    winner: found.winner,
                    overridden: found.overridden,
                    masked_by_explicit,
                },
                found.winner_is_dict,
            ))
        })
        .collect();
    // A nested `meta` record is stale when a proper ancestor ends up as a
    // non-dictionary value: the nested keys are not in the composed result.
    let scalar_ancestors: Vec<Vec<String>> = records
        .iter()
        .filter_map(|(record, is_dict)| match &record.field {
            FieldPath::Meta(path) if !is_dict => Some(path.clone()),
            _ => None,
        })
        .collect();
    records.retain(|(record, _)| match &record.field {
        FieldPath::Meta(path) => !scalar_ancestors.iter().any(|ancestor| {
            path.len() > ancestor.len() && path.starts_with(ancestor)
        }),
        _ => true,
    });
    records.into_iter().map(|(record, _)| record).collect()
}

struct Decided {
    winner: ContributorId,
    overridden: Vec<ContributorId>,
    winner_is_dict: bool,
    winner_is_list: bool,
    logs_facets: (bool, bool),
}

/// The winner and overridden contributors of one field. `entries` are in
/// composition order.
fn decide(entries: &[(ContributorId, Leaf)]) -> Option<Decided> {
    let (last_id, last) = entries.last()?;
    if let Leaf::Logs(_) = last {
        return decide_logs(entries);
    }
    let mut overridden: Vec<ContributorId> = Vec::new();
    for (id, leaf) in entries {
        if leaf.differs_from(last) && !overridden.contains(id) {
            overridden.push(id.clone());
        }
    }
    overridden.retain(|id| id != last_id);
    if overridden.is_empty() {
        return None;
    }
    Some(Decided {
        winner: last_id.clone(),
        overridden,
        winner_is_dict: matches!(last, Leaf::DictNode),
        winner_is_list: matches!(last, Leaf::ListNode),
        logs_facets: (false, false),
    })
}

/// `output_logs` facets (`new`, `cached`) are separate leaves: a contributor is
/// overridden when it set a facet that a later one set differently, and the
/// winner is the last contributor that set an overridden facet.
fn decide_logs(entries: &[(ContributorId, Leaf)]) -> Option<Decided> {
    type Facet =
        fn(&OutputLogsSplit) -> Option<omni_task_output_logs::LogsDisplay>;
    let facets: [Facet; 2] = [|s| s.new, |s| s.cached];
    let mut flags = [false; 2];
    let mut winner: Option<usize> = None;
    let mut overridden: Vec<ContributorId> = Vec::new();
    for (slot, facet) in facets.iter().enumerate() {
        let setters: Vec<(usize, &ContributorId, _)> = entries
            .iter()
            .enumerate()
            .filter_map(|(index, (id, leaf))| match leaf {
                Leaf::Logs(split) => {
                    facet(split).map(|value| (index, id, value))
                }
                _ => None,
            })
            .collect();
        let Some(&(last_index, last_id, last_value)) = setters.last() else {
            continue;
        };
        for (_, id, value) in &setters {
            if *value != last_value
                && *id != last_id
                && !overridden.contains(id)
            {
                overridden.push((*id).clone());
            }
            if *value != last_value && *id != last_id {
                flags[slot] = true;
            }
        }
        if flags[slot] {
            winner = Some(winner.map_or(last_index, |w| w.max(last_index)));
        }
    }
    let winner = entries[winner?].0.clone();
    overridden.retain(|id| id != &winner);
    overridden.sort_by_key(|id| entries.iter().position(|(e, _)| e == id));
    if overridden.is_empty() {
        return None;
    }
    Some(Decided {
        winner,
        overridden,
        winner_is_dict: false,
        winner_is_list: false,
        logs_facets: (flags[0], flags[1]),
    })
}
