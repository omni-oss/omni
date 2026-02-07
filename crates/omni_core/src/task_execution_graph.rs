use std::{
    collections::{HashMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher as _},
    path::{Path, PathBuf},
    time::Duration,
};

use omni_config_types::TeraExprBoolean;
use petgraph::{
    Direction,
    algo::is_cyclic_directed,
    graph::{DiGraph, NodeIndex},
    visit::{Dfs, IntoNeighborsDirected as _, Reversed, Topo, Walker},
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
    dependencies: Vec<String>,
    enabled: TeraExprBoolean,
    interactive: bool,
    persistent: bool,
    max_retries: Option<u8>,
    retry_interval: Option<Duration>,
}

impl TaskExecutionNode {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        task_name: impl Into<String>,
        task_command: impl Into<String>,
        project_name: impl Into<String>,
        project_dir: impl Into<PathBuf>,
        dependencies: Vec<String>,
        enabled: TeraExprBoolean,
        interactive: bool,
        persistent: bool,
        max_retries: Option<u8>,
        retry_interval: Option<Duration>,
    ) -> Self {
        let project_name = project_name.into();
        let task_name = task_name.into();
        Self {
            full_task_name: format!("{}#{}", &project_name, &task_name),
            task_name,
            task_command: task_command.into(),
            project_name,
            project_dir: project_dir.into(),
            dependencies,
            enabled,
            interactive,
            persistent,
            max_retries,
            retry_interval,
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

    pub fn dependencies(&self) -> &[String] {
        &self.dependencies
    }

    pub fn enabled(&self) -> &TeraExprBoolean {
        &self.enabled
    }

    pub fn interactive(&self) -> bool {
        self.interactive
    }

    pub fn persistent(&self) -> bool {
        self.persistent
    }

    pub fn max_retries(&self) -> Option<u8> {
        self.max_retries
    }

    pub fn retry_interval(&self) -> Option<Duration> {
        self.retry_interval
    }

    /// (task_name, task_command, project_name, project_dir, full_task_name, dependencies, enabled, interactive, persistent, max_retries, retry_interval)
    pub fn deconstruct(
        self,
    ) -> (
        String,
        String,
        String,
        PathBuf,
        String,
        Vec<String>,
        TeraExprBoolean,
        bool,
        bool,
        Option<u8>,
        Option<Duration>,
    ) {
        (
            self.task_name,
            self.task_command,
            self.project_name,
            self.project_dir,
            self.full_task_name,
            self.dependencies,
            self.enabled,
            self.interactive,
            self.persistent,
            self.max_retries,
            self.retry_interval,
        )
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, Copy,
)]
pub enum EdgeType {
    Dependency,
    Sibling,
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

type InnerGraph = DiGraph<TaskExecutionNode, EdgeType>;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TaskExecutionGraph {
    node_map: HashMap<TaskKey, NodeIndex>,
    di_graph: InnerGraph,
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
                    vec![],
                    task.1.enabled.clone(),
                    task.1.interactive,
                    task.1.persistent,
                    task.1.max_retries,
                    task.1.retry_interval,
                );

                let dep_node_index =
                    graph.di_graph.add_node(task_execution_node.clone());
                graph.node_map.insert(
                    TaskKey::new(project_name, task_name),
                    dep_node_index,
                );
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
                                EdgeType::Dependency,
                            )?;
                        }
                        crate::TaskDependency::ExplicitProject {
                            project,
                            task,
                        } => {
                            graph.add_edge_by_names(
                                pname,
                                tname,
                                project,
                                task,
                                EdgeType::Dependency,
                            )?;
                        }
                        crate::TaskDependency::Upstream { task } => {
                            add_upstream(
                                project_graph,
                                &mut graph,
                                project,
                                pname,
                                tname,
                                task,
                                EdgeType::Dependency,
                            )?;
                        }
                    };
                }

                for sibling in task.1.siblings.iter() {
                    match sibling {
                        crate::TaskDependency::Own { task } => {
                            graph.add_edge_by_names(
                                pname,
                                tname,
                                &project.name,
                                task,
                                EdgeType::Sibling,
                            )?;
                        }
                        crate::TaskDependency::ExplicitProject {
                            project,
                            task,
                        } => {
                            graph.add_edge_by_names(
                                pname,
                                tname,
                                project,
                                task,
                                EdgeType::Sibling,
                            )?;
                        }
                        crate::TaskDependency::Upstream { task } => {
                            add_upstream(
                                project_graph,
                                &mut graph,
                                project,
                                pname,
                                tname,
                                task,
                                EdgeType::Sibling,
                            )?;
                        }
                    };
                }
            }
        }

        Ok(graph)
    }
}

