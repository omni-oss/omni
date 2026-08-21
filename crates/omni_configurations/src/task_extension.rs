use std::collections::HashSet;

use config_utils::DictConfig;
use merge::Merge;
use omni_config_types::SingleOrMany;
use omni_core::{
    ExtensionGraph, ExtensionGraphError, ExtensionGraphErrorKind,
    ExtensionGraphNode,
};
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use thiserror::Error;

use crate::{
    ProjectConfiguration, TaskConfiguration, TaskConfigurationLongForm,
};

#[derive(Debug, Clone)]
struct TaskExtensionNode {
    name: String,
    extends: Vec<String>,
    task: TaskConfigurationLongForm,
}

impl Merge for TaskExtensionNode {
    fn merge(&mut self, other: Self) {
        self.task.merge(other.task);
    }
}

impl ExtensionGraphNode for TaskExtensionNode {
    type Id = String;

    fn id(&self) -> &Self::Id {
        &self.name
    }

    fn set_id(&mut self, id: &Self::Id) {
        self.name = id.clone();
    }

    fn extendee_ids(&self) -> &[Self::Id] {
        &self.extends
    }

    fn set_extendee_ids(&mut self, extendee_ids: &[Self::Id]) {
        self.extends = extendee_ids.to_vec();
    }
}

impl ProjectConfiguration {
    /// Resolve intra-project task extension in place.
    ///
    /// Tasks marked `base` become extension-only templates and are dropped from
    /// the emitted set; tasks with `extends` inherit and override the tasks they
    /// name. Every task is promoted to its canonical long form before merging so
    /// extending a short-form task is a lossless structural merge. When no task
    /// declares `base` or `extends`, the task map is left untouched.
    pub fn resolve_task_extensions(
        &mut self,
    ) -> Result<(), TaskExtensionError> {
        resolve_task_extensions(&mut self.tasks)
    }
}

fn resolve_task_extensions(
    tasks: &mut DictConfig<TaskConfiguration>,
) -> Result<(), TaskExtensionError> {
    let needs_resolution = tasks.values().any(|task| match task {
        TaskConfiguration::LongForm(long_form) => {
            long_form.base || long_form.extends.is_some()
        }
        TaskConfiguration::ShortForm(_) => false,
    });

    if !needs_resolution {
        return Ok(());
    }

    let nodes = tasks
        .iter()
        .map(|(name, task)| {
            let task = task.clone().into_long_form(name);
            let extends = task
                .extends
                .as_ref()
                .map(SingleOrMany::to_vec)
                .unwrap_or_default();

            TaskExtensionNode {
                name: name.clone(),
                extends,
                task,
            }
        })
        .collect::<Vec<_>>();

    let task_names = nodes
        .iter()
        .map(|node| node.name.clone())
        .collect::<HashSet<_>>();

    for node in &nodes {
        for extendee in &node.extends {
            if !task_names.contains(extendee) {
                return Err(TaskExtensionErrorInner::UnknownExtendee {
                    task: node.name.clone(),
                    extendee: extendee.clone(),
                }
                .into());
            }
        }
    }

    let mut graph =
        ExtensionGraph::from_nodes(nodes).map_err(map_graph_error)?;
    let resolved = graph.get_or_process_all_nodes().map_err(map_graph_error)?;

    let map = tasks.as_map_mut();
    map.clear();

    for node in resolved {
        if node.task.base {
            continue;
        }

        let mut task = node.task;
        task.base = false;
        task.extends = None;

        map.insert(node.name, TaskConfiguration::long_form(task));
    }

    Ok(())
}

fn map_graph_error(error: ExtensionGraphError) -> TaskExtensionError {
    match error.kind() {
        ExtensionGraphErrorKind::CyclicDependency => {
            TaskExtensionErrorInner::Cycle {
                message: error.to_string(),
            }
            .into()
        }
        _ => TaskExtensionErrorInner::ExtensionGraph(error).into(),
    }
}

#[derive(Error, Debug)]
#[error(transparent)]
pub struct TaskExtensionError(pub(crate) TaskExtensionErrorInner);

impl TaskExtensionError {
    pub fn kind(&self) -> TaskExtensionErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<TaskExtensionErrorInner>> From<T> for TaskExtensionError {
    fn from(value: T) -> Self {
        Self(value.into())
    }
}

#[derive(Error, Debug, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(TaskExtensionErrorKind))]
pub(crate) enum TaskExtensionErrorInner {
    #[error("task '{task}' extends unknown task '{extendee}'")]
    UnknownExtendee { task: String, extendee: String },

