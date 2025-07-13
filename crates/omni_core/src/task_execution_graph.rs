use std::{collections::HashMap, path::Path};

use derive_more::Constructor;
use petgraph::{
    Direction,
    algo::is_cyclic_directed,
    graph::{DiGraph, NodeIndex},
};

use crate::{Project, ProjectGraph, ProjectGraphError};

#[derive(Debug, Clone, Constructor)]
pub struct TaskExecutionNode<'a> {
    pub task_name: &'a str,
    pub project_name: &'a str,
    pub project_dir: &'a Path,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Constructor)]
pub struct TaskKey<'a> {
    project: &'a str,
    task: &'a str,
}

#[derive(Debug, Default)]
pub struct TaskExecutionGraph<'a> {
    node_map: HashMap<TaskKey<'a>, NodeIndex>,
    di_graph: DiGraph<TaskExecutionNode<'a>, ()>,
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
    ) -> TaskExecutionGraphResult<Self> {
        let mut graph = Self::new();

        let projects = project_graph.get_projects_toposorted()?;

        // add all nodes first before adding edges
        for project in projects.iter() {
            let project_name = project.name.as_str();
            let project_dir = project.dir.as_path();

            for task in project.tasks.iter() {
                let task_name = task.0.as_str();
                let task_execution_node = TaskExecutionNode::new(
                    task_name,
                    project_name,
                    project_dir,
                );

                let node_index = graph.di_graph.add_node(task_execution_node);
                graph
                    .node_map
                    .insert(TaskKey::new(project_name, task_name), node_index);
            }
        }

        // add edges
        for project in projects.iter() {
            for task in project.tasks.iter() {
                let tname = task.0.as_str();
                let pname = project.name.as_str();
                let dependent_key = TaskKey::new(pname, tname);

                for dependency in task.1.dependencies.iter() {
                    match dependency {
                        crate::TaskDependency::Own { task } => {
                            let k = TaskKey::new(project.name.as_str(), task);

                            graph.add_edge_using_keys(&dependent_key, &k)?;
                        }
                        crate::TaskDependency::ExplicitProject {
                            project,
                            task,
                        } => {
                            let k = TaskKey::new(project, task);
                            graph.add_edge_using_keys(&dependent_key, &k)?;
                        }
                        crate::TaskDependency::Upstream { task } => {
                            add_upstream_dependencies(
                                project_graph,
                                &mut graph,
                                project,
                                &dependent_key,
                                task,
                            )?;
                        }
                    };
                }
            }
        }

        Ok(graph)
    }
}

fn add_upstream_dependencies(
    project_graph: &ProjectGraph,
    task_graph: &mut TaskExecutionGraph<'_>,
    project: &&Project,
    dependent_key: &TaskKey<'_>,
    task: &str,
) -> Result<(), TaskExecutionGraphError> {
    let dependencies =
        project_graph.get_direct_dependencies_by_name(&project.name)?;

    if dependencies.is_empty() {
        return Ok(());
    }

    for (_, p) in dependencies.iter() {
        let dependent_key = if p.tasks.contains_key(task) {
            let k = TaskKey::new(p.name.as_str(), task);

            if !task_graph.contains_dependency_by_key(dependent_key, &k)? {
                task_graph.add_edge_using_keys(dependent_key, &k)?;
            }

            &TaskKey::new(p.name.as_str(), task)
        } else {
            dependent_key
        };
        add_upstream_dependencies(
            project_graph,
            task_graph,
            &p,
            dependent_key,
            task,
        )?;
    }
    Ok(())
}

impl<'a> TaskExecutionGraph<'a> {
    fn contains_dependency_by_key(
        &self,
        dependent_key: &TaskKey<'_>,
        dependee_key: &TaskKey<'_>,
    ) -> TaskExecutionGraphResult<bool> {
        let dependent_idx = self.get_task_index_using_key(dependent_key)?;
        let dependee_idx = self.get_task_index_using_key(dependee_key)?;

        self.contains_dependency(dependent_idx, dependee_idx)
    }

