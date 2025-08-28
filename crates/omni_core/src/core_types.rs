use std::path::PathBuf;

use derive_more::Constructor;
use derive_new::new;
use maps::OrderedMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Constructor, Deserialize, Serialize)]
pub struct Project {
    pub name: String,
    pub dir: PathBuf,
    pub dependencies: Vec<String>,
    pub tasks: OrderedMap<String, Task>,
}

#[derive(
    Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema, new,
)]
pub struct Task {
    pub command: String,
    pub dependencies: Vec<TaskDependency>,
    pub description: Option<String>,
    pub enabled: bool,
    pub interactive: bool,
    pub persistent: bool,
    pub siblings: Vec<TaskDependency>,
}

#[cfg(test)]
pub(crate) struct TaskBuilder {
    command: String,
    dependencies: Vec<TaskDependency>,
    description: Option<String>,
    enabled: bool,
    interactive: bool,
    persistent: bool,
    siblings: Vec<TaskDependency>,
}

#[cfg(test)]
impl TaskBuilder {
    pub fn new(command: String) -> Self {
        Self {
            command,
            dependencies: Default::default(),
            description: Default::default(),
            enabled: true,
            interactive: false,
            persistent: false,
            siblings: Default::default(),
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
    pub fn enabled(mut self, enabled: bool) -> Self {
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
            command: self.command,
            dependencies: self.dependencies,
            description: self.description,
            enabled: self.enabled,
            interactive: self.interactive,
            persistent: self.persistent,
            siblings: self.siblings,
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
        cmd: impl Into<String>,
        build_task: impl FnOnce(TaskBuilder) -> TaskBuilder,
    ) -> Self {
        let task = build_task(TaskBuilder::new(cmd.into())).build();

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
