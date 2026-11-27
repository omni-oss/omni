//! Ordered contributor composition (RFC 0024 F07.3): contributors compose in the
//! order of the given sequence, the later one wins, and explicit layers replay
//! over all of them.
//!
//! The extension graph still provides only ordered authored declarations; this
//! entry point takes already validated, activation-locally applied
//! contributions. It adds no plugin host and nothing in production calls it yet.

use std::collections::{HashMap, HashSet};

use config_utils::{DictConfig, ListConfig};
use merge::Merge;
use omni_config_types::MergeSingleOrMany;
use omni_glob_config::MergeGlobConfig;
use omni_types::OmniPath;

use crate::{
    CacheLayer, ContributorId, DeclarationSource, EnabledDecl, LoadedLayer,
    ProjectLayer, TaskLayer, TaskLongFormLayer,
};

/// One contributor's finished contribution.
///
/// `layer` holds additive forms only and no `cache.enabled`; the tiered
/// `cache.enabled` declarations travel in `enabled`, each with an origin that
/// says whether the plugin emitted it (`DeclarationSource::Contributor` with
/// this contribution's id) or an applied file declared it.
#[derive(Debug, Clone)]
pub struct Contribution {
    pub id: ContributorId,
    pub layer: ProjectLayer,
    pub base_dir: OmniPath,
    pub enabled: Vec<EnabledDecl>,
}

/// The composed result: contributors in order, then explicit layers. Finalize
/// it with [`LoadedLayer::finalize_composed`].
#[derive(Debug, Clone)]
pub struct Composed {
    pub layer: LoadedLayer,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ComposeError {
    #[error("contributor `{id}` appears more than once in the order")]
    DuplicateInOrder { id: String },
    #[error("contributor `{id}` was given more than once")]
    DuplicateContribution { id: String },
    #[error("contribution from `{id}` is not in the order")]
    UnknownContributor { id: String },
    #[error("contributor `{id}` is in the order but gave no contribution")]
    MissingContribution { id: String },
    #[error(
        "contributor `{contributor}` declared `cache.enabled` with an origin that is not its own"
    )]
    ForeignOrigin { contributor: String },
    #[error(
        "contributor `{contributor}` declared `cache.enabled` for task `{task}`, which no layer declares"
    )]
    UnknownTask { contributor: String, task: String },
    #[error(
        "contributor `{contributor}` supplied `{field}` in a form it may not use: {reason}"
    )]
    ForbiddenForm {
        contributor: String,
        field: String,
        reason: &'static str,
    },
}

/// Compose `contributions` in `order` (earlier first, the later one wins), then
/// `explicit` layers in the given order, which beat every contributor. Arrival
/// order of `contributions` does not matter. With no contributors this is the
/// plain fold of `explicit`.
pub fn compose_ordered(
    root: OmniPath,
    order: &[ContributorId],
    contributions: Vec<Contribution>,
    explicit: Vec<LoadedLayer>,
) -> Result<Composed, ComposeError> {
    let mut position: HashMap<&ContributorId, usize> = HashMap::new();
    for (index, id) in order.iter().enumerate() {
        if position.insert(id, index).is_some() {
            return Err(ComposeError::DuplicateInOrder {
                id: id.as_str().to_owned(),
            });
        }
    }
    let mut seen = HashSet::new();
    for contribution in &contributions {
        if !position.contains_key(&contribution.id) {
            return Err(ComposeError::UnknownContributor {
                id: contribution.id.as_str().to_owned(),
            });
        }
        if !seen.insert(&contribution.id) {
            return Err(ComposeError::DuplicateContribution {
                id: contribution.id.as_str().to_owned(),
            });
        }
    }
    if let Some(id) = order.iter().find(|id| !seen.contains(id)) {
        return Err(ComposeError::MissingContribution {
            id: id.as_str().to_owned(),
        });
    }

    let mut contributions = contributions;
    contributions.sort_by_key(|contribution| position[&contribution.id]);

    let mut declared_tasks: HashSet<String> = HashSet::new();
    for layer in contributions
        .iter()
        .map(|c| &c.layer)
        .chain(explicit.iter().map(|l| l.layer()))
    {
        declared_tasks.extend(declared_task_names(layer));
    }
    for contribution in &contributions {
        guard(contribution)?;
        for decl in &contribution.enabled {
            if let DeclarationSource::Contributor { id } = &decl.origin.source
                && id != &contribution.id
            {
                return Err(ComposeError::ForeignOrigin {
                    contributor: contribution.id.as_str().to_owned(),
                });
            }
            if let crate::EnabledScope::Task(task) = &decl.scope
                && !declared_tasks.contains(task)
            {
                return Err(ComposeError::UnknownTask {
                    contributor: contribution.id.as_str().to_owned(),
                    task: task.clone(),
                });
            }
        }
    }

    let explicit_empty = explicit.is_empty();
    let layers = contributions
        .into_iter()
        .map(|c| {
            LoadedLayer::contributed(c.id, c.base_dir, c.layer)
                .with_enabled(c.enabled)
        })
        .chain(explicit);
    let mut composed: Option<LoadedLayer> = None;
    for layer in layers {
        match composed.as_mut() {
            Some(acc) => acc.merge(layer),
            None => composed = Some(layer),
        }
    }
    let mut layer = composed.unwrap_or_default();
    if explicit_empty {
        layer = layer.with_dir(root);
    }
    Ok(Composed { layer })
}

