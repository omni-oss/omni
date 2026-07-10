use std::{path::PathBuf, time::Duration};

use derive_new::new;
use maps::OrderedMap;
use omni_command_config::CommandConfig;
use omni_config_types::TeraExprBoolean;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, new)]
pub struct Project {
    #[new(into)]
    pub name: String,
    pub dir: PathBuf,
    pub dependencies: Vec<String>,
    pub tasks: OrderedMap<String, Task>,
}

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
    pub max_retries: Option<u8>,
    pub retry_interval: Option<Duration>,
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
    max_retries: Option<u8>,
    retry_interval: Option<Duration>,
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
            max_retries: None,
            retry_interval: None,
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
            max_retries: self.max_retries,
            retry_interval: self.retry_interval,
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
            "max_retries": null,
            "retry_interval": null
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