    #[error("cyclic task extension detected: {message}")]
    Cycle { message: String },

    #[error(transparent)]
    ExtensionGraph(#[from] ExtensionGraphError),
}

#[cfg(test)]
mod tests {
    use config_utils::Replace;
    use omni_command_config::CommandConfig;

    use super::*;
    use crate::TaskEnvConfiguration;

    fn tasks(
        items: Vec<(&str, TaskConfiguration)>,
    ) -> DictConfig<TaskConfiguration> {
        let mut map = maps::map![];
        for (name, task) in items {
            map.insert(name.to_string(), task);
        }
        DictConfig::value(map)
    }

    fn long(
        base: bool,
        extends: Option<Vec<&str>>,
        exec: Option<&str>,
    ) -> TaskConfiguration {
        TaskConfiguration::long_form(TaskConfigurationLongForm {
            base,
            extends: extends.map(|e| {
                SingleOrMany::Many(
                    e.into_iter().map(|s| s.to_string()).collect(),
                )
            }),
            exec: exec
                .map(|e| Replace::new(CommandConfig::Shell(e.to_string()))),
            ..Default::default()
        })
    }

    fn env_vars(pairs: &[(&str, &str)]) -> TaskEnvConfiguration {
        let mut map = maps::map![];
        for (key, value) in pairs {
            map.insert(key.to_string(), Replace::new(value.to_string()));
        }
        TaskEnvConfiguration {
            vars: Some(DictConfig::value(map)),
        }
    }

    fn get_long<'a>(
        tasks: &'a DictConfig<TaskConfiguration>,
        name: &str,
    ) -> &'a TaskConfigurationLongForm {
        match tasks.as_map().get(name).expect("task should be present") {
            TaskConfiguration::LongForm(long_form) => long_form,
            TaskConfiguration::ShortForm(_) => panic!("expected long form"),
        }
    }

    #[test]
    fn test_fast_path_is_no_op() {
        let mut t = tasks(vec![
            ("a", TaskConfiguration::short_form("echo a")),
            ("b", long(false, None, Some("echo b"))),
        ]);
        let original = t.clone();

        resolve_task_extensions(&mut t).unwrap();

        assert_eq!(t, original);
        assert!(matches!(
            t.as_map().get("a"),
            Some(TaskConfiguration::ShortForm(_))
        ));
    }

    #[test]
    fn test_single_extend_overrides_and_drops_base() {
        let base = TaskConfigurationLongForm {
            base: true,
            exec: Some(Replace::new(CommandConfig::Shell("base".to_string()))),
            env: env_vars(&[("A", "1")]),
            ..Default::default()
        };

        let derived = TaskConfigurationLongForm {
            extends: Some(SingleOrMany::Single("base_task".to_string())),
            env: env_vars(&[("B", "2")]),
            ..Default::default()
        };

        let mut t = tasks(vec![
            ("base_task", TaskConfiguration::long_form(base)),
            ("derived", TaskConfiguration::long_form(derived)),
        ]);

        resolve_task_extensions(&mut t).unwrap();

        assert!(t.as_map().get("base_task").is_none());

        let derived = get_long(&t, "derived");
        assert_eq!(
            derived.exec,
            Some(Replace::new(CommandConfig::Shell("base".to_string())))
        );
        assert!(!derived.base);
        assert_eq!(derived.extends, None);

        let vars = derived.env.vars.as_ref().expect("env vars").to_map_inner();
        assert_eq!(vars.get("A").map(String::as_str), Some("1"));
        assert_eq!(vars.get("B").map(String::as_str), Some("2"));
    }

    #[test]
    fn test_extend_chain() {
        let mut t = tasks(vec![
            ("a", long(true, None, Some("a"))),
            ("b", long(true, Some(vec!["a"]), None)),
            ("c", long(false, Some(vec!["b"]), None)),
        ]);

        resolve_task_extensions(&mut t).unwrap();

        assert!(t.as_map().get("a").is_none());
        assert!(t.as_map().get("b").is_none());
        let c = get_long(&t, "c");
        assert_eq!(
            c.exec,
            Some(Replace::new(CommandConfig::Shell("a".to_string())))
        );
    }

