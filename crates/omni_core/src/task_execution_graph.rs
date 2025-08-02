use std::{
    collections::{HashMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher as _},
    path::{Path, PathBuf},
};

use petgraph::{
    Direction,
    algo::is_cyclic_directed,
    graph::{DiGraph, NodeIndex},
    visit::{Dfs, Topo, Walker},
};
use serde::{Deserialize, Serialize};
use strum::{EnumDiscriminants, IntoDiscriminant};

use crate::{Project, ProjectGraph, ProjectGraphError};

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize,
)]
pub struct TaskExecutionNode {
    task_name: String,
    task_command: String,
    project_name: String,
    project_dir: PathBuf,
    full_task_name: String,
}

impl TaskExecutionNode {
    pub fn new(
        task_name: impl Into<String>,
        task_command: impl Into<String>,
        project_name: impl Into<String>,
        project_dir: impl Into<PathBuf>,
    ) -> Self {
        let project_name = project_name.into();
        let task_name = task_name.into();
        Self {
            full_task_name: format!("{}#{}", &project_name, &task_name),
            task_name,
            task_command: task_command.into(),
            project_name,
            project_dir: project_dir.into(),
        }
    }
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

    pub fn full_task_name(&self) -> &str {
        self.full_task_name.as_str()
    }

