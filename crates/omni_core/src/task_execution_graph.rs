use std::{
    collections::{HashMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher as _},
    path::{Path, PathBuf},
};

use derive_more::Constructor;
use petgraph::{
    Direction,
    algo::is_cyclic_directed,
    graph::{DiGraph, NodeIndex},
    visit::{Dfs, Topo, Walker},
};
use serde::{Deserialize, Serialize};

use crate::{Project, ProjectGraph, ProjectGraphError};

#[derive(
    Debug,
    Clone,
    Constructor,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Deserialize,
    Serialize,
)]
pub struct TaskExecutionNode {
    task_name: String,
    task_command: String,
    project_name: String,
    project_dir: PathBuf,
}

impl TaskExecutionNode {
    pub fn task_name(&self) -> &str {
        self.task_name.as_str()
    }

    pub fn task_command(&self) -> &str {
        self.task_command.as_str()
    }

    pub fn project_name(&self) -> &str {
        self.project_name.as_str()
    }

    pub fn project_dir(&self) -> &Path {
        self.project_dir.as_path()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Copy)]
pub struct TaskKey(u64);

impl TaskKey {
    pub fn new(project: &str, task: &str) -> Self {
        let mut hasher = DefaultHasher::new();
        project.hash(&mut hasher);
        task.hash(&mut hasher);
        let hashed = hasher.finish();

        Self(hashed)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TaskExecutionGraph {
    node_map: HashMap<TaskKey, NodeIndex>,
    di_graph: DiGraph<TaskExecutionNode, ()>,
}

impl TaskExecutionGraph {
    pub fn new() -> Self {
        Self {
            node_map: HashMap::new(),
            di_graph: DiGraph::new(),
        }
    }

    pub fn from_project_graph(
        project_graph: &ProjectGraph,
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
                    task_name.to_string(),
                    task.1.command.clone(),
                    project_name.to_string(),
                    project_dir.to_path_buf(),
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
                            let k = TaskKey::new(&project.name, task);

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
    task_graph: &mut TaskExecutionGraph,
    project: &&Project,
    dependent_key: &TaskKey,
    task: &str,
) -> Result<(), TaskExecutionGraphError> {
    let dependencies =
        project_graph.get_direct_dependencies_by_name(&project.name)?;

    if dependencies.is_empty() {
        return Ok(());
    }

    for (_, p) in dependencies.iter() {
        if p.tasks.contains_key(task) {
            let k = TaskKey::new(&p.name, task);

            if !task_graph.contains_dependency_by_key(dependent_key, &k)? {
                task_graph.add_edge_using_keys(dependent_key, &k)?;
            }
        } else {
            add_upstream_dependencies(
                project_graph,
                task_graph,
                &p,
                dependent_key,
                task,
            )?;
        };
    }
    Ok(())
}

pub type BatchedExecutionPlan = Vec<Vec<TaskExecutionNode>>;

impl TaskExecutionGraph {
    fn contains_dependency_by_key(
        &self,
        dependent_key: &TaskKey,
        dependee_key: &TaskKey,
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
        dependent_key: &TaskKey,
        dependee_key: &TaskKey,
    ) -> TaskExecutionGraphResult<()> {
        let a_idx = self.get_task_index_using_key(dependee_key)?;
        let b_idx = self.get_task_index_using_key(dependent_key)?;

        let edge_idx = self.di_graph.add_edge(a_idx, b_idx, ());

        if is_cyclic_directed(&self.di_graph) {
            self.di_graph.remove_edge(edge_idx);
            let dependee = self.di_graph[b_idx].clone();
            let dependent = self.di_graph[a_idx].clone();

            return Err(TaskExecutionGraphError::cyclic_dependency(
                dependent.project_name(),
                dependent.task_name(),
                dependee.project_name(),
                dependee.task_name(),
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
    ) -> TaskExecutionGraphResult<&TaskExecutionNode> {
        self.get_task_using_key(&TaskKey::new(project, task))
    }

    #[inline(always)]
    pub fn get_task(
        &self,
        node_index: NodeIndex,
    ) -> TaskExecutionGraphResult<&TaskExecutionNode> {
        Ok(&self.di_graph[node_index])
    }

    #[inline(always)]
    pub fn get_task_using_key(
        &self,
        key: &TaskKey,
    ) -> TaskExecutionGraphResult<&TaskExecutionNode> {
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
        key: &TaskKey,
    ) -> TaskExecutionGraphResult<NodeIndex> {
        self.node_map
            .get(key)
            .copied()
            .ok_or_else(|| TaskExecutionGraphError::task_not_found_by_key(key))
    }

    #[inline(always)]
    pub fn get_direct_dependencies_by_name(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> TaskExecutionGraphResult<Vec<(NodeIndex, TaskExecutionNode)>> {
        let task_key = TaskKey::new(project_name, task_name);

        self.get_direct_dependencies_by_key(&task_key)
    }

    #[inline(always)]
    pub fn get_direct_dependencies_by_key(
        &self,
        key: &TaskKey,
    ) -> TaskExecutionGraphResult<Vec<(NodeIndex, TaskExecutionNode)>> {
        let task_index = self.get_task_index_using_key(key)?;

        self.get_direct_dependencies(task_index)
    }

    pub fn get_direct_dependencies(
        &self,
        task_index: NodeIndex,
    ) -> TaskExecutionGraphResult<Vec<(NodeIndex, TaskExecutionNode)>> {
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

    #[cfg_attr(
        feature = "enable-tracing",
        tracing::instrument(level = "trace", skip_all)
    )]
    pub fn get_batched_execution_plan(
        &self,
        is_root_node: impl Fn(&TaskExecutionNode) -> bool,
    ) -> TaskExecutionGraphResult<BatchedExecutionPlan> {
        let mut roots = HashSet::new();

        // Step 1: Get all nodes that match the predicate
        for node in self.di_graph.node_indices() {
            if is_root_node(&self.di_graph[node]) {
                roots.insert(node);
            }
        }

        let mut filtered = vec![];

        let graph = &self.di_graph;
        // Step 2: Filter out nodes that are direct or indirect dependencies of other nodes
        for i in roots.iter() {
            let other_roots = roots
                .difference(&HashSet::from([*i]))
                .copied()
                .collect::<HashSet<_>>();
            let dfs = Dfs::new(&graph, *i);

            if dfs.iter(&graph).any(|n| other_roots.contains(&n)) {
                continue;
            }

            filtered.push(*i);
        }

        // Step 3: Get all reachable nodes based from filtered roots
        let mut reachable = HashSet::new();
        let mut stack = filtered.clone();
        while let Some(i) = stack.pop() {
            if reachable.insert(i) {
                for n in
                    self.di_graph.neighbors_directed(i, Direction::Incoming)
                {
                    stack.push(n);
                }
            }
        }

        // Step 4: Assign levels = max(level of predecessors) + 1
        let mut levels = HashMap::new();
        let mut topo = Topo::new(&self.di_graph);

        while let Some(node) = topo.next(&self.di_graph) {
            if !reachable.contains(&node) {
                continue;
            }

            let level = self
                .di_graph
                .neighbors_directed(node, Direction::Incoming)
                .filter(|n| reachable.contains(n))
                .map(|n| levels.get(&n).copied().unwrap_or(0))
                .max()
                .unwrap_or(0);

            levels.insert(node, level + 1);
        }

        // Step 5: Group nodes by level
        let mut batches: HashMap<usize, Vec<NodeIndex>> = HashMap::new();

        for (node, level) in levels {
            batches.entry(level).or_default().push(node);
        }

        // Step 6: Collect sorted batches
        let mut ordered_batches = Vec::new();
        let mut levels = batches.keys().copied().collect::<Vec<_>>();
        levels.sort();

        for level in levels {
            ordered_batches.push(batches.get(&level).unwrap().clone());
        }

        Ok(ordered_batches
            .into_iter()
            .map(|batch| {
                batch
                    .into_iter()
                    .map(|node| self.di_graph[node].clone())
                    .collect()
            })
            .collect())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("TaskGraphError: {source}")]
pub struct TaskExecutionGraphError {
    kind: TaskExecutionGraphErrorKind,
    #[source]
    source: TaskExecutionGraphErrorInner,
}

impl TaskExecutionGraphError {
    #[doc(hidden)]
    pub fn project_graph(source: ProjectGraphError) -> Self {
        Self {
            kind: TaskExecutionGraphErrorKind::ProjectGraph,
            source: TaskExecutionGraphErrorInner::ProjectGraph(source),
        }
    }

    #[doc(hidden)]
    pub fn task_not_found(project: &str, task: &str) -> Self {
        Self {
            kind: TaskExecutionGraphErrorKind::TaskNotFound,
            source: TaskExecutionGraphErrorInner::TaskNotFound {
                project: project.to_string(),
                task: task.to_string(),
            },
        }
    }

    #[doc(hidden)]
    pub fn task_not_found_by_key(key: &TaskKey) -> Self {
        Self {
            kind: TaskExecutionGraphErrorKind::TaskNotFoundByKey,
            source: TaskExecutionGraphErrorInner::TaskNotFoundByKey {
                key: *key,
            },
        }
    }

    #[doc(hidden)]
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
    TaskNotFoundByKey,
    CyclicDependency,
    ProjectGraph,
}

#[derive(Debug, thiserror::Error)]
enum TaskExecutionGraphErrorInner {
    #[error(transparent)]
    ProjectGraph(#[from] ProjectGraphError),

    #[error("Task '{task}' in project '{project}' not found")]
    TaskNotFound { project: String, task: String },

    #[error("Task with key '{key:?}' not found")]
    TaskNotFoundByKey { key: TaskKey },

    #[error(
        "Cyclic dependency detected from '{from_project}#{from_task}' to '{to_project}#{to_task}'"
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
    use std::path::PathBuf;

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
                .task("p1t4", "echo p1t4", |b| {
                    b.explicit_project_dependency("project3", "p3t1")
                        .own_dependency("shared-task-3")
                })
                .task("shared-task-3", "echo shared-task-3", |b| {
                    b.upstream_dependency("shared-task-3")
                })
                .build(),
            ..create_project("project1")
        };

        let project2 = Project {
            dependencies: vec![dep("project3")],
            tasks: TasksBuilder::new()
                .task("shared-task", "echo shared-task", |b| {
                    b.upstream_dependency("shared-task")
                })
                .task("p2t1", "echo p2t1", |b| b)
                .task("shared-task-3", "echo shared-task-3", |b| {
                    b.explicit_project_dependency("project3", "shared-task-3")
                        .own_dependency("p2t1")
                })
                .build(),
            ..create_project("project2")
        };

        let project3 = Project {
            dependencies: vec![dep("project4")],
            tasks: TasksBuilder::new()
                .task("p3t1", "echo p3t1", |b| b)
                .task("shared-task-2", "echo shared-task-2", |b| b)
                .task("shared-task", "echo shared-task", |b| b)
                .task("shared-task-3", "echo shared-task-3", |b| {
                    b.upstream_dependency("shared-task-3")
                })
                .build(),
            ..create_project("project3")
        };

        let project4 = Project {
            tasks: TasksBuilder::new()
                .task("p4t1", "echo p4t1", |b| b)
                .task("shared-task-3", "echo shared-task-3", |b| b)
                .build(),
            ..create_project("project4")
        };

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

        assert_eq!(task_graph.count(), 14, "Should have 14 nodes");
    }

    #[test]
    fn test_own_dependency_handling() {
        let project_graph = create_project_graph();
        let task_graph =
            TaskExecutionGraph::from_project_graph(&project_graph).unwrap();

        let p1t1_dependencies = task_graph
            .get_direct_dependencies_by_name("project1", "p1t1")
            .unwrap();

        assert_eq!(p1t1_dependencies.len(), 1);

        let p1t1_dependency = &p1t1_dependencies[0];
        assert_eq!(
            p1t1_dependency.0,
            task_graph.get_task_index("project1", "p1t2").unwrap()
        );
        assert_eq!(p1t1_dependency.1.task_name, "p1t2");
    }

    #[test]
    fn test_explicit_project_dependency_handling() {
        let project_graph = create_project_graph();
        let task_graph =
            TaskExecutionGraph::from_project_graph(&project_graph).unwrap();

        let p1t4_dependencies = task_graph
            .get_direct_dependencies_by_name("project1", "p1t4")
            .unwrap();

        assert_eq!(p1t4_dependencies.len(), 2);

        let p1t4_dependency_1 = &p1t4_dependencies
            .iter()
            .find(|d| d.1.task_name == "p3t1")
            .unwrap();
        assert_eq!(
            p1t4_dependency_1.0,
            task_graph.get_task_index("project3", "p3t1").unwrap()
        );
        assert_eq!(p1t4_dependency_1.1.task_name, "p3t1");
        assert_eq!(p1t4_dependency_1.1.project_name, "project3");

        let p1t4_dependency_2 = &p1t4_dependencies
            .iter()
            .find(|d| {
                d.1.project_name == "project1"
                    && d.1.task_name == "shared-task-3"
            })
            .unwrap();
        assert_eq!(
            p1t4_dependency_2.0,
            task_graph
                .get_task_index("project1", "shared-task-3")
                .unwrap()
        );
        assert_eq!(p1t4_dependency_2.1.task_name, "shared-task-3");
        assert_eq!(p1t4_dependency_2.1.project_name, "project1");
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

    #[test]
    fn test_batched_execution_plan() {
        let project_graph = create_project_graph();
        let task_graph =
            TaskExecutionGraph::from_project_graph(&project_graph).unwrap();

        let mut actual_plan = task_graph
            .get_batched_execution_plan(|n| n.task_name == "p1t4")
            .unwrap();

        actual_plan.iter_mut().for_each(|batch| {
            batch.sort();
        });

        let blank_path = PathBuf::from("");

        let mut expected_plan = vec![
            vec![
                TaskExecutionNode {
                    task_name: "p3t1".to_string(),
                    task_command: "echo p3t1".to_string(),
                    project_name: "project3".to_string(),
                    project_dir: blank_path.clone(),
                },
                TaskExecutionNode {
                    task_name: "p2t1".to_string(),
                    task_command: "echo p2t1".to_string(),
                    project_name: "project2".to_string(),
                    project_dir: blank_path.clone(),
                },
                TaskExecutionNode {
                    task_name: "shared-task-3".to_string(),
                    task_command: "echo shared-task-3".to_string(),
                    project_name: "project4".to_string(),
                    project_dir: blank_path.clone(),
                },
            ],
            vec![TaskExecutionNode {
                task_name: "shared-task-3".to_string(),
                task_command: "echo shared-task-3".to_string(),
                project_name: "project3".to_string(),
                project_dir: blank_path.clone(),
            }],
            vec![TaskExecutionNode {
                task_name: "shared-task-3".to_string(),
                task_command: "echo shared-task-3".to_string(),
                project_name: "project2".to_string(),
                project_dir: blank_path.clone(),
            }],
            vec![TaskExecutionNode {
                task_name: "shared-task-3".to_string(),
                task_command: "echo shared-task-3".to_string(),
                project_name: "project1".to_string(),
                project_dir: blank_path.clone(),
            }],
            vec![TaskExecutionNode {
                task_name: "p1t4".to_string(),
                task_command: "echo p1t4".to_string(),
                project_name: "project1".to_string(),
                project_dir: blank_path.clone(),
            }],
        ];

        expected_plan.iter_mut().for_each(|batch| {
            batch.sort();
        });

        for (i, batch) in actual_plan.iter().enumerate() {
            for (j, task) in batch.iter().enumerate() {
                assert_eq!(task, &expected_plan[i][j]);
            }
        }
    }
}