    fn contains_dependency(
        &self,
        dependent_idx: NodeIndex,
        dependee_idx: NodeIndex,
    ) -> TaskExecutionGraphResult<bool> {
        Ok(self.di_graph.contains_edge(dependee_idx, dependent_idx))
    }

    fn add_edge_using_keys(
        &mut self,
        dependent_key: &TaskKey<'_>,
        dependee_key: &TaskKey<'_>,
    ) -> TaskExecutionGraphResult<()> {
        let a_idx = self.get_task_index_using_key(dependee_key)?;
        let b_idx = self.get_task_index_using_key(dependent_key)?;

        let edge_idx = self.di_graph.add_edge(a_idx, b_idx, ());

        if is_cyclic_directed(&self.di_graph) {
            self.di_graph.remove_edge(edge_idx);
            return Err(TaskExecutionGraphError::cyclic_dependency(
                dependent_key.project,
                dependent_key.task,
                dependee_key.project,
                dependee_key.task,
            ));
        }

        Ok(())
    }

    #[inline(always)]
    pub fn count(&self) -> usize {
        self.di_graph.node_count()
    }

    #[inline(always)]
    pub fn get_task_using_names(
        &self,
        project: &str,
        task: &str,
    ) -> TaskExecutionGraphResult<&TaskExecutionNode<'a>> {
        self.get_task_using_key(&TaskKey::new(project, task))
    }

    #[inline(always)]
    pub fn get_task(
        &self,
        node_index: NodeIndex,
    ) -> TaskExecutionGraphResult<&TaskExecutionNode<'a>> {
        Ok(&self.di_graph[node_index])
    }

    #[inline(always)]
    pub fn get_task_using_key(
        &self,
        key: &TaskKey<'_>,
    ) -> TaskExecutionGraphResult<&TaskExecutionNode<'a>> {
        let t = self.get_task_index_using_key(key)?;

        Ok(&self.di_graph[t])
    }

    #[inline(always)]
    pub fn get_task_index(
        &self,
        project: &str,
        task: &str,
    ) -> TaskExecutionGraphResult<NodeIndex> {
        self.get_task_index_using_key(&TaskKey::new(project, task))
    }

    #[inline(always)]
    pub fn get_task_index_using_key(
        &self,
        key: &TaskKey<'_>,
    ) -> TaskExecutionGraphResult<NodeIndex> {
        self.node_map.get(key).copied().ok_or_else(|| {
            TaskExecutionGraphError::task_not_found(key.project, key.task)
        })
    }

    #[inline(always)]
    pub fn get_direct_dependencies_by_name(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> TaskExecutionGraphResult<Vec<(NodeIndex, TaskExecutionNode<'a>)>> {
        let task_key = TaskKey::new(project_name, task_name);

        self.get_direct_dependencies_by_key(&task_key)
    }

    #[inline(always)]
    pub fn get_direct_dependencies_by_key(
        &self,
        key: &TaskKey<'_>,
    ) -> TaskExecutionGraphResult<Vec<(NodeIndex, TaskExecutionNode<'a>)>> {
        let task_index = self.get_task_index_using_key(key)?;

        self.get_direct_dependencies(task_index)
    }

    pub fn get_direct_dependencies(
        &self,
        task_index: NodeIndex,
    ) -> TaskExecutionGraphResult<Vec<(NodeIndex, TaskExecutionNode<'a>)>> {
        let neighbors = self
            .di_graph
            .neighbors_directed(task_index, Direction::Incoming);

        Ok(neighbors
            .map(|ni| {
                let node = &self.di_graph[ni];
                (ni, node.clone())
            })
            .collect())
    }
}

impl<'a> TaskExecutionGraph<'a> {}

#[derive(Debug, thiserror::Error)]
#[error("TaskGraphError: {source}")]
pub struct TaskExecutionGraphError {
    kind: TaskExecutionGraphErrorKind,
    #[source]
    source: TaskExecutionGraphErrorInner,
}

impl TaskExecutionGraphError {
    pub fn project_graph(source: ProjectGraphError) -> Self {
        Self {
            kind: TaskExecutionGraphErrorKind::ProjectGraph,
            source: TaskExecutionGraphErrorInner::ProjectGraph(source),
        }
    }

