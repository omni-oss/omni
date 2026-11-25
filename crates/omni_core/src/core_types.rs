use std::path::PathBuf;

use derive_new::new;
use maps::OrderedMap;
use omni_command_config::CommandConfig;
use omni_config_types::TeraExprBoolean;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use omni_retry_config::RetryConfiguration;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, new)]
pub struct Project {
    #[new(into)]
    pub name: String,
    pub dir: PathBuf,
    pub dependencies: Vec<String>,
    pub tasks: OrderedMap<String, Task>,
    /// Authored aliases of this project's tasks. They are not tasks: they only
    /// name a canonical entry of `tasks`.
    #[new(default)]
    #[serde(skip)]
    pub aliases: TaskAliases,
}

impl Project {
    /// Attach the resolved alias index.
    pub fn with_aliases(mut self, aliases: TaskAliases) -> Self {
        self.aliases = aliases;
        self
    }

    /// The canonical task name a public name (a task or an alias) refers to
    /// in this project. Exact task names win; an alias resolves through its
    /// index entry. `None` when the project has no such public name.
    pub fn canonical_task_name<'a>(&'a self, name: &'a str) -> Option<&'a str> {
        if let Some((canonical, _)) = self.tasks.get_key_value(name) {
            return Some(canonical.as_str());
        }
        let canonical = self.aliases.resolve(name)?;
        self.tasks.contains_key(canonical).then_some(canonical)
    }

    /// Whether `name` is an alias of this project (and not a task).
    pub fn is_alias(&self, name: &str) -> bool {
        !self.tasks.contains_key(name) && self.aliases.resolve(name).is_some()
    }
}

/// The resolved aliases of one project: each alias name maps to the canonical
/// task name it stands for. Chains are already flattened, so a lookup is one
/// step. This belongs to one resolved project; it is not part of any cache
/// key, entry or stored name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskAliases {
    canonical_by_alias: OrderedMap<String, String>,
}

impl TaskAliases {
    pub fn new(canonical_by_alias: OrderedMap<String, String>) -> Self {
        Self { canonical_by_alias }
    }

    pub fn is_empty(&self) -> bool {
        self.canonical_by_alias.is_empty()
    }

    pub fn len(&self) -> usize {
        self.canonical_by_alias.len()
    }

    /// The canonical task name of `alias`, if it is one.
    pub fn resolve(&self, alias: &str) -> Option<&str> {
        self.canonical_by_alias.get(alias).map(String::as_str)
    }

    /// `(alias, canonical)` pairs in alias-name order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.canonical_by_alias
            .iter()
            .map(|(alias, canonical)| (alias.as_str(), canonical.as_str()))
    }
}

#[allow(clippy::too_many_arguments)]
#[derive(
    Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema, new,
)]
pub struct Task {
    pub exec: Option<CommandConfig>,
    pub retry_exec: Option<CommandConfig>,
    pub dependencies: Vec<TaskDependency>,
    pub description: Option<String>,
    pub enabled: TeraExprBoolean,
    pub interactive: bool,
    pub persistent: bool,
    pub siblings: Vec<TaskDependency>,
    #[serde(default)]
    pub retry: RetryConfiguration,
}

#[cfg(test)]
pub(crate) struct TaskBuilder {
    exec: Option<CommandConfig>,
    retry_exec: Option<CommandConfig>,
    dependencies: Vec<TaskDependency>,
    description: Option<String>,
    enabled: TeraExprBoolean,
    interactive: bool,
    persistent: bool,
    siblings: Vec<TaskDependency>,
    retry: RetryConfiguration,
}

#[cfg(test)]
impl TaskBuilder {
    pub fn new(exec: String) -> Self {
        Self {
            exec: Some(CommandConfig::Shell(exec)),
            retry_exec: None,
            dependencies: Default::default(),
            description: Default::default(),
            enabled: TeraExprBoolean::new_boolean(true),
            interactive: false,
            persistent: false,
            siblings: Default::default(),
            retry: Default::default(),
        }
    }

