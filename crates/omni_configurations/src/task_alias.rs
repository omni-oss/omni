//! The canonical alias index of one composed project (RFC 0024 F03.4).
//!
//! An alias is another name for a task of the same project. After composition
//! the surviving aliases are resolved here, so chains are flattened and every
//! alias points at a runnable definition. The index is not a task map: aliases
//! never become executable tasks.

use std::collections::HashSet;

use config_utils::DictConfig;
use maps::{Map, OrderedMap};
use omni_core::TaskAliases;
use omni_types::{ReferenceSurface, TaskReference};
use thiserror::Error;

omni_codes::prefixes!("task_alias");

use crate::TaskLayer;

/// Decode a same-project reference operand (an `extends` entry or an alias
/// target) to a raw task name.
pub(crate) fn decode_same_project_reference(
    spelling: &str,
) -> Result<String, String> {
    let literal = || {
        TaskReference::own(spelling)
            .map(|reference| reference.format())
            .unwrap_or_else(|_| spelling.to_owned())
    };
    let hint = |e: &dyn std::fmt::Display| {
        format!(
            "`{spelling}` is not a valid same-project task name: {e}; write `{}` for a task literally named `{spelling}`",
            literal()
        )
    };
    let reference = TaskReference::parse(spelling).map_err(|e| hint(&e))?;
    reference
        .validate_for(ReferenceSurface::ExtendsOrAliasTarget)
        .map_err(|e| hint(&e))?;
    if reference.plugin().is_some() {
        return Err(hint(&"plugin scopes are not supported in this version"));
    }
    Ok(reference.task().to_owned())
}

/// Remove every alias entry from `tasks` and return `(alias, target spelling)`
/// in declaration order.
pub(crate) fn take_aliases(
    tasks: &mut DictConfig<TaskLayer>,
) -> Vec<(String, String)> {
    let map = tasks.as_map_mut();
    let names = map
        .iter()
        .filter(|(_, task)| matches!(task, TaskLayer::Alias(_)))
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    names
        .into_iter()
        .filter_map(|name| match map.shift_remove(&name)? {
            TaskLayer::Alias(alias) => Some((name, alias.alias)),
            _ => None,
        })
        .collect()
}

/// Flatten alias chains over `task_names` (every non-alias declaration of the
/// project, base templates included).
pub(crate) fn resolve_aliases(
    project: &str,
    aliases: &[(String, String)],
    task_names: &HashSet<String>,
) -> Result<OrderedMap<String, String>, TaskAliasError> {
    let mut targets: Map<&str, String> = maps::map![];
    for (alias, target) in aliases {
        let target =
            decode_same_project_reference(target).map_err(|message| {
                TaskAliasErrorInner::InvalidTarget {
                    project: project.to_owned(),
                    alias: alias.clone(),
                    message,
                }
            })?;
        targets.insert(alias.as_str(), target);
    }

    let mut resolved = OrderedMap::new();
    for (alias, _) in aliases {
        let mut chain = vec![alias.as_str()];
        let mut current = targets[alias.as_str()].as_str();
        let canonical = loop {
            if task_names.contains(current) {
                break current;
            }
            match targets.get(current) {
                Some(next) => {
                    if chain.contains(&current) {
                        chain.push(current);
                        return Err(TaskAliasErrorInner::Cycle {
                            project: project.to_owned(),
                            chain: chain.join(" -> "),
                        }
                        .into());
                    }
                    chain.push(current);
                    current = next.as_str();
                }
                None => {
                    return Err(TaskAliasErrorInner::MissingTarget {
                        project: project.to_owned(),
                        alias: alias.clone(),
                        target: current.to_owned(),
                    }
                    .into());
                }
            }
        };
        resolved.insert(alias.clone(), canonical.to_owned());
    }
    Ok(resolved)
}

/// Require every canonical target to survive inheritance as a runnable task.
pub(crate) fn finish_aliases(
    project: &str,
    resolved: OrderedMap<String, String>,
    runnable: impl Fn(&str) -> bool,
) -> Result<TaskAliases, TaskAliasError> {
    for (alias, target) in &resolved {
        if !runnable(target) {
            return Err(TaskAliasErrorInner::BaseTarget {
                project: project.to_owned(),
                alias: alias.clone(),
                target: target.clone(),
            }
            .into());
        }
    }
    Ok(TaskAliases::new(resolved))
}

#[derive(Error, Debug, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
pub struct TaskAliasError(pub(crate) TaskAliasErrorInner);

impl TaskAliasError {
    pub fn kind(&self) -> TaskAliasErrorKind {
        match self.0 {
            TaskAliasErrorInner::InvalidTarget { .. } => {
                TaskAliasErrorKind::InvalidTarget
            }
            TaskAliasErrorInner::MissingTarget { .. } => {
                TaskAliasErrorKind::MissingTarget
            }
            TaskAliasErrorInner::Cycle { .. } => TaskAliasErrorKind::Cycle,
            TaskAliasErrorInner::BaseTarget { .. } => {
                TaskAliasErrorKind::BaseTarget
            }
        }
    }
}

/// What kind of alias failure a [`TaskAliasError`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskAliasErrorKind {
    InvalidTarget,
    MissingTarget,
    Cycle,
    BaseTarget,
}

impl<T: Into<TaskAliasErrorInner>> From<T> for TaskAliasError {
    fn from(value: T) -> Self {
        Self(value.into())
    }
}

#[derive(Error, Debug, omni_diagnostic::Diagnostic)]
pub(crate) enum TaskAliasErrorInner {
    /// An alias target is not a valid same-project task name.
    #[error(
        "alias '{alias}' of project '{project}' has an invalid target: {message}"
    )]
    #[diagnostic(
        code = "task_alias.invalid_target",
        hint = "fix the target of alias `{alias}` in project `{project}`"
    )]
    InvalidTarget {
        #[diagnostic(detail)]
        project: String,
        #[diagnostic(detail)]
        alias: String,
        #[diagnostic(detail)]
        message: String,
    },

    /// An alias names a task its project does not define.
    #[error(
        "alias '{alias}' of project '{project}' targets unknown task '{target}'"
    )]
    #[diagnostic(
        code = "task_alias.missing_target",
        hint = "define task `{target}` in project `{project}`, or point alias `{alias}` at an existing task"
    )]
    MissingTarget {
        #[diagnostic(detail)]
        project: String,
        #[diagnostic(detail)]
        alias: String,
        #[diagnostic(detail)]
        target: String,
    },

    /// Aliases name each other in a cycle.
    #[error("alias cycle in project '{project}': {chain}")]
    #[diagnostic(
        code = "task_alias.cycle",
        hint = "point one alias in the cycle at a task that is not an alias"
    )]
    Cycle {
        #[diagnostic(detail)]
        project: String,
        #[diagnostic(detail)]
        chain: String,
    },

    /// An alias targets a base template, which never runs.
    #[error(
        "alias '{alias}' of project '{project}' targets base task '{target}', which is not runnable"
    )]
    #[diagnostic(
        code = "task_alias.base_target",
        hint = "point alias `{alias}` at a runnable task that extends `{target}`"
    )]
    BaseTarget {
        #[diagnostic(detail)]
        project: String,
        #[diagnostic(detail)]
        alias: String,
        #[diagnostic(detail)]
        target: String,
    },
}
