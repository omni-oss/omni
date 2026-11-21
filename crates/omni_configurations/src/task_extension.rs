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

omni_codes::prefixes!("task_extension");

use crate::TaskLayer;

#[derive(Debug, Clone)]
struct TaskExtensionNode<T> {
    name: String,
    extends: Vec<String>,
    task: T,
}

impl<T: Merge> Merge for TaskExtensionNode<T> {
    fn merge(&mut self, other: Self) {
        self.task.merge(other.task);
    }
}

impl<T: Merge + Clone + std::fmt::Debug> ExtensionGraphNode
    for TaskExtensionNode<T>
{
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

/// Merge every node into the nodes it extends, in extension order.
fn resolve_nodes<T: Merge + Clone + std::fmt::Debug>(
    nodes: Vec<TaskExtensionNode<T>>,
) -> Result<Vec<TaskExtensionNode<T>>, TaskExtensionError> {
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
    graph.get_or_process_all_nodes().map_err(map_graph_error)
}

/// Resolve intra-project task extension in place over presence-aware task
/// layers, so omitted fields stay omitted through task inheritance.
///
/// Tasks marked `base` become extension-only templates and are dropped from
/// the emitted set; tasks with `extends` inherit and override the tasks they
/// name. When no task declares `base` or `extends`, the map is left untouched
/// and `false` is returned.
pub(crate) fn resolve_task_layer_extensions(
    tasks: &mut DictConfig<TaskLayer>,
) -> Result<bool, TaskExtensionError> {
    if !tasks.values().any(TaskLayer::needs_extension_resolution) {
        return Ok(false);
    }

    let mut aliases = Vec::new();
    let nodes = tasks
        .iter()
        .filter_map(|(name, task)| {
            let Some(task) = task.clone().into_declarations(name) else {
                aliases.push((name.clone(), task.clone()));
                return None;
            };
            let extends =
                task.extends().map(SingleOrMany::to_vec).unwrap_or_default();

            Some(TaskExtensionNode {
                name: name.clone(),
                extends,
                task,
            })
        })
        .collect::<Vec<_>>();

    let resolved = resolve_nodes(nodes)?;

    let map = tasks.as_map_mut();
    map.clear();

    for node in resolved {
        if node.task.base() {
            continue;
        }

        map.insert(node.name, TaskLayer::Composed(node.task));
    }
    map.extend(aliases);

    Ok(true)
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

#[derive(Error, Debug, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
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

#[derive(Error, Debug, EnumDiscriminants, omni_diagnostic::Diagnostic)]
#[strum_discriminants(vis(pub), name(TaskExtensionErrorKind))]
pub(crate) enum TaskExtensionErrorInner {
    /// A task extends a task that its project does not define.
    #[error("task '{task}' extends unknown task '{extendee}'")]
    #[diagnostic(
        code = "task_extension.unknown_extendee",
        hint = "define task `{extendee}` in this project, or fix the `extends` entry of `{task}`"
    )]
    UnknownExtendee {
        #[diagnostic(detail)]
        task: String,
        #[diagnostic(detail)]
        extendee: String,
    },

    /// Tasks extend each other in a cycle.
    #[error("cyclic task extension detected: {message}")]
    #[diagnostic(
        code = "task_extension.cycle",
        hint = "remove the circular `extends` chain between these tasks"
    )]
    Cycle { message: String },

    #[error(transparent)]
    #[diagnostic(delegate)]
    ExtensionGraph(#[from] ExtensionGraphError),
}

#[cfg(test)]
mod tests {
    use config_utils::Replace;
    use omni_command_config::CommandConfig;

    use crate::{
        FinalizeError, FinalizedProject, LoadedLayer, ProjectLayer,
        TaskConfiguration, TaskConfigurationLongForm,
        TaskDependencyConfiguration, TaskExtensionErrorKind,
    };