fn declared_task_names(layer: &ProjectLayer) -> Vec<String> {
    let Some(tasks) = layer.tasks.as_ref() else {
        return Vec::new();
    };
    tasks
        .as_map()
        .iter()
        .filter(|(_, task)| !matches!(task, TaskLayer::Alias(_)))
        .map(|(name, _)| name.clone())
        .collect()
}

struct Guard<'a> {
    contributor: &'a str,
}

impl Guard<'_> {
    fn forbid(
        &self,
        field: &str,
        reason: &'static str,
    ) -> Result<(), ComposeError> {
        Err(ComposeError::ForbiddenForm {
            contributor: self.contributor.to_owned(),
            field: field.to_owned(),
            reason,
        })
    }

    fn list<T: Merge>(
        &self,
        field: &str,
        list: &Option<ListConfig<T>>,
    ) -> Result<(), ComposeError> {
        match list {
            None | Some(ListConfig::Append { .. }) => Ok(()),
            Some(_) => self.forbid(field, "lists must use the `append` form"),
        }
    }

    fn dict<T: Merge>(
        &self,
        field: &str,
        dict: &Option<DictConfig<T>>,
    ) -> Result<(), ComposeError> {
        match dict {
            Some(DictConfig::Replace { .. }) => {
                self.forbid(field, "`replace` would erase other contributors")
            }
            _ => Ok(()),
        }
    }

    fn side<T: Merge>(
        &self,
        field: &str,
        side: &MergeSingleOrMany<T>,
    ) -> Result<(), ComposeError> {
        match side {
            MergeSingleOrMany::List(ListConfig::Append { .. }) => Ok(()),
            _ => self.forbid(field, "patterns must use the `append` form"),
        }
    }

    fn glob<T: Merge>(
        &self,
        field: &str,
        glob: &Option<MergeGlobConfig<T>>,
    ) -> Result<(), ComposeError> {
        match glob {
            None => Ok(()),
            Some(MergeGlobConfig::Single(_)) => {
                self.forbid(field, "a bare pattern must use the `append` form")
            }
            Some(MergeGlobConfig::Many(side)) => self.side(field, side),
            Some(MergeGlobConfig::IncludeAndExclude { include, exclude }) => {
                include
                    .iter()
                    .chain(exclude.iter())
                    .try_for_each(|side| self.side(field, side))
            }
        }
    }

    fn cache(
        &self,
        scope: &str,
        cache: &Option<CacheLayer>,
    ) -> Result<(), ComposeError> {
        let Some(cache) = cache else { return Ok(()) };
        if cache.enabled.is_some() {
            return self.forbid(
                &format!("{scope}cache.enabled"),
                "declare it as a typed `cache.enabled` declaration",
            );
        }
        if cache.remote.is_some() {
            return self.forbid(
                &format!("{scope}cache.remote"),
                "only explicit configuration sets it",
            );
        }
        if let Some(key) = &cache.key {
            self.list(&format!("{scope}cache.key.env"), &key.env)?;
            self.glob(&format!("{scope}cache.key.files"), &key.files)?;
        }
        if let Some(output) = &cache.output {
            self.glob(&format!("{scope}cache.output.files"), &output.files)?;
        }
        Ok(())
    }

    fn task(
        &self,
        name: &str,
        task: &TaskLongFormLayer,
    ) -> Result<(), ComposeError> {
        let scope = format!("tasks.{name}.");
        self.dict(&format!("{scope}args"), &task.args)?;
        self.list(&format!("{scope}dependencies"), &task.dependencies)?;
        self.list(&format!("{scope}with"), &task.with)?;
        if task.redact.is_some() {
            return self.forbid(&format!("{scope}redact"), "host-owned");
        }
        if let Some(env) = &task.env {
            self.dict(&format!("{scope}env.vars"), &env.vars)?;
        }
        if let Some(meta) = &task.meta {
            self.dict_value(&format!("{scope}meta"), &meta.0)?;
        }
        self.cache(&scope, &task.cache)
    }

    fn dict_value<T: Merge>(
        &self,
        field: &str,
        dict: &DictConfig<T>,
    ) -> Result<(), ComposeError> {
        if matches!(dict, DictConfig::Replace { .. }) {
            return self
                .forbid(field, "`replace` would erase other contributors");
        }
        Ok(())
    }
}

fn guard(contribution: &Contribution) -> Result<(), ComposeError> {
    let g = Guard {
        contributor: contribution.id.as_str(),
    };
    let layer = &contribution.layer;
    g.list("dependencies", &layer.dependencies)?;
    if let Some(env) = &layer.env {
        g.dict("env.vars", &env.vars)?;
    }
    if let Some(meta) = &layer.meta {
        g.dict_value("meta", &meta.0)?;
    }
    if layer.redact.is_some() {
        return g.forbid("redact", "host-owned");
    }
    g.cache("", &layer.cache)?;
    if let Some(tasks) = &layer.tasks {
        g.dict_value("tasks", tasks)?;
        for (name, task) in tasks.as_map() {
            if let TaskLayer::LongForm(task) = task {
                g.task(name, task)?;
            }
        }
    }
    Ok(())
}