fn add_upstream(
    project_graph: &ProjectGraph,
    task_graph: &mut TaskExecutionGraph,
    project: &&Project,
    dependent_project_name: &str,
    dependent_task_name: &str,
    task: &str,
    edge_type: EdgeType,
) -> Result<(), TaskExecutionGraphError> {
    let dependencies =
        project_graph.get_direct_dependencies_by_name(&project.name)?;

    if dependencies.is_empty() {
        return Ok(());
    }

    for (_, p) in dependencies.iter() {
        if p.tasks.contains_key(task) {
            let add = match edge_type {
                EdgeType::Dependency => !task_graph
                    .contains_dependency_by_names(
                        dependent_project_name,
                        dependent_task_name,
                        &p.name,
                        task,
                    )?,
                EdgeType::Sibling => !task_graph.contains_sibling_by_names(
                    dependent_project_name,
                    dependent_task_name,
                    &p.name,
                    task,
                )?,
            };
            if add {
                task_graph.add_edge_by_names(
                    dependent_project_name,
                    dependent_task_name,
                    &p.name,
                    task,
                    edge_type,
                )?;
            }
        } else {
            add_upstream(
                project_graph,
                task_graph,
                &p,
                dependent_project_name,
                dependent_task_name,
                task,
                edge_type,
            )?;
        };
    }
    Ok(())
}

pub type BatchedExecutionPlan = Vec<Vec<TaskExecutionNode>>;

macro_rules! filtered_graph {
    ($graph:expr, $edge_type:expr) => {
        petgraph::visit::EdgeFiltered::from_fn($graph, |e| {
            *e.weight() == $edge_type
        })
    };
}

impl TaskExecutionGraph {
    fn has_connected_node(
        &self,
        from: NodeIndex,
        to: NodeIndex,
        edge_type: EdgeType,
    ) -> TaskExecutionGraphResult<bool> {
        let edge = self.di_graph.find_edge(to, from);

        if let Some(edge) = edge
            && self.di_graph[edge] == edge_type
        {
            return Ok(true);
        }

        Ok(false)
    }

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