    fn compose(tasks: &str) -> Result<FinalizedProject, FinalizeError> {
        let layer: ProjectLayer =
            serde_json::from_str(&format!(r#"{{"name":"p","tasks":{tasks}}}"#))
                .unwrap();
        LoadedLayer {
            layer,
            ..Default::default()
        }
        .finalize_composed()
    }

    fn resolve(tasks: &str) -> FinalizedProject {
        compose(tasks).unwrap()
    }

    fn extension_error(tasks: &str) -> TaskExtensionErrorKind {
        match compose(tasks) {
            Err(FinalizeError::TaskExtension(e)) => e.kind(),
            _ => panic!("expected a task extension error"),
        }
    }

    fn get_long(
        project: &FinalizedProject,
        name: &str,
    ) -> TaskConfigurationLongForm {
        project
            .config
            .tasks
            .as_map()
            .get(name)
            .expect("task should be present")
            .clone()
            .into_long_form(name)
    }

    fn shell(command: &str) -> Option<Replace<CommandConfig>> {
        Some(Replace::new(CommandConfig::Shell(command.to_string())))
    }

    #[test]
    fn test_fast_path_is_no_op() {
        let p = resolve(r#"{"a":"echo a","b":{"exec":"echo b"}}"#);

        assert!(matches!(
            p.config.tasks.as_map().get("a"),
            Some(TaskConfiguration::ShortForm(_))
        ));
        assert_eq!(get_long(&p, "b").exec, shell("echo b"));
    }

    #[test]
    fn test_single_extend_overrides_and_drops_base() {
        let p = resolve(
            r#"{
            "base_task":{"base":true,"exec":"base","env":{"vars":{"A":"1"}}},
            "derived":{"extends":"base_task","env":{"vars":{"B":"2"}}}}"#,
        );

        assert!(p.config.tasks.as_map().get("base_task").is_none());

        let derived = get_long(&p, "derived");
        assert_eq!(derived.exec, shell("base"));
        assert!(!derived.base);
        assert_eq!(derived.extends, None);

        let vars = derived.env.vars.as_ref().expect("env vars").to_map_inner();
        assert_eq!(vars.get("A").map(String::as_str), Some("1"));
        assert_eq!(vars.get("B").map(String::as_str), Some("2"));
    }

    #[test]
    fn test_extend_chain() {
        let p = resolve(
            r#"{"a":{"base":true,"exec":"a"},
            "b":{"base":true,"extends":["a"]},
            "c":{"extends":["b"]}}"#,
        );

        assert!(p.config.tasks.as_map().get("a").is_none());
        assert!(p.config.tasks.as_map().get("b").is_none());
        assert_eq!(get_long(&p, "c").exec, shell("a"));
    }

    #[test]
    fn test_diamond_dedup() {
        let p = resolve(
            r#"{"a":{"base":true,"exec":"a"},
            "b":{"base":true,"extends":["a"]},
            "c":{"base":true,"extends":["a"]},
            "d":{"extends":["b","c"]}}"#,
        );

        let d = get_long(&p, "d");
        assert_eq!(d.exec, shell("a"));
        assert!(!d.base);
        assert_eq!(d.extends, None);
    }

    #[test]
    fn test_base_marker_does_not_propagate_through_extends() {
        // `base` is a per-declaration template marker, never inherited: a
        // concrete task that extends a `base: true` template must itself be
        // emitted, not dropped.
        let p = resolve(
            r#"{"tmpl":{"base":true,"exec":"tmpl"},
            "concrete":{"extends":["tmpl"]}}"#,
        );

        assert!(
            p.config.tasks.as_map().get("tmpl").is_none(),
            "the base template must be dropped"
        );
        let concrete = get_long(&p, "concrete");
        assert!(!concrete.base, "the extender must not inherit `base`");
        assert_eq!(concrete.exec, shell("tmpl"));
    }

    #[test]
    fn test_empty_extends_applies_no_extension() {
        // `extends: []` is a real value that resolves to no extendees, so the
        // task keeps its own definition and is emitted standalone.
        let p = resolve(
            r#"{"a":{"base":true,"exec":"a"},
            "b":{"extends":[],"exec":"b"}}"#,
        );

        assert!(p.config.tasks.as_map().get("a").is_none());
        let b = get_long(&p, "b");
        assert_eq!(
            b.exec,
            shell("b"),
            "empty extends must not pull in any other task"
        );
        assert_eq!(b.extends, None);
    }

    #[test]
    fn test_cycle_is_rejected() {
        assert_eq!(
            extension_error(r#"{"a":{"extends":["b"]},"b":{"extends":["a"]}}"#),
            TaskExtensionErrorKind::Cycle
        );
    }

    #[test]
    fn test_unknown_extendee_is_rejected() {
        assert_eq!(
            extension_error(r#"{"a":{"extends":["missing"],"exec":"a"}}"#),
            TaskExtensionErrorKind::UnknownExtendee
        );
    }

    #[test]
    fn test_extending_non_base_keeps_target() {
        let p =
            resolve(r#"{"a":{"exec":"a"},"b":{"extends":["a"],"exec":"b"}}"#);

        assert!(p.config.tasks.as_map().get("a").is_some());
        assert!(p.config.tasks.as_map().get("b").is_some());
        assert_eq!(get_long(&p, "b").exec, shell("b"));
    }

    #[test]
    fn test_extending_short_form_base_is_lossless() {
        let p = resolve(
            r#"{"a":"echo a",
            "derived":{"extends":"a","env":{"vars":{"B":"2"}}}}"#,
        );

        let derived = get_long(&p, "derived");
        assert_eq!(derived.exec, shell("echo a"));
        // The short-form target's implicit upstream self-dependency survives.
        assert!(derived.dependencies.to_vec().iter().any(|d| matches!(
            d,
            TaskDependencyConfiguration::Upstream { task } if task == "a"
        )));
    }
}