    pub fn task_not_found(project: &str, task: &str) -> Self {
        Self {
            kind: TaskExecutionGraphErrorKind::TaskNotFound,
            source: TaskExecutionGraphErrorInner::TaskNotFound {
                project: project.to_string(),
                task: task.to_string(),
            },
        }
    }

    pub fn cyclic_dependency(
        from_project: &str,
        from_task: &str,
        to_project: &str,
        to_task: &str,
    ) -> Self {
        Self {
            kind: TaskExecutionGraphErrorKind::CyclicDependency,
            source: TaskExecutionGraphErrorInner::CyclicDependency {
                from_project: from_project.to_string(),
                from_task: from_task.to_string(),
                to_project: to_project.to_string(),
                to_task: to_task.to_string(),
            },
        }
    }
}

impl From<ProjectGraphError> for TaskExecutionGraphError {
    fn from(source: ProjectGraphError) -> Self {
        Self::project_graph(source)
    }
}

impl TaskExecutionGraphError {
    pub fn kind(&self) -> TaskExecutionGraphErrorKind {
        self.kind
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TaskExecutionGraphErrorKind {
    TaskNotFound,
    CyclicDependency,
    ProjectGraph,
}

#[derive(Debug, thiserror::Error)]
enum TaskExecutionGraphErrorInner {
    #[error(transparent)]
    ProjectGraph(#[from] ProjectGraphError),

    #[error("Task '{task}' in project '{project}' not found")]
    TaskNotFound { project: String, task: String },

    #[error(
        "Cyclic dependency detected from '{from_project}:{from_task}' to '{to_project}:{to_task}'"
    )]
    CyclicDependency {
        from_project: String,
        from_task: String,
        to_project: String,
        to_task: String,
    },
}

pub type TaskExecutionGraphResult<T> = Result<T, TaskExecutionGraphError>;

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
                .task("p2t1", "echo p2t1", |b| b)
                .build(),
            ..create_project("project2")
        };

        let project3 = Project {
            dependencies: vec![dep("project4")],
            tasks: TasksBuilder::new()
                .task("p3t1", "echo p3t1", |b| b)
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

        assert_eq!(task_graph.count(), 9, "Should have 8 nodes");
    }

    #[test]
    fn test_upstream_dependency_handling() {
        let project_graph = create_project_graph();

        let task_graph =
            TaskExecutionGraph::from_project_graph(&project_graph).unwrap();

        let p1t2_dependencies = task_graph
            .get_direct_dependencies_by_name("project1", "p1t2")
            .unwrap();

        let p1t3_dependencies = task_graph
            .get_direct_dependencies_by_name("project1", "p1t3")
            .unwrap();

        assert_eq!(p1t2_dependencies.len(), 2);

        let p1t2_dependency_1 = &p1t2_dependencies
            .iter()
            .find(|d| d.1.project_name == "project2")
            .expect("Should have dependency to project2");
        let p1t2_dependency_2 = &p1t2_dependencies
            .iter()
            .find(|d| d.1.project_name == "project3")
            .expect("Should have dependency to project3");

        assert_eq!(
            p1t2_dependency_1.0,
            task_graph
                .get_task_index("project2", "shared-task")
                .unwrap()
        );
        assert_eq!(p1t2_dependency_1.1.task_name, "shared-task");
        assert_eq!(p1t2_dependency_1.1.project_name, "project2");

        assert_eq!(
            p1t2_dependency_2.0,
            task_graph
                .get_task_index("project3", "shared-task")
                .unwrap()
        );
        assert_eq!(p1t2_dependency_2.1.task_name, "shared-task");
        assert_eq!(p1t2_dependency_2.1.project_name, "project3");

        assert_eq!(p1t3_dependencies.len(), 1);
        let p1t3_dependency = &p1t3_dependencies[0];
        assert_eq!(
            p1t3_dependency.0,
            task_graph
                .get_task_index("project3", "shared-task-2")
                .unwrap()
        );
        assert_eq!(p1t3_dependency.1.task_name, "shared-task-2");
        assert_eq!(p1t3_dependency.1.project_name, "project3");
    }
}
