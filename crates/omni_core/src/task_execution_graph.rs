use std::{collections::HashMap, path::Path};

use derive_more::Constructor;
use petgraph::graph::{DiGraph, NodeIndex};

use crate::{ProjectGraph, ProjectGraphError};

#[derive(Debug, Clone, Constructor)]
pub struct TaskExecutionNodeRef<'a> {
    pub task_name: &'a str,
    pub project_name: &'a str,
    pub project_dir: &'a Path,
    pub is_transport: bool,
}

#[derive(Debug, Default)]
pub struct TaskExecutionGraph<'a> {
    node_map: HashMap<(&'a str, &'a str), NodeIndex>,
    di_graph: DiGraph<TaskExecutionNodeRef<'a>, ()>,
}

impl<'a> TaskExecutionGraph<'a> {
    pub fn new() -> Self {
        Self {
            node_map: HashMap::new(),
            di_graph: DiGraph::new(),
        }
    }

    pub fn from_project_graph(
        project_graph: &'a ProjectGraph,
    ) -> TaskGraphResult<Self> {
        let mut graph = Self::new();

        let projects = project_graph.get_projects_toposorted()?;

        // add all nodes first before adding edges
        for project in projects.iter() {
            let project_name = project.name.as_str();
            let project_dir = project.dir.as_path();

            for task in project.tasks.iter() {
                let task_name = task.0.as_str();
                let task_execution_node = TaskExecutionNodeRef::new(
                    task_name,
                    project_name,
                    project_dir,
                    false,
                );

                let node_index = graph.di_graph.add_node(task_execution_node);
                graph.node_map.insert((project_name, task_name), node_index);
            }
        }

        // add edges
        // for project in projects.iter() {
        //     for task in project.tasks.iter() {}
        // }

        Ok(graph)
    }
}

impl<'a> TaskExecutionGraph<'a> {
    #[inline(always)]
    pub fn count(&self) -> usize {
        self.di_graph.node_count()
    }
}

impl<'a> TaskExecutionGraph<'a> {}

#[derive(Debug, thiserror::Error)]
#[error("TaskGraphError: {source}")]
pub struct TaskGraphError {
    kind: TaskGraphErrorKind,
    #[source]
    source: TaskGraphErrorInner,
}

impl TaskGraphError {
    pub fn project_graph(source: ProjectGraphError) -> Self {
        Self {
            kind: TaskGraphErrorKind::ProjectGraph,
            source: TaskGraphErrorInner::ProjectGraph(source),
        }
    }
}

impl From<ProjectGraphError> for TaskGraphError {
    fn from(source: ProjectGraphError) -> Self {
        Self::project_graph(source)
    }
}

impl TaskGraphError {
    pub fn kind(&self) -> TaskGraphErrorKind {
        self.kind
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TaskGraphErrorKind {
    ProjectGraph,
}

#[derive(Debug, thiserror::Error)]
enum TaskGraphErrorInner {
    #[error(transparent)]
    ProjectGraph(#[from] ProjectGraphError),
}

pub type TaskGraphResult<T> = Result<T, TaskGraphError>;

#[cfg(test)]
mod tests {
    use crate::{Project, TasksBuilder};

    use super::*;

    fn create_project(name: &str) -> Project {
        Project {
            name: name.to_string(),
            dir: Default::default(),
            dependencies: Default::default(),
            tasks: Default::default(),
        }
    }

    fn create_project_graph() -> ProjectGraph {
        fn dep(name: &str) -> String {
            name.to_string()
        }

        let project1 = Project {
            dependencies: vec![dep("project2"), dep("project3")],
            tasks: TasksBuilder::new()
                .task("p1t1", "echo p1t1", |b| b.own_dependency("p1t2"))
                .task("p1t2", "echo p1t2", |b| {
                    b.upstream_dependency("shared-task")
                })
                .task("p1t3", "echo p1t2", |b| {
                    b.upstream_dependency("shared-task-2")
                })
                .task("p1t4", "echo p1t2", |b| {
                    b.explicit_project_dependency("project3", "p3t1")
                })
                .build(),
            ..create_project("project1")
        };

        let project2 = Project {
            dependencies: vec![dep("project3")],
            tasks: TasksBuilder::new()
                .task("shared-task", "echo shared-task", |b| b)
                .task("p3t1", "echo p3t1", |b| b)
                .build(),
            ..create_project("project2")
        };

        let project3 = Project {
            dependencies: vec![dep("project4")],
            tasks: TasksBuilder::new()
                .task("shared-task-2", "echo shared-task-2", |b| b)
                .task("shared-task", "echo shared-task", |b| b)
                .build(),
            ..create_project("project3")
        };

        let project4 = create_project("project4");

        ProjectGraph::from_projects(vec![
            project1, project2, project3, project4,
        ])
        .expect("Can't create graph")
    }

    #[test]
    fn test_node_count_from_project_graph() {
        let project_graph = create_project_graph();
        let task_graph =
            TaskExecutionGraph::from_project_graph(&project_graph).unwrap();

        assert_eq!(task_graph.count(), 8, "Should have 8 nodes");
    }
}