    #[allow(unused)]
    pub fn own_dependency(mut self, task: impl Into<String>) -> Self {
        self.dependencies
            .push(TaskDependency::Own { task: task.into() });
        self
    }

    #[allow(unused)]
    pub fn explicit_project_dependency(
        mut self,
        project: impl Into<String>,
        task: impl Into<String>,
    ) -> Self {
        self.dependencies.push(TaskDependency::ExplicitProject {
            project: project.into(),
            task: task.into(),
        });
        self
    }

    #[allow(unused)]
    pub fn upstream_dependency(mut self, task: impl Into<String>) -> Self {
        self.dependencies
            .push(TaskDependency::Upstream { task: task.into() });
        self
    }

    #[allow(unused)]
    pub fn own_sibling(mut self, task: impl Into<String>) -> Self {
        self.siblings
            .push(TaskDependency::Own { task: task.into() });
        self
    }

    #[allow(unused)]
    pub fn explicit_project_sibling(
        mut self,
        project: impl Into<String>,
        task: impl Into<String>,
    ) -> Self {
        self.siblings.push(TaskDependency::ExplicitProject {
            project: project.into(),
            task: task.into(),
        });
        self
    }

    #[allow(unused)]
    pub fn upstream_sibling(mut self, task: impl Into<String>) -> Self {
        self.siblings
            .push(TaskDependency::Upstream { task: task.into() });
        self
    }

    #[allow(unused)]
    pub fn enabled(mut self, enabled: TeraExprBoolean) -> Self {
        self.enabled = enabled;
        self
    }

    #[allow(unused)]
    pub fn persistent(mut self, persistent: bool) -> Self {
        self.persistent = persistent;
        self
    }

    #[allow(unused)]
    pub fn interactive(mut self, interactive: bool) -> Self {
        self.interactive = interactive;
        self
    }

    #[allow(unused)]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn build(self) -> Task {
        Task {
            exec: self.exec,
            retry_exec: self.retry_exec,
            dependencies: self.dependencies,
            description: self.description,
            enabled: self.enabled,
            interactive: self.interactive,
            persistent: self.persistent,
            siblings: self.siblings,
            retry: self.retry,
        }
    }
}

#[cfg(test)]
pub(crate) struct TasksBuilder {
    tasks: OrderedMap<String, Task>,
}

#[cfg(test)]
impl TasksBuilder {
    pub fn new() -> Self {
        use maps::ordered_map;

        Self {
            tasks: ordered_map!(),
        }
    }

    pub fn task(
        mut self,
        name: impl Into<String>,
        exec: impl Into<String>,
        build_task: impl FnOnce(TaskBuilder) -> TaskBuilder,
    ) -> Self {
        let task = build_task(TaskBuilder::new(exec.into())).build();

        self.tasks.insert(name.into(), task);
        self
    }

    pub fn build(self) -> OrderedMap<String, Task> {
        self.tasks
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub enum TaskDependency {
    Own { task: String },
    ExplicitProject { project: String, task: String },
    Upstream { task: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_round_trips_shell_and_argv_exec() {
        let json = r#"{
            "exec": "echo hi",
            "retry_exec": ["echo", "retry hi"],
            "dependencies": [],
            "description": null,
            "enabled": true,
            "interactive": false,
            "persistent": false,
            "siblings": [],
            "retry": {}
        }"#;

        let task: Task = serde_json::from_str(json).unwrap();
        assert_eq!(
            task.exec,
            Some(CommandConfig::Shell("echo hi".to_string()))
        );
        assert_eq!(
            task.retry_exec,
            Some(CommandConfig::Argv(vec![
                "echo".to_string(),
                "retry hi".to_string(),
            ]))
        );

        // Serialize then deserialize again; the value must be stable.
        let reserialized = serde_json::to_string(&task).unwrap();
        let round_tripped: Task = serde_json::from_str(&reserialized).unwrap();
        assert_eq!(task, round_tripped);
    }
}
