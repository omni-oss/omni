use std::{collections::HashMap, path::PathBuf};

use derive_more::Constructor;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Constructor, Deserialize, Serialize)]
pub struct Project {
    pub name: String,
    pub dir: PathBuf,
    pub dependencies: Vec<String>,
    pub tasks: HashMap<String, Task>,
}

#[derive(
    Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema, Constructor,
)]
pub struct Task {
    pub command: String,
    pub dependencies: Vec<TaskDependency>,
}

#[cfg(test)]
pub(crate) struct TaskBuilder {
    command: String,
    dependencies: Vec<TaskDependency>,
}

#[cfg(test)]
impl TaskBuilder {
    pub fn new(command: String) -> Self {
        Self {
            command,
            dependencies: Default::default(),
        }
    }

    pub fn own_dependency(mut self, task: impl Into<String>) -> Self {
        self.dependencies
            .push(TaskDependency::Own { task: task.into() });
        self
    }

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

    pub fn upstream_dependency(mut self, task: impl Into<String>) -> Self {
        self.dependencies
            .push(TaskDependency::Upstream { task: task.into() });
        self
    }

    pub fn build(self) -> Task {
        Task {
            command: self.command,
            dependencies: self.dependencies,
        }
    }
}

#[cfg(test)]
pub(crate) struct TasksBuilder {
    tasks: HashMap<String, Task>,
}

#[cfg(test)]
impl TasksBuilder {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
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

    pub fn build(self) -> HashMap<String, Task> {
        self.tasks
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub enum TaskDependency {
    Own { task: String },
    ExplicitProject { project: String, task: String },
    Upstream { task: String },
}
