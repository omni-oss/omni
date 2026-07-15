use std::{
    collections::{HashMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher as _},
    path::{Path, PathBuf},
    time::Duration,
};

use omni_command_config::CommandConfig;
use omni_config_types::TeraExprBoolean;
use petgraph::{
    Direction,
    algo::has_path_connecting,
    graph::{DiGraph, NodeIndex},
    visit::{Dfs, IntoNeighborsDirected as _, Walker},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{EnumDiscriminants, IntoDiscriminant};
use trace::Level;

use crate::{Project, ProjectGraph, ProjectGraphError};

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Deserialize,
    Serialize,
    JsonSchema,
)]
pub struct TaskExecutionNode {
    task_name: String,
    task_exec: Option<CommandConfig>,
    task_retry_exec: Option<CommandConfig>,
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
        task_exec: Option<CommandConfig>,
        task_retry_exec: Option<CommandConfig>,
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
            task_exec,
            task_retry_exec,
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

    pub fn task_exec_config(&self) -> Option<&CommandConfig> {
        self.task_exec.as_ref()
    }

    pub fn task_retry_exec_config(&self) -> Option<&CommandConfig> {
        self.task_retry_exec.as_ref()
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
        Option<CommandConfig>,
        Option<CommandConfig>,
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
            self.task_exec,
            self.task_retry_exec,
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
                    task.1.exec.clone(),
                    None,
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

/// Builds a [`TaskExecutionGraphError::sibling_dependency_cycle`] from the
/// offending nodes, reporting their `full_task_name`s in a deterministic
/// (sorted, de-duplicated) order.
fn sibling_dependency_cycle_error(
    graph: &TaskExecutionGraph,
    members: &[NodeIndex],
) -> TaskExecutionGraphError {
    let mut tasks = members
        .iter()
        .map(|n| graph.di_graph[*n].full_task_name.clone())
        .collect::<Vec<_>>();
    tasks.sort();
    tasks.dedup();
    TaskExecutionGraphError::sibling_dependency_cycle(tasks)
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

        // The new edge runs `to_idx` (the dependee) before `from_idx` (the
        // dependent). Because the graph is acyclic before this insertion, the
        // edge can only close a cycle within its own edge type when the
        // dependent can *already* reach the dependee along same-type edges --
        // or when it is a self-loop (`to_idx == from_idx`).
        //
        // Probing that single reachability question from `from_idx` is far
        // cheaper than the previous full-graph `is_cyclic_directed` scan
        // (which rebuilt and walked the entire filtered graph on every edge):
        // the DFS only explores `from_idx`'s reachable subgraph and
        // short-circuits the moment it reaches `to_idx`.
        let closes_cycle = to_idx == from_idx || {
            let graph = filtered_graph!(&self.di_graph, edge_type);
            has_path_connecting(&graph, from_idx, to_idx, None)
        };

        if closes_cycle {
            let dependee = &self.di_graph[from_idx];
            let dependent = &self.di_graph[to_idx];

            return Err(TaskExecutionGraphError::cycle_detected(
                dependent.project_name(),
                dependent.task_name(),
                dependee.project_name(),
                dependee.task_name(),
            ));
        }

        self.di_graph.add_edge(to_idx, from_idx, edge_type);

        if edge_type == EdgeType::Dependency {
            let to_full_name = self.di_graph[to_idx].full_task_name.clone();
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

        // Step 4: Determine sibling-connected components over the scheduled
        // node set.
        //
        // The scheduled set is `reachable` plus any task transitively
        // co-scheduled (sibling-connected) with a reachable task: sibling
        // ("with") tasks that are not themselves dependency-reachable are
        // pulled in so they run alongside their peers.
        //
        // Because every task in a component MUST share a batch, levels are
        // computed on the *condensation* of the dependency graph: each
        // sibling component collapses into a single super-node, and levels
        // are assigned by longest path over those super-nodes. This satisfies
        // both invariants at once -- siblings co-scheduled AND every
        // dependency strictly before its dependents -- and cascades a
        // co-scheduling "lift" to a member's dependents for free.
        //
        // Components are discovered by an undirected traversal of the sibling
        // graph seeded from the reachable nodes in sorted order, so the
        // result is fully deterministic regardless of hash iteration order.
        let sibling_graph = filtered_graph!(&self.di_graph, EdgeType::Sibling);

        let mut component_of: HashMap<NodeIndex, usize> = HashMap::new();
        let mut components: Vec<Vec<NodeIndex>> = Vec::new();

        let mut seeds = reachable.iter().copied().collect::<Vec<_>>();
        seeds.sort();

        for seed in seeds {
            if component_of.contains_key(&seed) {
                continue;
            }

            let cid = components.len();
            let mut members = Vec::new();
            let mut stack = vec![seed];
            while let Some(node) = stack.pop() {
                if component_of.contains_key(&node) {
                    continue;
                }
                component_of.insert(node, cid);
                members.push(node);

                for neighbor in sibling_graph
                    .neighbors_directed(node, Direction::Outgoing)
                    .chain(
                        sibling_graph
                            .neighbors_directed(node, Direction::Incoming),
                    )
                {
                    if !component_of.contains_key(&neighbor) {
                        stack.push(neighbor);
                    }
                }
            }

            components.push(members);
        }

        let n_comp = components.len();

        // Build the condensed dependency graph: one node per component, with
        // an edge `comp(dependency) -> comp(dependent)` for every dependency
        // edge whose endpoints are both scheduled and live in *different*
        // components. A dependency edge *within* a component is an immediate
        // contradiction: a task co-scheduled with one it depends on.
        let mut cond_adj: Vec<HashSet<usize>> = vec![HashSet::new(); n_comp];
        let mut cond_indeg: Vec<usize> = vec![0; n_comp];

        for (&node, &cid) in &component_of {
            for dep in dep_graph.neighbors_directed(node, Direction::Incoming) {
                let Some(&dep_cid) = component_of.get(&dep) else {
                    continue;
                };

                if dep_cid == cid {
                    return Err(sibling_dependency_cycle_error(
                        self,
                        &components[cid],
                    ));
                }

                if cond_adj[dep_cid].insert(cid) {
                    cond_indeg[cid] += 1;
                }
            }
        }

        // Longest-path level per component via Kahn's algorithm. A component
        // is finalized only once all of its predecessors are, so its level is
        // the length of the longest dependency chain of components ending at
        // it (roots = 1).
        let mut comp_level = vec![1usize; n_comp];
        let mut indeg = cond_indeg;
        let mut queue = indeg
            .iter()
            .enumerate()
            .filter(|&(_, &d)| d == 0)
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        let mut processed = 0usize;

        while let Some(c) = queue.pop() {
            processed += 1;
            for &succ in &cond_adj[c] {
                if comp_level[c] + 1 > comp_level[succ] {
                    comp_level[succ] = comp_level[c] + 1;
                }
                indeg[succ] -= 1;
                if indeg[succ] == 0 {
                    queue.push(succ);
                }
            }
        }

        if processed != n_comp {
            // Some components never reached in-degree zero: a dependency cycle
            // *through* sibling components (e.g. `a ~ c` with `a <- b <- c`).
            let offenders = components
                .iter()
                .enumerate()
                .filter(|(cid, _)| indeg[*cid] != 0)
                .flat_map(|(_, members)| members.iter().copied())
                .collect::<Vec<_>>();
            return Err(sibling_dependency_cycle_error(self, &offenders));
        }

        // Persistent tasks run "forever", so they belong in the final batch --
        // but only when doing so cannot strand a dependent. A persistent task
        // can never itself be depended upon (enforced at construction), yet it
        // may be co-scheduled with a non-persistent sibling that *does* have
        // dependents; in that case dependency ordering wins and the component
        // keeps its computed level.
        let max_comp_level = comp_level.iter().copied().max().unwrap_or(0);
        for (cid, members) in components.iter().enumerate() {
            let is_persistent =
                members.iter().any(|n| self.di_graph[*n].persistent);
            if !is_persistent {
                continue;
            }

            let has_dependent = members.iter().any(|n| {
                dep_graph
                    .neighbors_directed(*n, Direction::Outgoing)
                    .any(|d| component_of.contains_key(&d))
            });

            if !has_dependent {
                comp_level[cid] = max_comp_level;
            }
        }

        // Assign every scheduled task its component's level.
        let mut levels: HashMap<NodeIndex, usize> = HashMap::new();
        for (&node, &cid) in &component_of {
            levels.insert(node, comp_level[cid]);
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
        tracing::instrument(level = Level::DEBUG, skip_all)
    )]
    pub fn get_batched_execution_plan(
        &self,
        filter: impl Fn(&TaskExecutionNode) -> Result<bool, eyre::Report>,
    ) -> TaskExecutionGraphResult<BatchedExecutionPlan> {
        self.get_batched_execution_plan_impl(filter, false, |_| Ok(false))
    }

    #[cfg_attr(
        feature = "enable-tracing",
        tracing::instrument(level = Level::DEBUG, skip_all)
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

    #[doc(hidden)]
    pub fn sibling_dependency_cycle(tasks: Vec<String>) -> Self {
        Self(TaskExecutionGraphErrorInner::SiblingDependencyCycle { tasks })
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

    #[error(
        "co-scheduled sibling tasks cannot also form a dependency chain among themselves: {tasks:?}"
    )]
    SiblingDependencyCycle { tasks: Vec<String> },

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
        task_exec: &str,
        project_name: &str,
        update: impl Fn(&mut TaskExecutionNode),
    ) -> TaskExecutionNode {
        node_with_deps_mut(task_name, task_exec, project_name, &[], update)
    }