    #[test]
    fn test_diamond_dedup() {
        let mut t = tasks(vec![
            ("a", long(true, None, Some("a"))),
            ("b", long(true, Some(vec!["a"]), None)),
            ("c", long(true, Some(vec!["a"]), None)),
            ("d", long(false, Some(vec!["b", "c"]), None)),
        ]);

        resolve_task_extensions(&mut t).unwrap();

        let d = get_long(&t, "d");
        assert_eq!(
            d.exec,
            Some(Replace::new(CommandConfig::Shell("a".to_string())))
        );
        assert!(!d.base);
        assert_eq!(d.extends, None);
    }

    #[test]
    fn test_base_marker_does_not_propagate_through_extends() {
        // `base` is a per-declaration template marker, never inherited: a
        // concrete task that extends a `base: true` template must itself be
        // emitted, not dropped. (Guards against `base` ever adopting the
        // inherit-on-omit rule used for `extends`.)
        let mut t = tasks(vec![
            ("tmpl", long(true, None, Some("tmpl"))),
            ("concrete", long(false, Some(vec!["tmpl"]), None)),
        ]);

        resolve_task_extensions(&mut t).unwrap();

        assert!(
            t.as_map().get("tmpl").is_none(),
            "the base template must be dropped"
        );
        let concrete = get_long(&t, "concrete");
        assert!(!concrete.base, "the extender must not inherit `base`");
        assert_eq!(
            concrete.exec,
            Some(Replace::new(CommandConfig::Shell("tmpl".to_string())))
        );
    }

    #[test]
    fn test_empty_extends_applies_no_extension() {
        // `extends: []` is a real (Some) value that resolves to no extendees,
        // so the task keeps its own definition and is emitted standalone.
        let mut t = tasks(vec![
            ("a", long(true, None, Some("a"))),
            ("b", long(false, Some(vec![]), Some("b"))),
        ]);

        resolve_task_extensions(&mut t).unwrap();

        assert!(t.as_map().get("a").is_none());
        let b = get_long(&t, "b");
        assert_eq!(
            b.exec,
            Some(Replace::new(CommandConfig::Shell("b".to_string()))),
            "empty extends must not pull in any other task"
        );
        assert_eq!(b.extends, None);
    }

    #[test]
    fn test_cycle_is_rejected() {
        let mut t = tasks(vec![
            ("a", long(false, Some(vec!["b"]), None)),
            ("b", long(false, Some(vec!["a"]), None)),
        ]);

        let err = resolve_task_extensions(&mut t).unwrap_err();
        assert_eq!(err.kind(), TaskExtensionErrorKind::Cycle);
    }

    #[test]
    fn test_unknown_extendee_is_rejected() {
        let mut t =
            tasks(vec![("a", long(false, Some(vec!["missing"]), Some("a")))]);

        let err = resolve_task_extensions(&mut t).unwrap_err();
        assert_eq!(err.kind(), TaskExtensionErrorKind::UnknownExtendee);
    }

    #[test]
    fn test_extending_non_base_keeps_target() {
        let mut t = tasks(vec![
            ("a", long(false, None, Some("a"))),
            ("b", long(false, Some(vec!["a"]), Some("b"))),
        ]);

        resolve_task_extensions(&mut t).unwrap();

        assert!(t.as_map().get("a").is_some());
        assert!(t.as_map().get("b").is_some());
        let b = get_long(&t, "b");
        assert_eq!(
            b.exec,
            Some(Replace::new(CommandConfig::Shell("b".to_string())))
        );
    }

    #[test]
    fn test_extending_short_form_base_is_lossless() {
        let derived = TaskConfigurationLongForm {
            extends: Some(SingleOrMany::Single("a".to_string())),
            env: env_vars(&[("B", "2")]),
            ..Default::default()
        };

        let mut t = tasks(vec![
            ("a", TaskConfiguration::short_form("echo a")),
            ("derived", TaskConfiguration::long_form(derived)),
        ]);

        resolve_task_extensions(&mut t).unwrap();

        let derived = get_long(&t, "derived");
        assert_eq!(
            derived.exec,
            Some(Replace::new(CommandConfig::Shell("echo a".to_string())))
        );
        // The short-form target's implicit upstream self-dependency survives.
        let deps = derived.dependencies.to_vec();
        assert!(deps.iter().any(|d| matches!(
            d,
            crate::TaskDependencyConfiguration::Upstream { task } if task == "a"
        )));
    }
}