        self.has_connected_node(depedent, dependee, EdgeType::Dependency)
    }

    fn contains_sibling_by_names(
        &self,
        from_project_name: &str,
        from_task_name: &str,
        to_project_name: &str,
        to_task_name: &str,
    ) -> TaskExecutionGraphResult<bool> {
        let from =
            self.get_task_index_by_name(from_project_name, from_task_name)?;
        let to = self.get_task_index_by_name(to_project_name, to_task_name)?;

        self.has_connected_node(from, to, EdgeType::Sibling)
    }

    fn add_edge_by_names(
        &mut self,
        from_project_name: &str,
        from_task_name: &str,
        to_project_name: &str,
        to_task_name: &str,
        edge_type: EdgeType,
    ) -> TaskExecutionGraphResult<()> {
        let to_idx =
            self.get_task_index_by_name(to_project_name, to_task_name)?;

        if edge_type == EdgeType::Dependency && self.di_graph[to_idx].persistent
        {
            return Err(
                TaskExecutionGraphError::cant_depend_on_persistent_task(
                    from_project_name,
                    from_task_name,
                    to_project_name,
                    to_task_name,
                ),
            );
        }

        let from_idx =
            self.get_task_index_by_name(from_project_name, from_task_name)?;

        let edge_idx = self.di_graph.add_edge(to_idx, from_idx, edge_type);

        let graph = filtered_graph!(&self.di_graph, edge_type);

        if is_cyclic_directed(&graph) {
            self.di_graph.remove_edge(edge_idx);
            let dependee = self.di_graph[from_idx].clone();
            let dependent = self.di_graph[to_idx].clone();

            return Err(TaskExecutionGraphError::cycle_detected(
                dependent.project_name(),
                dependent.task_name(),
                dependee.project_name(),
                dependee.task_name(),
            ));
        }

        let to_full_name = self.di_graph[to_idx].full_task_name.clone();

        if edge_type == EdgeType::Dependency {
            self.di_graph[from_idx].dependencies.push(to_full_name);
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
    pub fn get_direct_dependencies_ref_by_name(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> TaskExecutionGraphResult<Vec<(NodeIndex, &TaskExecutionNode)>> {
        let task_key = TaskKey::new(project_name, task_name);

        self.get_direct_dependencies_ref_by_key(&task_key)
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
    fn get_direct_dependencies_by_key(
        &self,
        key: &TaskKey,
    ) -> TaskExecutionGraphResult<Vec<(NodeIndex, TaskExecutionNode)>> {
        let task_index = self.get_task_index_by_key(key)?;

        self.get_direct_dependencies(task_index)
    }

    #[inline(always)]
    fn get_direct_dependencies_ref_by_key(
        &self,
        key: &TaskKey,
    ) -> TaskExecutionGraphResult<Vec<(NodeIndex, &TaskExecutionNode)>> {
        let task_index = self.get_task_index_by_key(key)?;

        self.get_direct_dependencies_ref(task_index)
    }

    pub fn get_direct_dependencies(
        &self,
        task_index: NodeIndex,
    ) -> TaskExecutionGraphResult<Vec<(NodeIndex, TaskExecutionNode)>> {
        self.get_connected_nodes_impl(
            task_index,
            EdgeType::Dependency,
            |ni, node| (ni, node.clone()),
        )
    }

    fn get_connected_nodes_impl<'a, R>(
        &'a self,
        task_index: NodeIndex,
        edge_type: EdgeType,
        map: impl Fn(NodeIndex, &'a TaskExecutionNode) -> R,
    ) -> TaskExecutionGraphResult<Vec<R>>
    where
        R: 'a,
    {
        let map = |ni| {
            let node = &self.di_graph[ni];
            map(ni, node)
        };

        let graph = filtered_graph!(&self.di_graph, edge_type);

        let neighbors =
            graph.neighbors_directed(task_index, Direction::Incoming);

        Ok(neighbors.map(map).collect())
    }

    pub fn get_direct_dependencies_ref(
        &self,
        task_index: NodeIndex,
    ) -> TaskExecutionGraphResult<Vec<(NodeIndex, &TaskExecutionNode)>> {
        self.get_connected_nodes_impl(
            task_index,
            EdgeType::Dependency,
            |ni, node| (ni, node),
        )
    }

    pub fn get_batched_execution_plan_impl(
        &self,
        filter: impl Fn(&TaskExecutionNode) -> Result<bool, eyre::Report>,
        with_dependents: bool,
        with_dependents_filter: impl Fn(
            &TaskExecutionNode,
        ) -> Result<bool, eyre::Report>,
    ) -> TaskExecutionGraphResult<BatchedExecutionPlan> {
        // Determine the root nodes. A root node is a node that is not a dependency of any other node
        // Root nodes should always be in the last batch, persistent root nodes should be sorted to the end of the last batch

        // Step 1: Get all nodes that match the predicate, all persistent tasks are considered as roots since no one depends on them
        let mut possible_roots = HashSet::new();
        for node in self.di_graph.node_indices() {
            if filter(&self.di_graph[node])? {
                possible_roots.insert(node);
            }
        }

        let dep_graph = filtered_graph!(&self.di_graph, EdgeType::Dependency);

        if with_dependents {
            // Step 1.5 - Include dependents (direct and indirect) of possible roots
            let mut dependents = HashSet::new();
            let mut stack =
                possible_roots.clone().into_iter().collect::<Vec<_>>();
            while let Some(i) = stack.pop() {
                if dependents.insert(i) {
                    for n in
                        dep_graph.neighbors_directed(i, Direction::Outgoing)
                    {
                        if with_dependents_filter(&self.di_graph[n])? {
                            stack.push(n);
                        }
                    }
                }
            }

            // Replace reachable with the union of both sets
            possible_roots.extend(dependents);
        }

        let mut actual_roots = vec![];

        // Step 2: Filter out root nodes that are direct or indirect dependencies of other root nodes
        for i in possible_roots.iter() {
            let other_roots = possible_roots
                .difference(&HashSet::from([*i]))
                .copied()
                .collect::<HashSet<_>>();
            let dfs = Dfs::new(&dep_graph, *i);

            if dfs.iter(&dep_graph).any(|n| other_roots.contains(&n)) {
                continue;
            }

            actual_roots.push(*i);
        }

        // Step 3: Get all reachable nodes based from filtered roots
        let mut reachable = HashSet::new();
        let mut stack = actual_roots.clone();
        while let Some(i) = stack.pop() {
            if reachable.insert(i) {
                for n in dep_graph.neighbors_directed(i, Direction::Incoming) {
                    stack.push(n);
                }
            }
        }

        // Use longest path to determine the level of each node
        // Level = max(level of predecessors) + 1
        let mut levels = HashMap::new();
        let mut topo = Topo::new(&dep_graph);

        while let Some(node) = topo.next(&dep_graph) {
            if !reachable.contains(&node) {
                continue;
            }

            let level = dep_graph
                .neighbors_directed(node, Direction::Incoming)
                .filter(|n| reachable.contains(n))
                .map(|n| levels.get(&n).copied().unwrap_or(0))
                .max()
                .unwrap_or(0);

            levels.insert(node, level + 1);
        }

        let max_level = *levels.values().max().unwrap_or(&0);
        for (node, level) in levels.iter_mut() {
            let node = &self.di_graph[*node];

            // if the node is persistent, it should be sorted to the end of the last batch
            if node.persistent {
                *level = max_level;
            }
        }

        // add sibling nodes
        let sibling_graph = filtered_graph!(&self.di_graph, EdgeType::Sibling);
        let reversed = Reversed(&sibling_graph);
        for (node, level) in levels.clone() {
            let dfs = Dfs::new(reversed, node);

            for n in dfs.iter(reversed) {
                // bring down the level of the existing node to the level of the sibling node if it is higher
                if let Some(n) = levels.get_mut(&n)
                    && *n > level
                {
                    *n = level;
                } else {
                    levels.insert(n, level);
                }
            }
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

    #[cfg_attr(
        feature = "enable-tracing",
        tracing::instrument(level = "trace", skip_all)
    )]
    pub fn get_batched_execution_plan(
        &self,
        filter: impl Fn(&TaskExecutionNode) -> Result<bool, eyre::Report>,
    ) -> TaskExecutionGraphResult<BatchedExecutionPlan> {
        self.get_batched_execution_plan_impl(filter, false, |_| Ok(false))
    }

    #[cfg_attr(
        feature = "enable-tracing",
        tracing::instrument(level = "trace", skip_all)
    )]
    pub fn get_batched_execution_plan_with_dependents(
        &self,
        filter: impl Fn(&TaskExecutionNode) -> Result<bool, eyre::Report>,
        with_dependents_filter: impl Fn(
            &TaskExecutionNode,
        ) -> Result<bool, eyre::Report>,
    ) -> TaskExecutionGraphResult<BatchedExecutionPlan> {
        self.get_batched_execution_plan_impl(
            filter,
            true,
            with_dependents_filter,
        )
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct TaskExecutionGraphError(TaskExecutionGraphErrorInner);

impl TaskExecutionGraphError {
    #[doc(hidden)]
    pub fn project_graph(source: ProjectGraphError) -> Self {
        Self(TaskExecutionGraphErrorInner::ProjectGraph(source))
    }

    #[doc(hidden)]
    pub fn task_not_found(project: &str, task: &str) -> Self {
        Self(TaskExecutionGraphErrorInner::TaskNotFound {
            project: project.to_string(),
            task: task.to_string(),
        })
    }

    #[doc(hidden)]
    pub fn task_not_found_by_key(key: TaskKey) -> Self {
        Self(TaskExecutionGraphErrorInner::TaskNotFoundByKey { key })
    }

    #[doc(hidden)]
    pub fn cycle_detected(
        from_project: &str,
        from_task: &str,
        to_project: &str,
        to_task: &str,
    ) -> Self {
        Self(TaskExecutionGraphErrorInner::CycleDetected {
            from_project: from_project.to_string(),
            from_task: from_task.to_string(),
            to_project: to_project.to_string(),
            to_task: to_task.to_string(),
        })
    }

    #[doc(hidden)]
    pub fn cant_depend_on_persistent_task(
        from_project: &str,
        from_task: &str,
        to_project: &str,
        to_task: &str,
    ) -> Self {
        Self(TaskExecutionGraphErrorInner::CantDependOnPersistentTask {
            from_project: from_project.to_string(),
            from_task: from_task.to_string(),
            to_project: to_project.to_string(),
            to_task: to_task.to_string(),
        })
    }
}

impl<T: Into<TaskExecutionGraphErrorInner>> From<T>
    for TaskExecutionGraphError
{
    fn from(value: T) -> Self {
        let repr = value.into();
        Self(repr)
    }
}

impl TaskExecutionGraphError {
    pub fn kind(&self) -> TaskExecutionGraphErrorKind {
        self.0.discriminant()
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(TaskExecutionGraphErrorKind), vis(pub))]
pub(crate) enum TaskExecutionGraphErrorInner {
    #[error(transparent)]
    ProjectGraph(#[from] ProjectGraphError),

    #[error("task '{task}' in project '{project}' not found")]
    TaskNotFound { project: String, task: String },

    #[error("task with key '{key:?}' not found")]
    TaskNotFoundByKey { key: TaskKey },

    #[error(
        "cycle detected from '{from_project}#{from_task}' to '{to_project}#{to_task}'"
    )]
    CycleDetected {
        from_project: String,
        from_task: String,
        to_project: String,
        to_task: String,
    },

    #[error(
        "can't depend on persistent task '{from_project}#{from_task}' from '{to_project}#{to_task}'"
    )]
    CantDependOnPersistentTask {
        from_project: String,
        from_task: String,
        to_project: String,
        to_task: String,
    },

    #[error(transparent)]
    Unknown(#[from] eyre::Report),
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
                .task("sibling-1", "echo sibling-1", |b| {
                    b.persistent(true)
                        .explicit_project_sibling("project2", "sibling-2")
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
                .task("sibling-2", "echo sibling-2", |b| {
                    b.own_sibling("sibling-2.1")
                })
                .task("sibling-2.1", "echo sibling-2.1", |b| {
                    b.upstream_sibling("sibling-3")
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
                .task("sibling-3", "echo sibling-3", |b| {
                    b.upstream_sibling("sibling-4")
                })
                .build(),
            ..create_project("project3")
        };

        let project4 = Project {
            tasks: TasksBuilder::new()
                .task("p4t1", "echo p4t1", |b| b)
                .task("shared-task-3", "echo shared-task-3", |b| b)
                .task("sibling-4", "echo sibling-4", |b| b)
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

        assert_eq!(task_graph.count(), 19, "Should have 19 nodes");
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

    fn node_mut(
        task_name: &str,
        task_command: &str,
        project_name: &str,
        update: impl Fn(&mut TaskExecutionNode),
    ) -> TaskExecutionNode {
        node_with_deps_mut(task_name, task_command, project_name, &[], update)
    }

    fn node_with_deps_mut(
        task_name: &str,
        task_command: &str,
        project_name: &str,
        dependencies: &[&str],
        update: impl Fn(&mut TaskExecutionNode),
    ) -> TaskExecutionNode {
        let mut node =
            node_with_deps(task_name, task_command, project_name, dependencies);
        update(&mut node);

        node
    }

    fn node(
        task_name: &str,
        task_command: &str,
        project_name: &str,
    ) -> TaskExecutionNode {
        return TaskExecutionNode::new(
            task_name.to_string(),
            task_command.to_string(),
            project_name.to_string(),
            PathBuf::from(""),
            vec![],
            TeraExprBoolean::Boolean(true),
            false,
            false,
            None,
            None,
        );
    }

    fn node_with_deps(
        task_name: &str,
        task_command: &str,
        project_name: &str,
        dependencies: &[&str],
    ) -> TaskExecutionNode {
        let mut node = node(task_name, task_command, project_name);
        node.dependencies =
            dependencies.iter().map(|s| s.to_string()).collect();
        node
    }

    #[test]
    fn test_batched_execution_plan() {
        let project_graph = create_project_graph();
        let task_graph =
            TaskExecutionGraph::from_project_graph(&project_graph).unwrap();

        let mut actual_plan = task_graph
            .get_batched_execution_plan(|n| {
                Ok(n.task_name == "p1t4" || n.task_name == "sibling-1")
            })
            .unwrap();

        actual_plan.iter_mut().for_each(|batch| {
            batch.sort();
        });

        let mut expected_plan = vec![
            vec![
                node("p3t1", "echo p3t1", "project3"),
                node("p2t1", "echo p2t1", "project2"),
                node("shared-task-3", "echo shared-task-3", "project4"),
            ],
            vec![node_with_deps(
                "shared-task-3",
                "echo shared-task-3",
                "project3",
                &["project4#shared-task-3"],
            )],
            vec![node_with_deps(
                "shared-task-3",
                "echo shared-task-3",
                "project2",
                &["project3#shared-task-3", "project2#p2t1"],
            )],
            vec![node_with_deps(
                "shared-task-3",
                "echo shared-task-3",
                "project1",
                &["project3#shared-task-3", "project2#shared-task-3"],
            )],
            vec![
                node_with_deps(
                    "p1t4",
                    "echo p1t4",
                    "project1",
                    &["project3#p3t1", "project1#shared-task-3"],
                ),
                node("sibling-2", "echo sibling-2", "project2"),
                node("sibling-2.1", "echo sibling-2.1", "project2"),
                node("sibling-3", "echo sibling-3", "project3"),
                node("sibling-4", "echo sibling-4", "project4"),
                node_mut("sibling-1", "echo sibling-1", "project1", |n| {
                    n.persistent = true
                }),
            ],
        ];

        expected_plan.iter_mut().for_each(|batch| {
            batch.sort();
        });

        for (i, batch) in actual_plan.iter().enumerate() {
            assert_eq!(batch.len(), expected_plan[i].len());
            for (j, task) in batch.iter().enumerate() {
                assert_eq!(task, &expected_plan[i][j]);
            }
        }
    }

    #[test]
    fn test_batched_execution_plan_must_not_include_persistent_tasks_if_not_matching_filter()
     {
        let project_graph = create_project_graph();
        let task_graph =
            TaskExecutionGraph::from_project_graph(&project_graph).unwrap();

        let actual_plan = task_graph
            .get_batched_execution_plan(|n| Ok(n.task_name == "p1t4"))
            .unwrap();

        for batch in &actual_plan {
            for task in batch {
                assert!(
                    !task.persistent,
                    "persistent task should not be included in the plan"
                );
            }
        }
    }
}