    fn node_with_deps_mut(
        task_name: &str,
        task_exec: &str,
        project_name: &str,
        dependencies: &[&str],
        update: impl Fn(&mut TaskExecutionNode),
    ) -> TaskExecutionNode {
        let mut node =
            node_with_deps(task_name, task_exec, project_name, dependencies);
        update(&mut node);

        node
    }

    fn node(
        task_name: &str,
        task_exec: &str,
        project_name: &str,
    ) -> TaskExecutionNode {
        return TaskExecutionNode::new(
            task_name.to_string(),
            Some(CommandConfig::Shell(task_exec.to_string())),
            None,
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
        task_exec: &str,
        project_name: &str,
        dependencies: &[&str],
    ) -> TaskExecutionNode {
        let mut node = node(task_name, task_exec, project_name);
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

    // -- Scheduling invariants -------------------------------------------
    //
    // These tests lock in the correctness/determinism contract of the
    // batched execution plan. The barrier-based executor relies on these
    // invariants for free, but they are the *specification* that any future
    // scheduler (e.g. a continuous cross-batch scheduler) MUST also satisfy:
    //
    //   1. Topological order: for every `Dependency` edge, the dependee runs
    //      in a strictly earlier batch than the dependent. This is what lets
    //      a task safely read its dependencies' digests at dispatch time.
    //   2. Sibling co-scheduling: `Sibling`-linked tasks land in the same
    //      batch (co-scheduled, not ordered).
    //   3. Determinism: repeated planning of the same graph yields the same
    //      number of batches and the same membership per batch.
    //   4. Completeness & partition: every reachable task appears exactly
    //      once across all batches.

    use petgraph::visit::EdgeRef as _;

    /// Maps every task in the plan to the index of the batch it appears in.
    fn batch_index_by_task(
        plan: &BatchedExecutionPlan,
    ) -> HashMap<String, usize> {
        let mut map = HashMap::new();
        for (i, batch) in plan.iter().enumerate() {
            for node in batch {
                map.insert(node.full_task_name().to_string(), i);
            }
        }
        map
    }

    /// Asserts every `Dependency` edge whose endpoints are both present in
    /// the plan places the dependee in a strictly earlier batch than the
    /// dependent. Edges in the graph store `dependee -> dependent`.
    fn assert_topological_order(
        task_graph: &TaskExecutionGraph,
        plan: &BatchedExecutionPlan,
    ) {
        let batch_of = batch_index_by_task(plan);

        for edge in task_graph.di_graph.edge_references() {
            if *edge.weight() != EdgeType::Dependency {
                continue;
            }

            let dependee = task_graph.di_graph[edge.source()].full_task_name();
            let dependent = task_graph.di_graph[edge.target()].full_task_name();

            if let (Some(&dependee_batch), Some(&dependent_batch)) =
                (batch_of.get(dependee), batch_of.get(dependent))
            {
                assert!(
                    dependee_batch < dependent_batch,
                    "dependency `{dependee}` (batch {dependee_batch}) must \
                     run strictly before dependent `{dependent}` (batch \
                     {dependent_batch})"
                );
            }
        }
    }

    /// Asserts every `Sibling` edge whose endpoints are both present in the
    /// plan places both tasks in the very same batch.
    fn assert_siblings_co_scheduled(
        task_graph: &TaskExecutionGraph,
        plan: &BatchedExecutionPlan,
    ) {
        let batch_of = batch_index_by_task(plan);

        for edge in task_graph.di_graph.edge_references() {
            if *edge.weight() != EdgeType::Sibling {
                continue;
            }

            let a = task_graph.di_graph[edge.source()].full_task_name();
            let b = task_graph.di_graph[edge.target()].full_task_name();

            if let (Some(&a_batch), Some(&b_batch)) =
                (batch_of.get(a), batch_of.get(b))
            {
                assert_eq!(
                    a_batch, b_batch,
                    "siblings `{a}` and `{b}` must be co-scheduled in the \
                     same batch (got {a_batch} vs {b_batch})"
                );
            }
        }
    }

    /// Returns the sorted `full_task_name`s of each batch, giving a
    /// canonical, within-batch-order-independent view of a plan.
    fn canonical_batches(plan: &BatchedExecutionPlan) -> Vec<Vec<String>> {
        plan.iter()
            .map(|batch| {
                let mut names = batch
                    .iter()
                    .map(|n| n.full_task_name().to_string())
                    .collect::<Vec<_>>();
                names.sort();
                names
            })
            .collect()
    }

    /// Single project holding a strict linear chain a <- b <- c <- d.
    fn linear_chain_project_graph() -> ProjectGraph {
        let project = Project {
            tasks: TasksBuilder::new()
                .task("a", "echo a", |b| b)
                .task("b", "echo b", |b| b.own_dependency("a"))
                .task("c", "echo c", |b| b.own_dependency("b"))
                .task("d", "echo d", |b| b.own_dependency("c"))
                .build(),
            ..create_project("proj")
        };

        ProjectGraph::from_projects(vec![project]).expect("Can't create graph")
    }

    /// Single project with a fan-out: root `a` and three dependents b, c, d
    /// that each depend only on `a`.
    fn fan_out_project_graph() -> ProjectGraph {
        let project = Project {
            tasks: TasksBuilder::new()
                .task("a", "echo a", |b| b)
                .task("b", "echo b", |b| b.own_dependency("a"))
                .task("c", "echo c", |b| b.own_dependency("a"))
                .task("d", "echo d", |b| b.own_dependency("a"))
                .build(),
            ..create_project("proj")
        };

        ProjectGraph::from_projects(vec![project]).expect("Can't create graph")
    }

    #[test]
    fn full_plan_preserves_topological_order() {
        let project_graph = create_project_graph();
        let task_graph =
            TaskExecutionGraph::from_project_graph(&project_graph).unwrap();

        // `Ok(true)` selects every node as a possible root, so the resulting
        // plan spans the entire reachable graph.
        let plan = task_graph.get_batched_execution_plan(|_| Ok(true)).unwrap();

        assert_topological_order(&task_graph, &plan);
    }

    #[test]
    fn full_plan_co_schedules_siblings() {
        let project_graph = create_project_graph();
        let task_graph =
            TaskExecutionGraph::from_project_graph(&project_graph).unwrap();

        let plan = task_graph.get_batched_execution_plan(|_| Ok(true)).unwrap();

        assert_siblings_co_scheduled(&task_graph, &plan);
    }

    #[test]
    fn full_plan_partitions_every_task_exactly_once() {
        let project_graph = create_project_graph();
        let task_graph =
            TaskExecutionGraph::from_project_graph(&project_graph).unwrap();

        let plan = task_graph.get_batched_execution_plan(|_| Ok(true)).unwrap();

        let mut seen = HashSet::new();
        let mut total = 0;
        for batch in &plan {
            for node in batch {
                total += 1;
                assert!(
                    seen.insert(node.full_task_name().to_string()),
                    "task `{}` appears in more than one batch",
                    node.full_task_name()
                );
            }
        }

        // Every node in the fixture is reachable from a sink, so the plan
        // must cover the whole graph with no duplicates.
        assert_eq!(total, seen.len());
        assert_eq!(
            total,
            task_graph.count(),
            "the full plan must contain every task exactly once"
        );
    }

    #[test]
    fn plan_is_deterministic_across_repeated_calls() {
        let project_graph = create_project_graph();
        let task_graph =
            TaskExecutionGraph::from_project_graph(&project_graph).unwrap();

        let baseline = canonical_batches(
            &task_graph.get_batched_execution_plan(|_| Ok(true)).unwrap(),
        );

        // The plan uses hash-based intermediate collections; re-planning many
        // times must always yield the same batch count and membership.
        for _ in 0..25 {
            let plan = canonical_batches(
                &task_graph.get_batched_execution_plan(|_| Ok(true)).unwrap(),
            );
            assert_eq!(
                plan, baseline,
                "batched execution plan is not deterministic"
            );
        }
    }

    #[test]
    fn linear_chain_is_fully_serialized() {
        let project_graph = linear_chain_project_graph();
        let task_graph =
            TaskExecutionGraph::from_project_graph(&project_graph).unwrap();

        let plan = task_graph.get_batched_execution_plan(|_| Ok(true)).unwrap();

        assert_topological_order(&task_graph, &plan);

        // A strict chain must produce one task per batch, in order.
        assert_eq!(
            canonical_batches(&plan),
            vec![
                vec!["proj#a".to_string()],
                vec!["proj#b".to_string()],
                vec!["proj#c".to_string()],
                vec!["proj#d".to_string()],
            ]
        );
    }

    #[test]
    fn fan_out_dependents_share_one_batch_after_root() {
        let project_graph = fan_out_project_graph();
        let task_graph =
            TaskExecutionGraph::from_project_graph(&project_graph).unwrap();

        let plan = task_graph.get_batched_execution_plan(|_| Ok(true)).unwrap();

        assert_topological_order(&task_graph, &plan);

        assert_eq!(
            canonical_batches(&plan),
            vec![
                vec!["proj#a".to_string()],
                vec![
                    "proj#b".to_string(),
                    "proj#c".to_string(),
                    "proj#d".to_string(),
                ],
            ]
        );
    }

    #[test]
    fn diamond_dependency_levels_are_asap() {
        // a <- {b, c} <- d  (classic split/join diamond)
        let project = Project {
            tasks: TasksBuilder::new()
                .task("a", "echo a", |b| b)
                .task("b", "echo b", |b| b.own_dependency("a"))
                .task("c", "echo c", |b| b.own_dependency("a"))
                .task("d", "echo d", |b| {
                    b.own_dependency("b").own_dependency("c")
                })
                .build(),
            ..create_project("proj")
        };
        let project_graph = ProjectGraph::from_projects(vec![project])
            .expect("Can't create graph");
        let task_graph =
            TaskExecutionGraph::from_project_graph(&project_graph).unwrap();

        let plan = task_graph.get_batched_execution_plan(|_| Ok(true)).unwrap();

        assert_topological_order(&task_graph, &plan);
        assert_eq!(
            canonical_batches(&plan),
            vec![
                vec!["proj#a".to_string()],
                vec!["proj#b".to_string(), "proj#c".to_string()],
                vec!["proj#d".to_string()],
            ]
        );
    }

    /// Minimal deterministic xorshift PRNG so property tests are reproducible
    /// (no external crate, no reliance on hash seeds).
    struct Rng(u32);

    impl Rng {
        fn new(seed: u32) -> Self {
            // Avoid the zero state, which is a fixed point for xorshift.
            Rng(seed | 1)
        }

        fn next_u32(&mut self) -> u32 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            self.0 = x;
            x
        }
    }

    /// Builds a random acyclic single-project task graph with `n` tasks.
    /// Dependency edges only ever point from a lower index to a higher one,
    /// which makes cycles impossible by construction. Returns the graph plus
    /// the adjacency list (`deps_of[j]` = indices task `j` depends on) so the
    /// expected longest-path levels can be computed independently.
    fn random_dag(seed: u32, n: usize) -> (ProjectGraph, Vec<Vec<usize>>) {
        let mut rng = Rng::new(seed);

        let mut deps_of: Vec<Vec<usize>> = Vec::with_capacity(n);
        for j in 0..n {
            let mut ds = Vec::new();
            for i in 0..j {
                // ~40% edge density.
                if rng.next_u32() % 100 < 40 {
                    ds.push(i);
                }
            }
            deps_of.push(ds);
        }

        let mut builder = TasksBuilder::new();
        for (j, deps) in deps_of.iter().enumerate() {
            let name = format!("t{j}");
            let exec = format!("echo {name}");
            let deps = deps.iter().map(|i| format!("t{i}")).collect::<Vec<_>>();
            builder = builder.task(name, exec, move |mut b| {
                for dep in &deps {
                    b = b.own_dependency(dep.clone());
                }
                b
            });
        }

        let project = Project {
            tasks: builder.build(),
            ..create_project("proj")
        };

        let graph = ProjectGraph::from_projects(vec![project])
            .expect("Can't create graph");

        (graph, deps_of)
    }

    /// Longest dependency-path length ending at each task (roots = 1). This is
    /// exactly the ASAP level the planner is expected to assign.
    fn longest_path_levels(deps_of: &[Vec<usize>]) -> Vec<usize> {
        let mut level = vec![0usize; deps_of.len()];
        // deps_of[j] only references indices < j, so a single forward pass
        // resolves every predecessor before its dependents.
        for j in 0..deps_of.len() {
            let pred_max =
                deps_of[j].iter().map(|&i| level[i]).max().unwrap_or(0);
            level[j] = pred_max + 1;
        }
        level
    }

    #[test]
    fn random_dags_satisfy_scheduling_invariants() {
        for seed in 1u32..=60 {
            let n = 8 + (seed as usize % 25); // 8..=32 tasks
            let (project_graph, deps_of) = random_dag(seed, n);
            let task_graph =
                TaskExecutionGraph::from_project_graph(&project_graph).unwrap();

            let plan =
                task_graph.get_batched_execution_plan(|_| Ok(true)).unwrap();

            // 1. Dependencies always precede dependents.
            assert_topological_order(&task_graph, &plan);

            // 2. Every task appears exactly once.
            let batch_of = batch_index_by_task(&plan);
            assert_eq!(
                batch_of.len(),
                n,
                "seed {seed}: plan must contain every task exactly once"
            );

            // 3. ASAP leveling: each task's batch index equals its longest
            //    dependency-path length minus one, and the batch count equals
            //    the overall longest path.
            let expected = longest_path_levels(&deps_of);
            let expected_batches = *expected.iter().max().unwrap();
            assert_eq!(
                plan.len(),
                expected_batches,
                "seed {seed}: unexpected batch count"
            );
            for (j, &lvl) in expected.iter().enumerate() {
                let name = format!("proj#t{j}");
                assert_eq!(
                    batch_of.get(&name).copied(),
                    Some(lvl - 1),
                    "seed {seed}: task {name} placed in the wrong batch"
                );
            }

            // 4. Determinism: re-planning is identical.
            let baseline = canonical_batches(&plan);
            for _ in 0..5 {
                let again = canonical_batches(
                    &task_graph
                        .get_batched_execution_plan(|_| Ok(true))
                        .unwrap(),
                );
                assert_eq!(
                    again, baseline,
                    "seed {seed}: plan is not deterministic"
                );
            }
        }
    }

    // -- Graph construction / cycle detection ----------------------------
    //
    // These lock in the contract of `add_edge_by_names` (the per-edge cycle
    // check and the persistent-dependency guard). They are the regression
    // net for any optimization that replaces the full-graph
    // `is_cyclic_directed` scan with a cheaper targeted reachability check.

    /// Builds a single-project graph from `(task, exec, build_fn)` tuples,
    /// returning the fallible result so cycle/validation errors can be
    /// asserted directly.
    fn try_build_single_project(
        builder: TasksBuilder,
    ) -> TaskExecutionGraphResult<TaskExecutionGraph> {
        let project = Project {
            tasks: builder.build(),
            ..create_project("proj")
        };
        let project_graph = ProjectGraph::from_projects(vec![project])
            .expect("Can't create project graph");
        TaskExecutionGraph::from_project_graph(&project_graph)
    }

    #[test]
    fn self_dependency_is_rejected_as_cycle() {
        let result = try_build_single_project(TasksBuilder::new().task(
            "a",
            "echo a",
            |b| b.own_dependency("a"),
        ));

        let err = result.expect_err("self-dependency must be a cycle");
        assert_eq!(err.kind(), TaskExecutionGraphErrorKind::CycleDetected);
    }

    #[test]
    fn two_node_dependency_cycle_is_rejected() {
        let result = try_build_single_project(
            TasksBuilder::new()
                .task("a", "echo a", |b| b.own_dependency("b"))
                .task("b", "echo b", |b| b.own_dependency("a")),
        );

        let err = result.expect_err("a<->b must be a cycle");
        assert_eq!(err.kind(), TaskExecutionGraphErrorKind::CycleDetected);
    }

    #[test]
    fn three_node_dependency_cycle_is_rejected() {
        let result = try_build_single_project(
            TasksBuilder::new()
                .task("a", "echo a", |b| b.own_dependency("c"))
                .task("b", "echo b", |b| b.own_dependency("a"))
                .task("c", "echo c", |b| b.own_dependency("b")),
        );

        let err = result.expect_err("a<-b<-c<-a must be a cycle");
        assert_eq!(err.kind(), TaskExecutionGraphErrorKind::CycleDetected);
    }

    #[test]
    fn dependency_cycle_detection_is_independent_of_sibling_edges() {
        // A `Dependency` edge b->a plus a `Sibling` edge a->b would look like
        // a 2-cycle if edge types were conflated, but they live in separate
        // filtered subgraphs and must NOT be reported as a cycle.
        try_build_single_project(
            TasksBuilder::new()
                .task("a", "echo a", |b| b.own_dependency("b"))
                .task("b", "echo b", |b| b.own_sibling("a")),
        )
        .expect("a dependency edge and a reverse sibling edge are not a cycle");
    }

    #[test]
    fn real_dependency_cycle_still_caught_despite_sibling_edges() {
        // Presence of unrelated sibling edges must not mask a genuine
        // same-type dependency cycle (a<-b<-a).
        let result = try_build_single_project(
            TasksBuilder::new()
                .task("a", "echo a", |b| b.own_dependency("b").own_sibling("c"))
                .task("b", "echo b", |b| b.own_dependency("a"))
                .task("c", "echo c", |b| b),
        );

        let err =
            result.expect_err("a real dependency cycle must still be detected");
        assert_eq!(err.kind(), TaskExecutionGraphErrorKind::CycleDetected);
    }

    #[test]
    fn depending_on_persistent_task_is_rejected() {
        let result = try_build_single_project(
            TasksBuilder::new()
                .task("server", "echo server", |b| b.persistent(true))
                .task("client", "echo client", |b| b.own_dependency("server")),
        );

        let err = result
            .expect_err("depending on a persistent task must be rejected");
        assert_eq!(
            err.kind(),
            TaskExecutionGraphErrorKind::CantDependOnPersistentTask
        );
    }

    #[test]
    fn sibling_of_persistent_task_is_allowed() {
        // The persistent guard only applies to `Dependency` edges; siblings
        // (co-scheduled "with" tasks) may freely reference persistent tasks.
        try_build_single_project(
            TasksBuilder::new()
                .task("server", "echo server", |b| b.persistent(true))
                .task("watcher", "echo watcher", |b| b.own_sibling("server")),
        )
        .expect("a sibling of a persistent task is allowed");
    }

    // -- Sibling / `with` leveling ---------------------------------------
    //
    // Siblings are co-scheduled: every task in a sibling-connected component
    // (following sibling edges transitively, in both directions) must land
    // in exactly the same batch, while never breaking dependency ordering.

    #[test]
    fn transitive_siblings_are_co_scheduled_at_max_level() {
        // base <- x (x:2), base <- y (y:2), z:1 (no deps).
        // Sibling chain x ~ y ~ z links all three transitively; none of them
        // has dependents, so lifting z from level 1 to 2 cannot break any
        // dependency. The whole component must share one batch.
        let task_graph = try_build_single_project(
            TasksBuilder::new()
                .task("base", "echo base", |b| b)
                .task("x", "echo x", |b| {
                    b.own_dependency("base").own_sibling("y")
                })
                .task("y", "echo y", |b| {
                    b.own_dependency("base").own_sibling("z")
                })
                .task("z", "echo z", |b| b),
        )
        .unwrap();

        let plan = task_graph.get_batched_execution_plan(|_| Ok(true)).unwrap();

        assert_topological_order(&task_graph, &plan);
        assert_siblings_co_scheduled(&task_graph, &plan);

        assert_eq!(
            canonical_batches(&plan),
            vec![
                vec!["proj#base".to_string()],
                vec![
                    "proj#x".to_string(),
                    "proj#y".to_string(),
                    "proj#z".to_string(),
                ],
            ]
        );
    }

    #[test]
    fn sibling_pulls_in_non_dependency_reachable_task() {
        // Only `main` is selected as a root, but it is a sibling of `helper`,
        // which is not otherwise reachable. `helper` must be pulled into the
        // plan and co-scheduled in the same batch as `main`.
        let task_graph = try_build_single_project(
            TasksBuilder::new()
                .task("main", "echo main", |b| b.own_sibling("helper"))
                .task("helper", "echo helper", |b| b)
                .task("unrelated", "echo unrelated", |b| b),
        )
        .unwrap();

        let plan = task_graph
            .get_batched_execution_plan(|n| Ok(n.task_name == "main"))
            .unwrap();

        assert_siblings_co_scheduled(&task_graph, &plan);

        assert_eq!(
            canonical_batches(&plan),
            vec![vec!["proj#helper".to_string(), "proj#main".to_string()]],
            "the sibling must be pulled in and co-scheduled, and the \
             unrelated task must not appear"
        );
    }

    #[test]
    fn transitive_siblings_across_projects_share_one_batch() {
        // Uses the cross-project sibling chain baked into the shared fixture:
        //   project1#sibling-1 ~ project2#sibling-2 ~ project2#sibling-2.1
        //     ~ project3#sibling-3 ~ project4#sibling-4
        // Every member (following the transitive sibling edges) must be
        // co-scheduled in the very same batch.
        let project_graph = create_project_graph();
        let task_graph =
            TaskExecutionGraph::from_project_graph(&project_graph).unwrap();

        let plan = task_graph
            .get_batched_execution_plan(|n| Ok(n.task_name == "sibling-1"))
            .unwrap();

        assert_siblings_co_scheduled(&task_graph, &plan);

        let batch_of = batch_index_by_task(&plan);
        let members = [
            "project1#sibling-1",
            "project2#sibling-2",
            "project2#sibling-2.1",
            "project3#sibling-3",
            "project4#sibling-4",
        ];

        let first = batch_of
            .get(members[0])
            .copied()
            .expect("sibling-1 must be scheduled");
        for member in members {
            assert_eq!(
                batch_of.get(member).copied(),
                Some(first),
                "transitive sibling `{member}` must be in the same batch as \
                 the rest of its component"
            );
        }
    }

    #[test]
    fn sibling_lift_cascades_to_its_dependents() {
        // Two independent dependency chains:
        //   chain 1:  a <- b           (a:1, b:2)   b depends on a
        //   chain 2:  p <- q <- r      (p:1, q:2, r:3)
        // Sibling link a ~ r forces `a` and `r` into the same batch, lifting
        // `a` from its natural level 1 up to `r`'s level 3. Because `b`
        // depends on `a`, the lift must cascade: `b` is pushed to a strictly
        // later batch so dependency ordering still holds.
        let task_graph = try_build_single_project(
            TasksBuilder::new()
                .task("a", "echo a", |b| b.own_sibling("r"))
                .task("b", "echo b", |b| b.own_dependency("a"))
                .task("p", "echo p", |b| b)
                .task("q", "echo q", |b| b.own_dependency("p"))
                .task("r", "echo r", |b| b.own_dependency("q")),
        )
        .unwrap();

        let plan = task_graph.get_batched_execution_plan(|_| Ok(true)).unwrap();

        // Both invariants must hold simultaneously.
        assert_siblings_co_scheduled(&task_graph, &plan);
        assert_topological_order(&task_graph, &plan);

        // Exact schedule: a and r co-scheduled at level 3; b cascaded to 4.
        assert_eq!(
            canonical_batches(&plan),
            vec![
                vec!["proj#p".to_string()],
                vec!["proj#q".to_string()],
                vec!["proj#a".to_string(), "proj#r".to_string()],
                vec!["proj#b".to_string()],
            ]
        );
    }

    #[test]
    fn co_scheduled_sibling_may_be_depended_upon() {
        // The capability Option B preserves: a co-scheduled task (`build`) can
        // simultaneously be an upstream dependency of a downstream task
        // (`test`). `build` and `lint` share a batch; `test` runs after.
        let task_graph = try_build_single_project(
            TasksBuilder::new()
                .task("codegen", "echo codegen", |b| b)
                .task("build", "echo build", |b| {
                    b.own_dependency("codegen").own_sibling("lint")
                })
                .task("lint", "echo lint", |b| b.own_dependency("codegen"))
                .task("test", "echo test", |b| b.own_dependency("build")),
        )
        .unwrap();

        let plan = task_graph.get_batched_execution_plan(|_| Ok(true)).unwrap();

        assert_siblings_co_scheduled(&task_graph, &plan);
        assert_topological_order(&task_graph, &plan);

        assert_eq!(
            canonical_batches(&plan),
            vec![
                vec!["proj#codegen".to_string()],
                vec!["proj#build".to_string(), "proj#lint".to_string()],
                vec!["proj#test".to_string()],
            ]
        );
    }

    #[test]
    fn direct_dependency_between_siblings_is_rejected() {
        // `a ~ b` but `a` also depends on `b`: the pair must be co-scheduled
        // AND strictly ordered -- an unsatisfiable contradiction.
        let task_graph = try_build_single_project(
            TasksBuilder::new()
                .task("a", "echo a", |b| b.own_sibling("b").own_dependency("b"))
                .task("b", "echo b", |b| b),
        )
        .unwrap();

        let err = task_graph
            .get_batched_execution_plan(|_| Ok(true))
            .expect_err("a sibling depending on its own peer is unschedulable");
        assert_eq!(
            err.kind(),
            TaskExecutionGraphErrorKind::SiblingDependencyCycle
        );
    }

    #[test]
    fn transitive_dependency_between_siblings_is_rejected() {
        // `a ~ c` with a dependency path a <- b <- c routed through a
        // non-sibling `b`: still a contradiction, detected via the condensed
        // graph cycle rather than a direct intra-component edge.
        let task_graph = try_build_single_project(
            TasksBuilder::new()
                .task("a", "echo a", |b| b.own_sibling("c"))
                .task("b", "echo b", |b| b.own_dependency("a"))
                .task("c", "echo c", |b| b.own_dependency("b")),
        )
        .unwrap();

        let err = task_graph
            .get_batched_execution_plan(|_| Ok(true))
            .expect_err("a transitive dependency between siblings is a cycle");
        assert_eq!(
            err.kind(),
            TaskExecutionGraphErrorKind::SiblingDependencyCycle
        );
    }
}