    /// (task_name, task_command, project_name, project_dir, full_task_name)
    pub fn deconstruct(self) -> (String, String, String, PathBuf, String) {
        (
            self.task_name,
            self.task_command,
            self.project_name,
            self.project_dir,
            self.full_task_name,
        )
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

                for dependency in task.1.dependencies.iter() {
                    match dependency {
                        crate::TaskDependency::Own { task } => {
                            graph.add_edge_by_names(
                                pname,
                                tname,
                                &project.name,
                                task,
                            )?;
                        }
                        crate::TaskDependency::ExplicitProject {
                            project,
                            task,
                        } => {
                            graph.add_edge_by_names(
                                pname, tname, project, task,
                            )?;
                        }
                        crate::TaskDependency::Upstream { task } => {
                            add_upstream_dependencies(
                                project_graph,
                                &mut graph,
                                project,
                                pname,
                                tname,
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
    dependent_project_name: &str,
    dependent_task_name: &str,
    task: &str,
) -> Result<(), TaskExecutionGraphError> {
    let dependencies =
        project_graph.get_direct_dependencies_by_name(&project.name)?;

    if dependencies.is_empty() {
        return Ok(());
    }

    for (_, p) in dependencies.iter() {
        if p.tasks.contains_key(task) {
            if !task_graph.contains_dependency_by_names(
                dependent_project_name,
                dependent_task_name,
                &p.name,
                task,
            )? {
                task_graph.add_edge_by_names(
                    dependent_project_name,
                    dependent_task_name,
                    &p.name,
                    task,
                )?;
            }
        } else {
            add_upstream_dependencies(
                project_graph,
                task_graph,
                &p,
                dependent_project_name,
                dependent_task_name,
                task,
            )?;
        };
    }
    Ok(())
}

pub type BatchedExecutionPlan = Vec<Vec<TaskExecutionNode>>;

impl TaskExecutionGraph {
    fn contains_dependency_by_names(
        &self,
        dependent_project_name: &str,
        dependent_task_name: &str,
        dependee_project_name: &str,
        dependee_task_name: &str,
    ) -> TaskExecutionGraphResult<bool> {
        let depedent = self.get_task_index_by_name(
            dependent_project_name,
            dependent_task_name,
        )?;
        let dependee = self.get_task_index_by_name(
            dependee_project_name,
            dependee_task_name,
        )?;

        self.contains_dependency(depedent, dependee)
    }

    fn contains_dependency(
        &self,
        dependent_idx: NodeIndex,
        dependee_idx: NodeIndex,
    ) -> TaskExecutionGraphResult<bool> {
        Ok(self.di_graph.contains_edge(dependee_idx, dependent_idx))
    }

    fn add_edge_by_names(
        &mut self,
        dependent_project_name: &str,
        dependent_task_name: &str,
        dependee_project_name: &str,
        dependee_task_name: &str,
    ) -> TaskExecutionGraphResult<()> {
        let a_idx = self.get_task_index_by_name(
            dependee_project_name,
            dependee_task_name,
        )?;
        let b_idx = self.get_task_index_by_name(
            dependent_project_name,
            dependent_task_name,
        )?;

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
    pub fn get_task_by_name(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> TaskExecutionGraphResult<&TaskExecutionNode> {
        self.get_task_by_key(&TaskKey::new(project_name, task_name))
            .map_err(|e| {
                if e.kind() == TaskExecutionGraphErrorKind::TaskNotFoundByKey {
                    TaskExecutionGraphError::task_not_found(
                        project_name,
                        task_name,
                    )
                } else {
                    e
                }
            })
    }

    #[inline(always)]
    pub fn get_task(
        &self,
        node_index: NodeIndex,
    ) -> TaskExecutionGraphResult<&TaskExecutionNode> {
        Ok(&self.di_graph[node_index])
    }

    #[inline(always)]
    fn get_task_by_key(
        &self,
        key: &TaskKey,
    ) -> TaskExecutionGraphResult<&TaskExecutionNode> {
        let t = self.get_task_index_by_key(key)?;

        Ok(&self.di_graph[t])
    }

    #[inline(always)]
    pub fn get_task_index_by_name(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> TaskExecutionGraphResult<NodeIndex> {
        self.get_task_index_by_key(&TaskKey::new(project_name, task_name))
            .map_err(|e| {
                if e.kind() == TaskExecutionGraphErrorKind::TaskNotFoundByKey {
                    TaskExecutionGraphError::task_not_found(
                        project_name,
                        task_name,
                    )
                } else {
                    e
                }
            })
    }

    #[inline(always)]
    fn get_task_index_by_key(
        &self,
        key: &TaskKey,
    ) -> TaskExecutionGraphResult<NodeIndex> {
        self.node_map
            .get(key)
            .copied()
            .ok_or_else(|| TaskExecutionGraphError::task_not_found_by_key(*key))
    }

    #[inline(always)]
    pub fn get_direct_dependencies_by_name(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> TaskExecutionGraphResult<Vec<(NodeIndex, TaskExecutionNode)>> {
        let task_key = TaskKey::new(project_name, task_name);

        self.get_direct_dependencies_by_key(&task_key).map_err(|e| {
            if e.kind() == TaskExecutionGraphErrorKind::TaskNotFoundByKey {
                TaskExecutionGraphError::task_not_found(project_name, task_name)
            } else {
                e
            }
        })
    }

    #[inline(always)]
    fn get_direct_dependencies_by_key(
        &self,
        key: &TaskKey,
    ) -> TaskExecutionGraphResult<Vec<(NodeIndex, TaskExecutionNode)>> {
        let task_index = self.get_task_index_by_key(key)?;

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
#[error("{inner}")]
pub struct TaskExecutionGraphError {
    kind: TaskExecutionGraphErrorKind,
    #[source]
    inner: TaskExecutionGraphErrorInner,
}

impl TaskExecutionGraphError {
    #[doc(hidden)]
    pub fn project_graph(source: ProjectGraphError) -> Self {
        Self {
            kind: TaskExecutionGraphErrorKind::ProjectGraph,
            inner: TaskExecutionGraphErrorInner::ProjectGraph(source),
        }
    }

    #[doc(hidden)]
    pub fn task_not_found(project: &str, task: &str) -> Self {
        Self {
            kind: TaskExecutionGraphErrorKind::TaskNotFound,
            inner: TaskExecutionGraphErrorInner::TaskNotFound {
                project: project.to_string(),
                task: task.to_string(),
            },
        }
    }

    #[doc(hidden)]
    pub fn task_not_found_by_key(key: TaskKey) -> Self {
        Self {
            kind: TaskExecutionGraphErrorKind::TaskNotFoundByKey,
            inner: TaskExecutionGraphErrorInner::TaskNotFoundByKey { key },
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
            inner: TaskExecutionGraphErrorInner::CyclicDependency {
                from_project: from_project.to_string(),
                from_task: from_task.to_string(),
                to_project: to_project.to_string(),
                to_task: to_task.to_string(),
            },
        }
    }
}

impl<T: Into<TaskExecutionGraphErrorInner>> From<T>
    for TaskExecutionGraphError
{
    fn from(value: T) -> Self {
        let repr = value.into();
        let kind = repr.discriminant();
        Self { inner: repr, kind }
    }
}

impl TaskExecutionGraphError {
    pub fn kind(&self) -> TaskExecutionGraphErrorKind {
        self.kind
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(TaskExecutionGraphErrorKind), vis(pub))]
enum TaskExecutionGraphErrorInner {
    #[error(transparent)]
    ProjectGraph(#[from] ProjectGraphError),

    #[error("task '{task}' in project '{project}' not found")]
    TaskNotFound { project: String, task: String },

    #[error("task with key '{key:?}' not found")]
    TaskNotFoundByKey { key: TaskKey },

    #[error(
        "cyclic dependency detected from '{from_project}#{from_task}' to '{to_project}#{to_task}'"
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
                        .description("p1t2 description")
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
            task_graph
                .get_task_index_by_name("project1", "p1t2")
                .unwrap()
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
            task_graph
                .get_task_index_by_name("project3", "p3t1")
                .unwrap()
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
                .get_task_index_by_name("project1", "shared-task-3")
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
                .get_task_index_by_name("project2", "shared-task")
                .unwrap()
        );
        assert_eq!(p1t2_dependency_1.1.task_name, "shared-task");
        assert_eq!(p1t2_dependency_1.1.project_name, "project2");

        assert_eq!(
            p1t2_dependency_2.0,
            task_graph
                .get_task_index_by_name("project3", "shared-task")
                .unwrap()
        );
        assert_eq!(p1t2_dependency_2.1.task_name, "shared-task");
        assert_eq!(p1t2_dependency_2.1.project_name, "project3");

        assert_eq!(p1t3_dependencies.len(), 1);
        let p1t3_dependency = &p1t3_dependencies[0];
        assert_eq!(
            p1t3_dependency.0,
            task_graph
                .get_task_index_by_name("project3", "shared-task-2")
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
                TaskExecutionNode::new(
                    "p3t1".to_string(),
                    "echo p3t1".to_string(),
                    "project3".to_string(),
                    blank_path.clone(),
                ),
                TaskExecutionNode::new(
                    "p2t1".to_string(),
                    "echo p2t1".to_string(),
                    "project2".to_string(),
                    blank_path.clone(),
                ),
                TaskExecutionNode::new(
                    "shared-task-3".to_string(),
                    "echo shared-task-3".to_string(),
                    "project4".to_string(),
                    blank_path.clone(),
                ),
            ],
            vec![TaskExecutionNode::new(
                "shared-task-3".to_string(),
                "echo shared-task-3".to_string(),
                "project3".to_string(),
                blank_path.clone(),
            )],
            vec![TaskExecutionNode::new(
                "shared-task-3".to_string(),
                "echo shared-task-3".to_string(),
                "project2".to_string(),
                blank_path.clone(),
            )],
            vec![TaskExecutionNode::new(
                "shared-task-3".to_string(),
                "echo shared-task-3".to_string(),
                "project1".to_string(),
                blank_path.clone(),
            )],
            vec![TaskExecutionNode::new(
                "p1t4".to_string(),
                "echo p1t4".to_string(),
                "project1".to_string(),
                blank_path.clone(),
            )],
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
