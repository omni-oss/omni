use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use omni_command_config::CommandConfig;
use omni_config_types::TeraExprBoolean;
use omni_retry_config::RetryConfiguration;
use omni_types::task_label;
pub use omni_types::{TaskKey, TaskKeyRef, TaskLabel, TaskMap};
use petgraph::{
    Direction,
    algo::has_path_connecting,
    graph::{DiGraph, NodeIndex},
    visit::{Dfs, IntoNeighborsDirected as _, Walker},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{EnumDiscriminants, IntoDiscriminant};
use tracing::Level;

use crate::{Project, ProjectGraph, ProjectGraphError};

/// The documented serialized form of a task node. Field names, types and the
/// rendered `dependencies` labels are unchanged. This is a **report**: it can be
/// read back for inspection but never executed; only [`TaskExecutionNode`]
/// built by the graph builder or the `omni exec` and `--ignore-dependencies`
/// paths is executable.
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
#[schemars(rename = "TaskExecutionNode")]
pub struct TaskExecutionNodeReport {
    pub task_name: String,
    pub task_exec: Option<CommandConfig>,
    pub task_retry_exec: Option<CommandConfig>,
    pub project_name: String,
    pub project_dir: PathBuf,
    pub full_task_name: String,
    pub dependencies: Vec<String>,
    pub enabled: TeraExprBoolean,
    pub interactive: bool,
    pub persistent: bool,
    pub retry: RetryConfiguration,
}

/// An executable task node. Identified by its `(project, task)` pair; the
/// ordered dependency list is typed. It is serializable as a
/// [`TaskExecutionNodeReport`] but deliberately not deserializable.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(into = "TaskExecutionNodeReport")]
pub struct TaskExecutionNode {
    key: TaskKey,
    task_exec: Option<CommandConfig>,
    task_retry_exec: Option<CommandConfig>,
    project_dir: PathBuf,
    full_task_name: String,
    dependencies: Vec<TaskKey>,
    enabled: TeraExprBoolean,
    interactive: bool,
    persistent: bool,
    retry: RetryConfiguration,
}

impl From<TaskExecutionNode> for TaskExecutionNodeReport {
    fn from(node: TaskExecutionNode) -> Self {
        Self {
            task_name: node.key.task().to_string(),
            task_exec: node.task_exec,
            task_retry_exec: node.task_retry_exec,
            project_name: node.key.project().to_string(),
            project_dir: node.project_dir,
            full_task_name: node.full_task_name,
            dependencies: node
                .dependencies
                .iter()
                .map(|d| d.label().into_string())
                .collect(),
            enabled: node.enabled,
            interactive: node.interactive,
            persistent: node.persistent,
            retry: node.retry,
        }
    }
}

impl JsonSchema for TaskExecutionNode {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        TaskExecutionNodeReport::schema_name()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        TaskExecutionNodeReport::schema_id()
    }

    fn json_schema(
        generator: &mut schemars::SchemaGenerator,
    ) -> schemars::Schema {
        TaskExecutionNodeReport::json_schema(generator)
    }
}

impl TaskExecutionNode {
    /// Build an executable node. `dependencies` is the explicit, possibly
    /// empty, ordered list of dependency identities.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        key: TaskKey,
        task_exec: Option<CommandConfig>,
        task_retry_exec: Option<CommandConfig>,
        project_dir: impl Into<PathBuf>,
        dependencies: Vec<TaskKey>,
        enabled: TeraExprBoolean,
        interactive: bool,
        persistent: bool,
        retry: RetryConfiguration,
    ) -> Self {
        Self {
            full_task_name: key.label().into_string(),
            key,
            task_exec,
            task_retry_exec,
            project_dir: project_dir.into(),
            dependencies,
            enabled,
            interactive,
            persistent,
            retry,
        }
    }
}

impl TaskExecutionNode {
    pub fn key(&self) -> &TaskKey {
        &self.key
    }

    pub fn key_ref(&self) -> TaskKeyRef<'_> {
        self.key.as_ref()
    }

    pub fn task_name(&self) -> &str {
        self.key.task()
    }

    pub fn task_exec_config(&self) -> Option<&CommandConfig> {
        self.task_exec.as_ref()
    }

    pub fn task_retry_exec_config(&self) -> Option<&CommandConfig> {
        self.task_retry_exec.as_ref()
    }

    pub fn project_name(&self) -> &str {
        self.key.project()
    }

    pub fn project_dir(&self) -> &Path {
        self.project_dir.as_path()
    }

    /// The escaped display label. Presentation only, never an identity.
    pub fn full_task_name(&self) -> &str {
        self.full_task_name.as_str()
    }

    /// The ordered dependency identities, in declaration order with explicit
    /// duplicates kept.
    pub fn dependencies(&self) -> &[TaskKey] {
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

    pub fn retry(&self) -> &RetryConfiguration {
        &self.retry
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, Copy,
)]
pub enum EdgeType {
    Dependency,
    Sibling,
}

type InnerGraph = DiGraph<TaskExecutionNode, EdgeType>;

/// The executable task graph, only produced by the graph builder. `node_map` is
/// an internal index and is not serialized.
#[derive(Debug, Default, Serialize)]
pub struct TaskExecutionGraph {
    #[serde(skip)]
    node_map: TaskMap<NodeIndex>,
    di_graph: InnerGraph,
}

#[derive(Debug, Deserialize)]
struct TaskExecutionGraphReportRaw {
    di_graph: DiGraph<TaskExecutionNodeReport, EdgeType>,
}

/// A deserialized graph: a report-only view. It has no batch planning; use it
/// to inspect nodes and edges. The index is rebuilt from each node's own
/// project and task fields; a legacy serialized `node_map` is ignored and
/// `dependencies` is rebuilt from the `Dependency` edges in edge-table order,
/// never parsed from the serialized labels.
#[derive(Debug, Deserialize)]
#[serde(from = "TaskExecutionGraphReportRaw")]
pub struct TaskExecutionGraphReport {
    node_map: TaskMap<NodeIndex>,
    di_graph: DiGraph<TaskExecutionNodeReport, EdgeType>,
}

impl From<TaskExecutionGraphReportRaw> for TaskExecutionGraphReport {
    fn from(raw: TaskExecutionGraphReportRaw) -> Self {
        let mut di_graph = raw.di_graph;
        let mut node_map = TaskMap::new();
        for index in di_graph.node_indices() {
            let node = &di_graph[index];
            node_map.insert(&node.project_name, &node.task_name, index);
        }
        let labels = di_graph
            .node_indices()
            .map(|i| {
                let n = &di_graph[i];
                (i, task_label(&n.project_name, &n.task_name))
            })
            .collect::<HashMap<_, _>>();
        for index in di_graph.node_indices() {
            di_graph[index].dependencies.clear();
            di_graph[index].full_task_name = labels[&index].clone();
        }
        // Edges run dependee -> dependent; append in edge-table order.
        let edges = di_graph
            .edge_indices()
            .filter_map(|e| {
                let (from, to) = di_graph.edge_endpoints(e)?;
                (di_graph[e] == EdgeType::Dependency).then_some((from, to))
            })
            .collect::<Vec<_>>();
        for (dependee, dependent) in edges {
            let label = labels[&dependee].clone();
            di_graph[dependent].dependencies.push(label);
        }
        Self { node_map, di_graph }
    }
}

/// One edge of a [`TaskExecutionGraphReport`]. For a `Dependency` edge `from`
/// is the dependee, scheduled before `to` (the dependent). For a `Sibling`
/// edge the two tasks are scheduled together; `from` is the referenced
/// sibling and `to` the task that declares it (for example, `watch` declaring
/// `serve` gives `serve -> watch`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskReportEdge<'a> {
    pub from: TaskKeyRef<'a>,
    pub to: TaskKeyRef<'a>,
    pub edge_type: EdgeType,
}

impl TaskExecutionGraphReport {
    /// Every edge, including `Sibling` edges that carry no dependency entry,
    /// in edge-table order.
    pub fn edges(&self) -> impl Iterator<Item = TaskReportEdge<'_>> {
        self.di_graph.edge_indices().filter_map(|e| {
            let (from, to) = self.di_graph.edge_endpoints(e)?;
            let key = |i: NodeIndex| {
                let n = &self.di_graph[i];
                TaskKeyRef::new(&n.project_name, &n.task_name)
            };
            Some(TaskReportEdge {
                from: key(from),
                to: key(to),
                edge_type: self.di_graph[e],
            })
        })
    }

    pub fn count(&self) -> usize {
        self.di_graph.node_count()
    }

    pub fn get_task_by_name(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&TaskExecutionNodeReport> {
        let index = self.node_map.get(project_name, task_name)?;
        Some(&self.di_graph[*index])
    }
}

impl TaskExecutionGraph {
    pub fn new() -> Self {
        Self {
            node_map: TaskMap::new(),
            di_graph: DiGraph::new(),
        }
    }

    pub fn from_project_graph(
        project_graph: &ProjectGraph,
    ) -> TaskExecutionGraphResult<Self> {
        let mut graph = Self::new();

        let projects = project_graph.get_projects_toposorted()?;

        for project in projects.iter() {
            for task_name in project.tasks.keys() {
                TaskKey::try_new(project.name.as_str(), task_name.as_str())
                    .map_err(|e| {
                        TaskExecutionGraphError::invalid_name(
                            &e.project, &e.task,
                        )
                    })?;
            }
        }

        // add all nodes first before adding edges
        for project in projects.iter() {
            let project_name = project.name.as_str();
            let project_dir = project.dir.as_path();

            for task in project.tasks.iter() {
                let task_name = task.0.as_str();
                let task_execution_node = TaskExecutionNode::new(
                    TaskKey::new(project_name, task_name),
                    task.1.exec.clone(),
                    None,
                    project_dir.to_path_buf(),
                    vec![],
                    task.1.enabled.clone(),
                    task.1.interactive,
                    task.1.persistent,
                    task.1.retry.clone(),
                );

                let dep_node_index =
                    graph.di_graph.add_node(task_execution_node.clone());
                graph
                    .node_map
                    .insert(project_name, task_name, dep_node_index);
            }
        }

        // add edges
        for project in projects.iter() {
            for task in project.tasks.iter() {
                let tname = task.0.as_str();

                add_references(
                    project_graph,
                    &mut graph,
                    project,
                    tname,
                    &task.1.dependencies,
                    EdgeType::Dependency,
                )?;
                add_references(
                    project_graph,
                    &mut graph,
                    project,
                    tname,
                    &task.1.siblings,
                    EdgeType::Sibling,
                )?;
            }
        }

        Ok(graph)
    }
}

/// A reference resolved to the canonical task it names. `alias` is set when
/// the written name was an alias of its project, not a canonical task. The
/// names are borrowed so resolving allocates nothing.
#[derive(Clone, Copy)]
struct ResolvedReference<'a> {
    project: &'a str,
    task: &'a str,
    alias: bool,
}

impl ResolvedReference<'_> {
    fn pair(&self) -> (&str, &str) {
        (self.project, self.task)
    }
}

/// The canonical task a public `name` of `project` refers to.
fn resolve_in<'a>(
    project: &'a Project,
    name: &'a str,
) -> Option<ResolvedReference<'a>> {
    if let Some((canonical, _)) = project.tasks.get_key_value(name) {
        return Some(ResolvedReference {
            project: project.name.as_str(),
            task: canonical.as_str(),
            alias: false,
        });
    }
    let canonical = project.canonical_task_name(name)?;
    Some(ResolvedReference {
        project: project.name.as_str(),
        task: canonical,
        alias: true,
    })
}

/// Collect the first match of `task` in each branch of `project`'s upstream
/// projects; a project with no such public name adds nothing and the walk
/// continues into its own predecessors.
///
/// Each project is visited once per lookup, in first-visit order. A revisit
/// could only repeat matches already collected, so skipping it keeps the
/// result while shared branches of a layered graph are not walked once per
/// path. `visited` is created lazily: a lookup that never descends needs no
/// set.
fn collect_upstream<'a>(
    project_graph: &'a ProjectGraph,
    project: &'a Project,
    task: &'a str,
    found: &mut Vec<ResolvedReference<'a>>,
    visited: &mut Option<HashSet<&'a str>>,
) -> Result<(), TaskExecutionGraphError> {
    let dependencies =
        project_graph.get_direct_dependency_refs_by_name(&project.name)?;
    for p in dependencies {
        if let Some(seen) = visited.as_mut()
            && !seen.insert(p.name.as_str())
        {
            continue;
        }
        match resolve_in(p, task) {
            Some(resolved) => found.push(resolved),
            None => {
                visited
                    .get_or_insert_with(HashSet::new)
                    .insert(p.name.as_str());
                collect_upstream(project_graph, p, task, found, visited)?
            }
        }
    }
    Ok(())
}

/// Insert the edges of one task's `references` of one kind.
///
/// Explicit canonical references keep their slots and multiplicity. An alias
/// reference collapses into an explicit slot for the same canonical target
/// whatever the declaration order, and alias references among themselves
/// collapse to the first. A `^name` reference keeps its existing rule: it adds
/// no edge when one of the same kind already connects the pair.
///
/// Names are borrowed throughout and the explicit/upstream sets are built only
/// when some reference resolved through an alias, the only case that reads
/// them.
fn add_references(
    project_graph: &ProjectGraph,
    graph: &mut TaskExecutionGraph,
    project: &Project,
    task_name: &str,
    references: &[crate::TaskDependency],
    edge_type: EdgeType,
) -> Result<(), TaskExecutionGraphError> {
    enum Step<'a> {
        Direct(Option<ResolvedReference<'a>>, &'a str, &'a str),
        Upstream(Vec<ResolvedReference<'a>>),
    }

    let mut any_alias = false;
    let mut steps = Vec::with_capacity(references.len());
    for reference in references {
        steps.push(match reference {
            crate::TaskDependency::Own { task } => {
                let resolved = resolve_in(project, task);
                any_alias |= resolved.is_some_and(|r| r.alias);
                Step::Direct(resolved, project.name.as_str(), task.as_str())
            }
            crate::TaskDependency::ExplicitProject {
                project: target,
                task,
            } => {
                let resolved = project_graph
                    .get_project_by_name(target)
                    .ok()
                    .and_then(|p| resolve_in(p, task));
                any_alias |= resolved.is_some_and(|r| r.alias);
                Step::Direct(resolved, target.as_str(), task.as_str())
            }
            crate::TaskDependency::Upstream { task } => {
                let mut found = vec![];
                collect_upstream(
                    project_graph,
                    project,
                    task,
                    &mut found,
                    &mut None,
                )?;
                any_alias |= found.iter().any(|r| r.alias);
                Step::Upstream(found)
            }
        });
    }

    let mut explicit = HashSet::new();
    let mut upstream = HashSet::new();
    if any_alias {
        for step in &steps {
            match step {
                Step::Direct(Some(resolved), ..) if !resolved.alias => {
                    explicit.insert(resolved.pair());
                }
                Step::Upstream(found) => {
                    upstream.extend(found.iter().map(|r| r.pair()));
                }
                Step::Direct(..) => {}
            }
        }
    }

    let pname = project.name.as_str();
    for step in &steps {
        match step {
            // An unresolved name keeps its literal spelling so the missing
            // task is reported as before.
            &Step::Direct(None, target_project, target_task) => graph
                .add_edge_by_names(
                    pname,
                    task_name,
                    target_project,
                    target_task,
                    edge_type,
                )?,
            &Step::Direct(Some(resolved), ..) if !resolved.alias => graph
                .add_edge_by_names(
                    pname,
                    task_name,
                    resolved.project,
                    resolved.task,
                    edge_type,
                )?,
            &Step::Direct(Some(resolved), ..) => {
                if explicit.contains(&resolved.pair())
                    || upstream.contains(&resolved.pair())
                    || graph.is_connected(
                        pname,
                        task_name,
                        resolved.project,
                        resolved.task,
                        edge_type,
                    )?
                {
                    continue;
                }
                graph.add_edge_by_names(
                    pname,
                    task_name,
                    resolved.project,
                    resolved.task,
                    edge_type,
                )?;
            }
            Step::Upstream(found) => {
                for resolved in found.iter().copied() {
                    if (resolved.alias && explicit.contains(&resolved.pair()))
                        || graph.is_connected(
                            pname,
                            task_name,
                            resolved.project,
                            resolved.task,
                            edge_type,
                        )?
                    {
                        continue;
                    }
                    graph.add_edge_by_names(
                        pname,
                        task_name,
                        resolved.project,
                        resolved.task,
                        edge_type,
                    )?;
                }
            }
        }
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
    /// Whether an edge of `edge_type` already runs from `to` (the dependee)
    /// to `from`. Every connecting edge is considered, so a sibling edge
    /// cannot hide a dependency edge between the same pair, or the reverse.
    fn has_connected_node(
        &self,
        from: NodeIndex,
        to: NodeIndex,
        edge_type: EdgeType,
    ) -> TaskExecutionGraphResult<bool> {
        Ok(self
            .di_graph
            .edges_connecting(to, from)
            .any(|e| *e.weight() == edge_type))
    }

    fn is_connected(
        &self,
        from_project_name: &str,
        from_task_name: &str,
        to_project_name: &str,
        to_task_name: &str,
        edge_type: EdgeType,
    ) -> TaskExecutionGraphResult<bool> {
        let from =
            self.get_task_index_by_name(from_project_name, from_task_name)?;
        let to = self.get_task_index_by_name(to_project_name, to_task_name)?;
        self.has_connected_node(from, to, edge_type)
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
            let to_key = self.di_graph[to_idx].key.clone();
            self.di_graph[from_idx].dependencies.push(to_key);
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
        let index = self.get_task_index_by_name(project_name, task_name)?;

        Ok(&self.di_graph[index])
    }

    #[inline(always)]
    pub fn get_task(
        &self,
        node_index: NodeIndex,
    ) -> TaskExecutionGraphResult<&TaskExecutionNode> {
        Ok(&self.di_graph[node_index])
    }

    #[inline(always)]
    pub fn get_task_index_by_name(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> TaskExecutionGraphResult<NodeIndex> {
        self.node_map
            .get(project_name, task_name)
            .copied()
            .ok_or_else(|| {
                TaskExecutionGraphError::task_not_found(project_name, task_name)
            })
    }

    #[inline(always)]
    pub fn get_direct_dependencies_by_name(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> TaskExecutionGraphResult<Vec<(NodeIndex, TaskExecutionNode)>> {
        let index = self.get_task_index_by_name(project_name, task_name)?;

        self.get_direct_dependencies(index)
    }

    #[inline(always)]
    pub fn get_direct_dependencies_ref_by_name(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> TaskExecutionGraphResult<Vec<(NodeIndex, &TaskExecutionNode)>> {
        let index = self.get_task_index_by_name(project_name, task_name)?;

        self.get_direct_dependencies_ref(index)
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
        Ok(self
            .get_batched_execution_selection_impl(
                filter,
                with_dependents,
                with_dependents_filter,
            )?
            .1)
    }

    /// Like [`Self::get_batched_execution_plan_impl`], also returning the tasks
    /// the filter matched directly, before dependencies or dependents are
    /// added, sorted by identity.
    pub fn get_batched_execution_selection_impl(
        &self,
        filter: impl Fn(&TaskExecutionNode) -> Result<bool, eyre::Report>,
        with_dependents: bool,
        with_dependents_filter: impl Fn(
            &TaskExecutionNode,
        ) -> Result<bool, eyre::Report>,
    ) -> TaskExecutionGraphResult<(Vec<TaskKey>, BatchedExecutionPlan)> {
        // Determine the root nodes. A root node is a node that is not a dependency of any other node
        // Root nodes should always be in the last batch, persistent root nodes should be sorted to the end of the last batch

        // Step 1: Get all nodes that match the predicate, all persistent tasks are considered as roots since no one depends on them
        let mut possible_roots = HashSet::new();
        for node in self.di_graph.node_indices() {
            if filter(&self.di_graph[node])? {
                possible_roots.insert(node);
            }
        }

        let mut direct = possible_roots
            .iter()
            .map(|n| self.di_graph[*n].key.clone())
            .collect::<Vec<_>>();
        direct.sort_by(|a, b| {
            (a.project(), a.task()).cmp(&(b.project(), b.task()))
        });

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

        let plan = ordered_batches
            .into_iter()
            .map(|batch| {
                batch
                    .into_iter()
                    .map(|node| self.di_graph[node].clone())
                    .collect()
            })
            .collect();
        Ok((direct, plan))
    }

    /// The direct matches and the plan, with or without dependents.
    #[allow(clippy::type_complexity)]
    #[tracing::instrument(level = Level::DEBUG, skip_all)]
    pub fn get_batched_execution_selection(
        &self,
        filter: impl Fn(&TaskExecutionNode) -> Result<bool, eyre::Report>,
        with_dependents: Option<
            &dyn Fn(&TaskExecutionNode) -> Result<bool, eyre::Report>,
        >,
    ) -> TaskExecutionGraphResult<(Vec<TaskKey>, BatchedExecutionPlan)> {
        match with_dependents {
            Some(dependents) => self
                .get_batched_execution_selection_impl(filter, true, dependents),
            None => {
                self.get_batched_execution_selection_impl(filter, false, |_| {
                    Ok(false)
                })
            }
        }
    }

    #[tracing::instrument(level = Level::DEBUG, skip_all)]
    pub fn get_batched_execution_plan(
        &self,
        filter: impl Fn(&TaskExecutionNode) -> Result<bool, eyre::Report>,
    ) -> TaskExecutionGraphResult<BatchedExecutionPlan> {
        self.get_batched_execution_plan_impl(filter, false, |_| Ok(false))
    }

    #[tracing::instrument(level = Level::DEBUG, skip_all)]
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

#[derive(Debug, thiserror::Error, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
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
    pub fn invalid_name(project: &str, task: &str) -> Self {
        Self(TaskExecutionGraphErrorInner::InvalidName {
            project: project.to_string(),
            task: task.to_string(),
        })
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

#[derive(
    Debug, thiserror::Error, EnumDiscriminants, omni_diagnostic::Diagnostic,
)]
#[strum_discriminants(name(TaskExecutionGraphErrorKind), vis(pub))]
pub(crate) enum TaskExecutionGraphErrorInner {
    #[error(transparent)]
    #[diagnostic(delegate)]
    ProjectGraph(#[from] ProjectGraphError),

    /// A task named by a selector or dependency does not exist in its project.
    #[error("task '{task}' in project '{project}' not found")]
    #[diagnostic(
        code = "task.not_found",
        hint = "check the task name, or define task `{task}` in project `{project}`"
    )]
    TaskNotFound {
        #[diagnostic(detail)]
        project: String,
        #[diagnostic(detail)]
        task: String,
    },

    /// A project or task name is empty or contains NUL.
    #[error("project {project:?} or task {task:?} is empty or contains NUL")]
    #[diagnostic(
        code = "task.invalid_name",
        hint = "give every project and task a non-empty name without NUL characters"
    )]
    InvalidName {
        #[diagnostic(detail)]
        project: String,
        #[diagnostic(detail)]
        task: String,
    },

    /// Tasks depend on each other in a cycle.
    #[error(
        "cycle detected from '{}' to '{}'",
        task_label(.from_project, .from_task),
        task_label(.to_project, .to_task)
    )]
    #[diagnostic(
        code = "task.cycle_detected",
        hint = "remove the dependency from `{from_task}` to `{to_task}`, or break the cycle elsewhere in the chain"
    )]
    CycleDetected {
        #[diagnostic(detail = "from_project")]
        from_project: String,
        #[diagnostic(detail = "from_task")]
        from_task: String,
        #[diagnostic(detail = "to_project")]
        to_project: String,
        #[diagnostic(detail = "to_task")]
        to_task: String,
    },

    /// A task depends on a persistent task, which cannot be a dependency.
    #[error(
        "can't depend on persistent task '{}' from '{}'",
        task_label(.to_project, .to_task),
        task_label(.from_project, .from_task)
    )]
    #[diagnostic(
        code = "task.persistent_dependency",
        hint = "remove the dependency on persistent task `{to_task}` from `{from_task}`"
    )]
    CantDependOnPersistentTask {
        #[diagnostic(detail = "from_project")]
        from_project: String,
        #[diagnostic(detail = "from_task")]
        from_task: String,
        #[diagnostic(detail = "to_project")]
        to_project: String,
        #[diagnostic(detail = "to_task")]
        to_task: String,
    },

    /// Tasks that run together form a dependency chain among themselves.
    #[error(
        "co-scheduled sibling tasks cannot also form a dependency chain among themselves: {tasks:?}"
    )]
    #[diagnostic(
        code = "task.sibling_cycle",
        hint = "remove the dependency chain among these sibling tasks so they can run together"
    )]
    SiblingDependencyCycle {
        #[diagnostic(detail)]
        tasks: Vec<String>,
    },

    #[error(transparent)]
    #[diagnostic(delegate)]
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
            aliases: Default::default(),
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
        assert_eq!(p1t1_dependency.1.task_name(), "p1t2");
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
            .find(|d| d.1.task_name() == "p3t1")
            .unwrap();
        assert_eq!(
            p1t4_dependency_1.0,
            task_graph
                .get_task_index_by_name("project3", "p3t1")
                .unwrap()
        );
        assert_eq!(p1t4_dependency_1.1.task_name(), "p3t1");
        assert_eq!(p1t4_dependency_1.1.project_name(), "project3");

        let p1t4_dependency_2 = &p1t4_dependencies
            .iter()
            .find(|d| {
                d.1.project_name() == "project1"
                    && d.1.task_name() == "shared-task-3"
            })
            .unwrap();
        assert_eq!(
            p1t4_dependency_2.0,
            task_graph
                .get_task_index_by_name("project1", "shared-task-3")
                .unwrap()
        );
        assert_eq!(p1t4_dependency_2.1.task_name(), "shared-task-3");
        assert_eq!(p1t4_dependency_2.1.project_name(), "project1");
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
            .find(|d| d.1.project_name() == "project2")
            .expect("Should have dependency to project2");
        let p1t2_dependency_2 = &p1t2_dependencies
            .iter()
            .find(|d| d.1.project_name() == "project3")
            .expect("Should have dependency to project3");

        assert_eq!(
            p1t2_dependency_1.0,
            task_graph
                .get_task_index_by_name("project2", "shared-task")
                .unwrap()
        );
        assert_eq!(p1t2_dependency_1.1.task_name(), "shared-task");
        assert_eq!(p1t2_dependency_1.1.project_name(), "project2");

        assert_eq!(
            p1t2_dependency_2.0,
            task_graph
                .get_task_index_by_name("project3", "shared-task")
                .unwrap()
        );
        assert_eq!(p1t2_dependency_2.1.task_name(), "shared-task");
        assert_eq!(p1t2_dependency_2.1.project_name(), "project3");

        assert_eq!(p1t3_dependencies.len(), 1);
        let p1t3_dependency = &p1t3_dependencies[0];
        assert_eq!(
            p1t3_dependency.0,
            task_graph
                .get_task_index_by_name("project3", "shared-task-2")
                .unwrap()
        );
        assert_eq!(p1t3_dependency.1.task_name(), "shared-task-2");
        assert_eq!(p1t3_dependency.1.project_name(), "project3");
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
        TaskExecutionNode::new(
            TaskKey::new(project_name, task_name),
            Some(CommandConfig::Shell(task_exec.to_string())),
            None,
            PathBuf::from(""),
            vec![],
            TeraExprBoolean::Boolean(true),
            false,
            false,
            Default::default(),
        )
    }

    fn node_with_deps(
        task_name: &str,
        task_exec: &str,
        project_name: &str,
        dependencies: &[&str],
    ) -> TaskExecutionNode {
        let mut node = node(task_name, task_exec, project_name);
        node.dependencies = dependencies
            .iter()
            .map(|s| {
                let (p, t) = s.split_once('#').expect("project#task");
                TaskKey::new(p, t)
            })
            .collect();
        node
    }

    #[test]
    fn test_batched_execution_plan() {
        let project_graph = create_project_graph();
        let task_graph =
            TaskExecutionGraph::from_project_graph(&project_graph).unwrap();

        let mut actual_plan = task_graph
            .get_batched_execution_plan(|n| {
                Ok(n.task_name() == "p1t4" || n.task_name() == "sibling-1")
            })
            .unwrap();

        actual_plan.iter_mut().for_each(|batch| {
            batch.sort();
        });

        let mut expected_plan = [
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
            .get_batched_execution_plan(|n| Ok(n.task_name() == "p1t4"))
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
    fn batch_index_by_task(plan: &BatchedExecutionPlan) -> TaskMap<usize> {
        let mut map = TaskMap::new();
        for (i, batch) in plan.iter().enumerate() {
            for node in batch {
                map.insert(node.project_name(), node.task_name(), i);
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

            let dependee = task_graph.di_graph[edge.source()].key_ref();
            let dependent = task_graph.di_graph[edge.target()].key_ref();

            if let (Some(&dependee_batch), Some(&dependent_batch)) =
                (batch_of.get_ref(dependee), batch_of.get_ref(dependent))
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

            let a = task_graph.di_graph[edge.source()].key_ref();
            let b = task_graph.di_graph[edge.target()].key_ref();

            if let (Some(&a_batch), Some(&b_batch)) =
                (batch_of.get_ref(a), batch_of.get_ref(b))
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
                let name = format!("t{j}");
                assert_eq!(
                    batch_of.get("proj", &name).copied(),
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
            .get_batched_execution_plan(|n| Ok(n.task_name() == "main"))
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
            .get_batched_execution_plan(|n| Ok(n.task_name() == "sibling-1"))
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

        let split = |m: &str| {
            let (p, t) = m.split_once('#').unwrap();
            (p.to_string(), t.to_string())
        };
        let (fp, ft) = split(members[0]);
        let first = batch_of
            .get(&fp, &ft)
            .copied()
            .expect("sibling-1 must be scheduled");
        for member in members {
            let (mp, mt) = split(member);
            assert_eq!(
                batch_of.get(&mp, &mt).copied(),
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

    // -- Structured identity (RFC 0024 F03.4 slice 0) --------------------

    fn colliding_project_graph() -> ProjectGraph {
        let ab = Project {
            tasks: TasksBuilder::new().task("c", "echo ab-c", |b| b).build(),
            ..create_project("a#b")
        };
        let a = Project {
            tasks: TasksBuilder::new().task("b#c", "echo a-bc", |b| b).build(),
            ..create_project("a")
        };
        let app = Project {
            tasks: TasksBuilder::new()
                .task("x", "echo x", |b| {
                    b.explicit_project_dependency("a#b", "c")
                })
                .task("y", "echo y", |b| {
                    b.explicit_project_dependency("a", "b#c")
                        .explicit_project_dependency("a#b", "c")
                })
                .build(),
            ..create_project("app")
        };
        ProjectGraph::from_projects(vec![ab, a, app]).expect("graph")
    }

    #[test]
    fn formerly_colliding_pairs_are_distinct_nodes_with_separate_edges() {
        let graph =
            TaskExecutionGraph::from_project_graph(&colliding_project_graph())
                .unwrap();

        assert_eq!(graph.count(), 4);
        let ab = graph.get_task_by_name("a#b", "c").unwrap();
        let a = graph.get_task_by_name("a", "b#c").unwrap();
        assert_ne!(ab.key(), a.key());
        assert_eq!(ab.full_task_name(), r"a\#b#c");
        assert_eq!(a.full_task_name(), r"a#b\#c");
        assert!(graph.get_task_by_name("a", "c").is_err());

        let x = graph.get_task_by_name("app", "x").unwrap();
        assert_eq!(x.dependencies(), &[TaskKey::new("a#b", "c")]);
        let y = graph.get_task_by_name("app", "y").unwrap();
        assert_eq!(
            y.dependencies(),
            &[TaskKey::new("a", "b#c"), TaskKey::new("a#b", "c")]
        );

        let deps = graph.get_direct_dependencies_by_name("app", "x").unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].1.project_name(), "a#b");
    }

    #[test]
    fn insertion_order_does_not_change_pair_identity() {
        let reversed = {
            let ab = Project {
                tasks: TasksBuilder::new()
                    .task("c", "echo ab-c", |b| b)
                    .build(),
                ..create_project("a#b")
            };
            let a = Project {
                tasks: TasksBuilder::new()
                    .task("b#c", "echo a-bc", |b| b)
                    .build(),
                ..create_project("a")
            };
            ProjectGraph::from_projects(vec![a, ab]).unwrap()
        };
        let graph = TaskExecutionGraph::from_project_graph(&reversed).unwrap();
        assert_eq!(
            graph
                .get_task_by_name("a#b", "c")
                .unwrap()
                .task_exec_config(),
            Some(&CommandConfig::Shell("echo ab-c".to_string()))
        );
        assert_eq!(
            graph
                .get_task_by_name("a", "b#c")
                .unwrap()
                .task_exec_config(),
            Some(&CommandConfig::Shell("echo a-bc".to_string()))
        );
    }

    #[test]
    fn colliding_pairs_plan_as_two_nodes_in_one_batch() {
        let graph =
            TaskExecutionGraph::from_project_graph(&colliding_project_graph())
                .unwrap();
        let plan = graph
            .get_batched_execution_plan(|n| Ok(n.project_name() == "app"))
            .unwrap();
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].len(), 2);
        let mut first =
            plan[0].iter().map(|n| n.key().clone()).collect::<Vec<_>>();
        first.sort();
        assert_eq!(
            first,
            vec![TaskKey::new("a", "b#c"), TaskKey::new("a#b", "c")]
        );
    }

    #[test]
    fn serialized_node_keeps_the_documented_shape() {
        let graph =
            TaskExecutionGraph::from_project_graph(&colliding_project_graph())
                .unwrap();
        let y = graph.get_task_by_name("app", "y").unwrap();
        let value = serde_json::to_value(y).unwrap();
        let mut keys = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        keys.sort();
        assert_eq!(
            keys,
            [
                "dependencies",
                "enabled",
                "full_task_name",
                "interactive",
                "persistent",
                "project_dir",
                "project_name",
                "retry",
                "task_exec",
                "task_name",
                "task_retry_exec",
            ]
        );
        assert_eq!(
            value["dependencies"],
            serde_json::json!([r"a#b\#c", r"a\#b#c"])
        );
        assert_eq!(value["full_task_name"], "app#y");
        assert_eq!(value["project_name"], "app");
        assert_eq!(value["task_name"], "y");
    }

    #[test]
    fn graph_serializes_without_the_node_map_and_reads_back_as_a_report() {
        let graph =
            TaskExecutionGraph::from_project_graph(&colliding_project_graph())
                .unwrap();
        let mut value = serde_json::to_value(&graph).unwrap();
        assert!(value.get("node_map").is_none());
        assert!(value.get("di_graph").is_some());

        // A legacy serialized node_map is ignored, and a scrambled
        // dependency list is rebuilt from the edge table.
        value["node_map"] = serde_json::json!({"1": 0});
        for node in value["di_graph"]["nodes"].as_array_mut().unwrap() {
            node["dependencies"] = serde_json::json!(["bogus#label"]);
        }
        let report: TaskExecutionGraphReport =
            serde_json::from_value(value).unwrap();

        assert_eq!(report.count(), 4);
        let ab = report.get_task_by_name("a#b", "c").unwrap();
        let a = report.get_task_by_name("a", "b#c").unwrap();
        assert_ne!(ab.full_task_name, a.full_task_name);
        assert!(report.get_task_by_name("a", "c").is_none());
        let y = report.get_task_by_name("app", "y").unwrap();
        assert_eq!(
            y.dependencies,
            vec![r"a#b\#c".to_string(), r"a\#b#c".to_string()]
        );
        let x = report.get_task_by_name("app", "x").unwrap();
        assert_eq!(x.dependencies, vec![r"a\#b#c".to_string()]);
    }

    #[test]
    fn a_node_with_zero_dependencies_is_a_valid_executable_node() {
        let n = node("t", "echo t", "p");
        assert!(n.dependencies().is_empty());
        assert_eq!(n.key(), &TaskKey::new("p", "t"));
    }

    #[test]
    fn cycle_error_uses_escaped_labels() {
        let err =
            TaskExecutionGraphError::cycle_detected("a#b", "c", "a", "b#c");
        assert_eq!(
            err.to_string(),
            r"cycle detected from 'a\#b#c' to 'a#b\#c'"
        );
    }

    #[test]
    fn persistent_dependency_error_names_the_persistent_task_first() {
        let server = Project {
            tasks: TasksBuilder::new()
                .task("c", "echo server", |b| b.persistent(true))
                .build(),
            ..create_project("a#b")
        };
        let client = Project {
            tasks: TasksBuilder::new()
                .task("b#c", "echo client", |b| {
                    b.explicit_project_dependency("a#b", "c")
                })
                .build(),
            ..create_project("a")
        };
        let project_graph =
            ProjectGraph::from_projects(vec![server, client]).unwrap();
        let err = TaskExecutionGraph::from_project_graph(&project_graph)
            .expect_err("a dependency on a persistent task is rejected");
        assert_eq!(
            err.kind(),
            TaskExecutionGraphErrorKind::CantDependOnPersistentTask
        );
        assert_eq!(
            err.to_string(),
            r"can't depend on persistent task 'a\#b#c' from 'a#b\#c'"
        );
    }

    #[test]
    fn empty_or_nul_names_are_a_graph_error_not_a_panic() {
        for (project, task) in
            [("", "build"), ("p", ""), ("p\0", "build"), ("p", "b\0")]
        {
            let p = Project {
                tasks: TasksBuilder::new().task(task, "echo", |b| b).build(),
                ..create_project(project)
            };
            let project_graph = ProjectGraph::from_projects(vec![p]).unwrap();
            let err = TaskExecutionGraph::from_project_graph(&project_graph)
                .expect_err("invalid names must be rejected");
            assert_eq!(err.kind(), TaskExecutionGraphErrorKind::InvalidName);
        }
    }

    #[test]
    fn report_exposes_sibling_only_edges() {
        let graph = TaskExecutionGraph::from_project_graph(
            &ProjectGraph::from_projects(vec![Project {
                tasks: TasksBuilder::new()
                    .task("serve", "echo serve", |b| b.persistent(true))
                    .task("watch", "echo watch", |b| b.own_sibling("serve"))
                    .build(),
                ..create_project("a#b")
            }])
            .unwrap(),
        )
        .unwrap();
        let value = serde_json::to_value(&graph).unwrap();
        let report: TaskExecutionGraphReport =
            serde_json::from_value(value).unwrap();

        let edges = report.edges().collect::<Vec<_>>();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].edge_type, EdgeType::Sibling);
        assert_eq!(edges[0].from, TaskKeyRef::new("a#b", "serve"));
        assert_eq!(edges[0].to, TaskKeyRef::new("a#b", "watch"));
        assert!(
            report
                .get_task_by_name("a#b", "watch")
                .unwrap()
                .dependencies
                .is_empty()
        );
    }

    #[test]
    fn report_rebuilds_a_legacy_node_label_like_its_references() {
        let graph =
            TaskExecutionGraph::from_project_graph(&colliding_project_graph())
                .unwrap();
        let mut value = serde_json::to_value(&graph).unwrap();
        for node in value["di_graph"]["nodes"].as_array_mut().unwrap() {
            let legacy = format!(
                "{}#{}",
                node["project_name"].as_str().unwrap(),
                node["task_name"].as_str().unwrap()
            );
            node["full_task_name"] = serde_json::json!(legacy);
        }
        let report: TaskExecutionGraphReport =
            serde_json::from_value(value).unwrap();

        let ab = report.get_task_by_name("a#b", "c").unwrap();
        assert_eq!(ab.full_task_name, r"a\#b#c");
        let x = report.get_task_by_name("app", "x").unwrap();
        assert_eq!(x.full_task_name, "app#x");
        assert_eq!(x.dependencies, vec![ab.full_task_name.clone()]);
    }
}

#[cfg(test)]
mod alias_tests {
    use maps::OrderedMap;

    use super::*;
    use crate::{TaskAliases, TasksBuilder};

    fn aliases(pairs: &[(&str, &str)]) -> TaskAliases {
        TaskAliases::new(
            pairs
                .iter()
                .map(|(a, c)| (a.to_string(), c.to_string()))
                .collect::<OrderedMap<_, _>>(),
        )
    }

    fn project(
        name: &str,
        deps: &[&str],
        tasks: OrderedMap<String, crate::Task>,
        alias: &[(&str, &str)],
    ) -> Project {
        Project::new(
            name,
            PathBuf::new(),
            deps.iter().map(|d| d.to_string()).collect(),
            tasks,
        )
        .with_aliases(aliases(alias))
    }

    fn leaf(names: &[&str]) -> OrderedMap<String, crate::Task> {
        names
            .iter()
            .fold(TasksBuilder::new(), |b, n| b.task(*n, "x", |b| b))
            .build()
    }

    fn graph(projects: Vec<Project>) -> TaskExecutionGraph {
        ProjectGraph::from_projects(projects)
            .unwrap()
            .get_task_execution_graph()
            .unwrap()
    }

    fn deps(g: &TaskExecutionGraph, p: &str, t: &str) -> Vec<String> {
        g.get_direct_dependencies_ref_by_name(p, t)
            .unwrap()
            .into_iter()
            .map(|(_, n)| n.key().label().into_string())
            .collect()
    }

    fn sibling_count(g: &TaskExecutionGraph, p: &str, t: &str) -> usize {
        let idx = g.get_task_index_by_name(p, t).unwrap();
        g.di_graph
            .edges_directed(idx, Direction::Incoming)
            .filter(|e| *e.weight() == EdgeType::Sibling)
            .count()
    }

    /// `web` has canonical `build-native` and alias `compile`; `app` owns the
    /// dependent task `t` with the given own-dependency names.
    fn own(refs: &[&str]) -> TaskExecutionGraph {
        let mut builder = TasksBuilder::new().task("build-native", "x", |b| b);
        builder = builder.task("t", "x", |mut b| {
            for r in refs {
                b = b.own_dependency(*r);
            }
            b
        });
        graph(vec![project(
            "web",
            &[],
            builder.build(),
            &[("compile", "build-native")],
        )])
    }

    #[test]
    fn an_alias_reference_makes_a_canonical_edge() {
        let g = own(&["compile"]);
        assert_eq!(deps(&g, "web", "t"), ["web#build-native"]);
    }

    #[test]
    fn aliases_and_canonical_references_collapse_in_either_order() {
        for refs in [["compile", "build-native"], ["build-native", "compile"]] {
            assert_eq!(
                deps(&own(&refs), "web", "t"),
                ["web#build-native"],
                "{refs:?}"
            );
        }
        assert_eq!(deps(&own(&["compile", "compile"]), "web", "t").len(), 1);
    }

    #[test]
    fn explicit_canonical_slots_keep_their_multiplicity() {
        assert_eq!(
            deps(&own(&["build-native", "build-native"]), "web", "t").len(),
            2
        );
        for refs in [
            ["build-native", "build-native", "compile"],
            ["compile", "build-native", "build-native"],
            ["build-native", "compile", "build-native"],
        ] {
            assert_eq!(deps(&own(&refs), "web", "t").len(), 2, "{refs:?}");
        }
    }

    #[test]
    fn an_unknown_name_is_still_a_missing_task() {
        let t = TasksBuilder::new()
            .task("t", "x", |b| b.own_dependency("nope"))
            .build();
        let err =
            ProjectGraph::from_projects(vec![project("web", &[], t, &[])])
                .unwrap()
                .get_task_execution_graph()
                .unwrap_err();
        assert_eq!(err.kind(), TaskExecutionGraphErrorKind::TaskNotFound);
    }

    fn upstream_fixture(
        dep_refs: &[&str],
        with_refs: &[&str],
    ) -> TaskExecutionGraph {
        let t = TasksBuilder::new()
            .task("t", "x", |mut b| {
                for r in dep_refs {
                    b = b.upstream_dependency(*r);
                }
                for r in with_refs {
                    b = b.upstream_sibling(*r);
                }
                b
            })
            .build();
        graph(vec![
            project("app", &["lib", "tool", "none"], t, &[]),
            project("lib", &[], leaf(&["compile"]), &[]),
            project(
                "tool",
                &[],
                leaf(&["build-native"]),
                &[("compile", "build-native")],
            ),
            project("none", &[], leaf(&["other"]), &[]),
        ])
    }

    #[test]
    fn upstream_projects_bind_a_public_name_independently() {
        let g = upstream_fixture(&["compile"], &[]);
        let mut d = deps(&g, "app", "t");
        d.sort();
        assert_eq!(d, ["lib#compile", "tool#build-native"]);
        let g = upstream_fixture(&[], &["compile"]);
        assert_eq!(sibling_count(&g, "app", "t"), 2);
    }

    #[test]
    fn the_upstream_walk_passes_unmatched_projects_and_stops_at_a_match() {
        let t = TasksBuilder::new()
            .task("t", "x", |b| b.upstream_dependency("compile"))
            .build();
        // app -> mid (no match) -> deep (alias match); mid2 matches, so its
        // own deeper match is never reached.
        let g = graph(vec![
            project("app", &["mid", "mid2"], t, &[]),
            project("mid", &["deep"], leaf(&["other"]), &[]),
            project(
                "deep",
                &[],
                leaf(&["build-native"]),
                &[("compile", "build-native")],
            ),
            project("mid2", &["deeper"], leaf(&["compile"]), &[]),
            project("deeper", &[], leaf(&["compile"]), &[]),
        ]);
        let mut d = deps(&g, "app", "t");
        d.sort();
        assert_eq!(d, ["deep#build-native", "mid2#compile"]);
    }

    /// `layers` layers of two projects, each depending on both projects of
    /// the next layer; `matching` lists the (layer, index) projects that own
    /// `compile`. `app` depends on both projects of layer 0.
    fn layered(layers: usize, matching: &[(usize, usize)]) -> Vec<Project> {
        let name = |l: usize, i: usize| format!("l{l}_{i}");
        let t = TasksBuilder::new()
            .task("t", "x", |b| b.upstream_dependency("compile"))
            .build();
        let mut projects = vec![project("app", &["l0_0", "l0_1"], t, &[])];
        for l in 0..layers {
            for i in 0..2 {
                let next = [name(l + 1, 0), name(l + 1, 1)];
                let deps: Vec<&str> = if l + 1 < layers {
                    next.iter().map(String::as_str).collect()
                } else {
                    vec![]
                };
                let tasks = if matching.contains(&(l, i)) {
                    leaf(&["compile"])
                } else {
                    leaf(&["other"])
                };
                projects.push(project(&name(l, i), &deps, tasks, &[]));
            }
        }
        projects
    }

    /// The unmemoised walk: every path is followed and every match stored.
    fn brute_force<'a>(
        pg: &'a ProjectGraph,
        project: &'a Project,
        found: &mut Vec<&'a str>,
    ) {
        for p in pg
            .get_direct_dependency_refs_by_name(&project.name)
            .unwrap()
        {
            if p.tasks.contains_key("compile") {
                found.push(p.name.as_str());
            } else {
                brute_force(pg, p, found);
            }
        }
    }

    fn first_occurrences(all: Vec<&str>) -> Vec<&str> {
        let mut seen = HashSet::new();
        all.into_iter().filter(|n| seen.insert(*n)).collect()
    }

    #[test]
    fn the_upstream_walk_matches_the_unmemoised_walk_in_first_visit_order() {
        let cases: &[&[(usize, usize)]] = &[
            &[(4, 0), (4, 1)],
            &[(4, 1)],
            &[(2, 0), (4, 1)],
            &[(1, 1), (3, 0)],
            &[(0, 0), (3, 1)],
            &[],
        ];
        for matching in cases {
            let pg = ProjectGraph::from_projects(layered(5, matching)).unwrap();
            let app = pg.get_project_by_name("app").unwrap();
            let mut expected = vec![];
            brute_force(&pg, app, &mut expected);
            let mut found = vec![];
            collect_upstream(&pg, app, "compile", &mut found, &mut None)
                .unwrap();
            let got: Vec<&str> = found.iter().map(|r| r.project).collect();
            assert_eq!(
                first_occurrences(got),
                first_occurrences(expected),
                "{matching:?}"
            );
        }
    }

    #[test]
    fn a_deep_layered_diamond_is_walked_once_per_project() {
        let layers = 25;
        let g = graph(layered(layers, &[(layers - 1, 0), (layers - 1, 1)]));
        let mut d = deps(&g, "app", "t");
        d.sort();
        assert_eq!(d, ["l24_0#compile", "l24_1#compile"]);
        let pg =
            ProjectGraph::from_projects(layered(layers, &[(layers - 1, 0)]))
                .unwrap();
        let app = pg.get_project_by_name("app").unwrap();
        let mut found = vec![];
        collect_upstream(&pg, app, "compile", &mut found, &mut None).unwrap();
        // Each project is reached through at most two parents before it is
        // recorded as visited, so the walk stays linear in the layer count.
        assert!(found.len() <= 4, "{}", found.len());
        assert_eq!(found[0].project, "l24_0");
    }

    #[test]
    fn a_diamond_reaches_one_canonical_target_once() {
        let t = TasksBuilder::new()
            .task("t", "x", |b| b.upstream_dependency("compile"))
            .build();
        let g = graph(vec![
            project("app", &["a", "b"], t, &[]),
            project("a", &["c"], leaf(&["x"]), &[]),
            project("b", &["c"], leaf(&["x"]), &[]),
            project(
                "c",
                &[],
                leaf(&["build-native"]),
                &[("compile", "build-native")],
            ),
        ]);
        assert_eq!(deps(&g, "app", "t"), ["c#build-native"]);
    }

    #[test]
    fn an_upstream_alias_collapses_into_an_explicit_canonical_edge() {
        for order in [true, false] {
            let t = TasksBuilder::new()
                .task("t", "x", |b| {
                    if order {
                        b.explicit_project_dependency("tool", "build-native")
                            .upstream_dependency("compile")
                    } else {
                        b.upstream_dependency("compile")
                            .explicit_project_dependency("tool", "build-native")
                    }
                })
                .build();
            let g = graph(vec![
                project("app", &["tool"], t, &[]),
                project(
                    "tool",
                    &[],
                    leaf(&["build-native"]),
                    &[("compile", "build-native")],
                ),
            ]);
            assert_eq!(deps(&g, "app", "t"), ["tool#build-native"], "{order}");
        }
    }

    #[test]
    fn without_aliases_upstream_still_collapses_only_after_an_explicit_edge() {
        let build = |explicit_first: bool| {
            let t = TasksBuilder::new()
                .task("t", "x", |b| {
                    if explicit_first {
                        b.explicit_project_dependency("lib", "compile")
                            .upstream_dependency("compile")
                    } else {
                        b.upstream_dependency("compile")
                            .explicit_project_dependency("lib", "compile")
                    }
                })
                .build();
            graph(vec![
                project("app", &["lib"], t, &[]),
                project("lib", &[], leaf(&["compile"]), &[]),
            ])
        };
        assert_eq!(deps(&build(true), "app", "t").len(), 1);
        assert_eq!(deps(&build(false), "app", "t").len(), 2);
    }

    #[test]
    fn the_no_alias_path_matches_the_general_path() {
        // The second run adds a reference written as the alias `l2` (which
        // names `local`), so `t` really takes the general path with its sets;
        // it collapses into the existing explicit `local` edges, so both runs
        // must produce exactly the same dependency list.
        let build = |with_alias_reference: bool| {
            let t = TasksBuilder::new()
                .task("t", "x", |b| {
                    let b = b
                        .own_dependency("local")
                        .own_dependency("local")
                        .explicit_project_dependency("lib", "compile")
                        .explicit_project_dependency("lib", "compile")
                        .upstream_dependency("compile")
                        .upstream_dependency("compile")
                        .upstream_dependency("compile");
                    if with_alias_reference {
                        b.own_dependency("l2")
                    } else {
                        b
                    }
                })
                .task("local", "x", |b| b)
                .build();
            let alias: &[(&str, &str)] = if with_alias_reference {
                &[("l2", "local")]
            } else {
                &[]
            };
            graph(vec![
                project("app", &["lib"], t, alias),
                project("lib", &[], leaf(&["compile"]), &[]),
            ])
        };
        let plain = deps(&build(false), "app", "t");
        assert_eq!(
            plain,
            ["lib#compile", "lib#compile", "app#local", "app#local"]
        );
        assert_eq!(plain, deps(&build(true), "app", "t"));
    }

    #[test]
    fn a_dependency_and_a_with_edge_to_one_target_both_exist() {
        let g = upstream_fixture(&["compile"], &["compile"]);
        assert_eq!(deps(&g, "app", "t").len(), 2);
        assert_eq!(sibling_count(&g, "app", "t"), 2);
    }

    #[test]
    fn a_connecting_edge_of_another_kind_does_not_hide_the_requested_one() {
        let mut g = TaskExecutionGraph::new();
        let a = TaskExecutionNode::new(
            TaskKey::new("p", "a"),
            None,
            None,
            "",
            vec![],
            Default::default(),
            false,
            false,
            Default::default(),
        );
        let mut b = a.clone();
        b.key = TaskKey::new("p", "b");
        let ia = g.di_graph.add_node(a);
        let ib = g.di_graph.add_node(b);
        g.node_map.insert("p", "a", ia);
        g.node_map.insert("p", "b", ib);
        g.add_edge_by_names("p", "a", "p", "b", EdgeType::Dependency)
            .unwrap();
        g.add_edge_by_names("p", "a", "p", "b", EdgeType::Sibling)
            .unwrap();
        assert!(g.has_connected_node(ia, ib, EdgeType::Dependency).unwrap());
        assert!(g.has_connected_node(ia, ib, EdgeType::Sibling).unwrap());
    }
}

/// Proof for the `with` prerequisites bug: a sibling pulled in by the
/// sibling expansion must bring its own dependencies, scheduled strictly
/// before it, while staying co-scheduled with the task that named it.
#[cfg(test)]
mod with_sibling_prerequisites_tests {
    use std::path::PathBuf;

    use maps::OrderedMap;

    use super::*;
    use crate::{TaskAliases, TasksBuilder};

    fn project(
        name: &str,
        deps: &[&str],
        tasks: OrderedMap<String, crate::Task>,
    ) -> Project {
        Project::new(
            name,
            PathBuf::new(),
            deps.iter().map(|d| d.to_string()).collect(),
            tasks,
        )
    }

    fn build(projects: Vec<Project>) -> TaskExecutionGraph {
        ProjectGraph::from_projects(projects)
            .unwrap()
            .get_task_execution_graph()
            .unwrap()
    }

    fn batches(plan: &BatchedExecutionPlan) -> Vec<Vec<String>> {
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

    fn strs(batches: &[&[&str]]) -> Vec<Vec<String>> {
        batches
            .iter()
            .map(|b| b.iter().map(|s| s.to_string()).collect())
            .collect()
    }

    /// `tool` holds `setup <- build-native`; `app` depends on `tool`.
    fn tool_tasks() -> OrderedMap<String, crate::Task> {
        TasksBuilder::new()
            .task("setup", "x", |b| b)
            .task("build-native", "x", |b| b.own_dependency("setup"))
            .build()
    }

    fn app_with(
        t: impl Fn(crate::TaskBuilder) -> crate::TaskBuilder,
    ) -> TaskExecutionGraph {
        build(vec![
            project(
                "app",
                &["tool"],
                TasksBuilder::new().task("t", "x", t).build(),
            ),
            project("tool", &[], tool_tasks()),
        ])
    }

    fn app_plan(g: &TaskExecutionGraph) -> Vec<Vec<String>> {
        batches(
            &g.get_batched_execution_plan(|n| Ok(n.project_name() == "app"))
                .unwrap(),
        )
    }

    const PREREQ_FIRST: &[&[&str]] =
        &[&["tool#setup"], &["app#t", "tool#build-native"]];

    #[test]
    fn same_project_sibling_brings_its_prerequisite() {
        let g = build(vec![project(
            "proj",
            &[],
            TasksBuilder::new()
                .task("t", "x", |b| b.own_sibling("build-native"))
                .task("setup", "x", |b| b)
                .task("build-native", "x", |b| b.own_dependency("setup"))
                .build(),
        )]);
        // The user filter selects only `t`, which excludes `setup`.
        let plan = g
            .get_batched_execution_plan(|n| Ok(n.task_name() == "t"))
            .unwrap();
        assert_eq!(
            batches(&plan),
            strs(&[&["proj#setup"], &["proj#build-native", "proj#t"]])
        );
    }

    #[test]
    fn sibling_in_another_project_brings_its_prerequisite() {
        let g =
            app_with(|b| b.explicit_project_sibling("tool", "build-native"));
        assert_eq!(app_plan(&g), strs(PREREQ_FIRST));
    }

    #[test]
    fn sibling_reached_through_an_alias_brings_its_prerequisite() {
        let aliases = TaskAliases::new(
            [("native".to_string(), "build-native".to_string())]
                .into_iter()
                .collect::<OrderedMap<_, _>>(),
        );
        let g = build(vec![
            project(
                "app",
                &["tool"],
                TasksBuilder::new()
                    .task("t", "x", |b| {
                        b.explicit_project_sibling("tool", "native")
                    })
                    .build(),
            ),
            project("tool", &[], tool_tasks()).with_aliases(aliases),
        ]);
        assert_eq!(app_plan(&g), strs(PREREQ_FIRST));
    }

    #[test]
    fn sibling_reached_through_upstream_brings_its_prerequisite() {
        let g = app_with(|b| b.upstream_sibling("build-native"));
        assert_eq!(app_plan(&g), strs(PREREQ_FIRST));
    }

    #[test]
    fn deep_prerequisite_chain_of_a_sibling_is_fully_scheduled() {
        let g = build(vec![project(
            "proj",
            &[],
            TasksBuilder::new()
                .task("t", "x", |b| b.own_sibling("build-native"))
                .task("s1", "x", |b| b)
                .task("s2", "x", |b| b.own_dependency("s1"))
                .task("s3", "x", |b| b.own_dependency("s2"))
                .task("build-native", "x", |b| b.own_dependency("s3"))
                .build(),
        )]);
        let plan = g
            .get_batched_execution_plan(|n| Ok(n.task_name() == "t"))
            .unwrap();
        assert_eq!(
            batches(&plan),
            strs(&[
                &["proj#s1"],
                &["proj#s2"],
                &["proj#s3"],
                &["proj#build-native", "proj#t"],
            ])
        );
    }

    /// Regression guard: already passes on the base because `setup` is
    /// reachable from `t` itself; the fix must keep it scheduled once.
    #[test]
    fn prerequisite_shared_with_the_selected_chain_is_scheduled_once() {
        let g = app_with(|b| {
            b.explicit_project_dependency("tool", "setup")
                .explicit_project_sibling("tool", "build-native")
        });
        let plan = app_plan(&g);
        assert_eq!(plan, strs(PREREQ_FIRST));
        assert_eq!(plan.iter().flatten().count(), 3);
    }

    /// The shared prerequisite is only partly in the selected chain: `s1` is
    /// (through `t -> s2 -> s1`), `s0` belongs to the sibling alone.
    #[test]
    fn partly_shared_prerequisites_are_each_scheduled_once() {
        let g = build(vec![project(
            "proj",
            &[],
            TasksBuilder::new()
                .task("s0", "x", |b| b)
                .task("s1", "x", |b| b)
                .task("s2", "x", |b| b.own_dependency("s1"))
                .task("t", "x", |b| {
                    b.own_dependency("s2").own_sibling("build-native")
                })
                .task("build-native", "x", |b| {
                    b.own_dependency("s1").own_dependency("s0")
                })
                .build(),
        )]);
        let plan = g
            .get_batched_execution_plan(|n| Ok(n.task_name() == "t"))
            .unwrap();
        assert_eq!(
            batches(&plan),
            strs(&[
                &["proj#s0", "proj#s1"],
                &["proj#s2"],
                &["proj#build-native", "proj#t"],
            ])
        );
    }

    #[test]
    fn with_dependents_schedules_the_prerequisite_of_a_dependent_sibling() {
        // `app#t` depends on `tool#x` and names `tool#build-native`.
        let g = build(vec![
            project(
                "app",
                &["tool"],
                TasksBuilder::new()
                    .task("t", "x", |b| {
                        b.upstream_dependency("x")
                            .explicit_project_sibling("tool", "build-native")
                    })
                    .build(),
            ),
            project(
                "tool",
                &[],
                TasksBuilder::new()
                    .task("x", "x", |b| b)
                    .task("setup", "x", |b| b)
                    .task("build-native", "x", |b| b.own_dependency("setup"))
                    .build(),
            ),
        ]);
        let plan = g
            .get_batched_execution_plan_with_dependents(
                |n| Ok(n.task_name() == "x"),
                |_| Ok(true),
            )
            .unwrap();
        assert_eq!(
            batches(&plan),
            strs(
                &[&["tool#setup", "tool#x"], &["app#t", "tool#build-native"],]
            )
        );
    }

    #[test]
    fn persistent_task_brings_its_sibling_prerequisite_before_it() {
        let g = build(vec![project(
            "proj",
            &[],
            TasksBuilder::new()
                .task("t", "x", |b| b.own_sibling("dev"))
                .task("setup", "x", |b| b)
                .task("dev", "x", |b| {
                    b.persistent(true).own_dependency("setup")
                })
                .build(),
        )]);
        let plan = g
            .get_batched_execution_plan(|n| Ok(n.task_name() == "t"))
            .unwrap();
        assert_eq!(
            batches(&plan),
            strs(&[&["proj#setup"], &["proj#dev", "proj#t"]])
        );
    }

    #[test]
    fn prerequisite_cycle_through_a_pulled_in_sibling_is_rejected() {
        // `t ~ b`, `b <- p`, `q <- b`, `p ~ q`: once `p` is scheduled its
        // sibling `q` comes with it, and {p, q} both precedes and follows `b`.
        let g = build(vec![project(
            "proj",
            &[],
            TasksBuilder::new()
                .task("t", "x", |b| b.own_sibling("b"))
                .task("b", "x", |b| b.own_dependency("p"))
                .task("p", "x", |b| b.own_sibling("q"))
                .task("q", "x", |b| b.own_dependency("b"))
                .build(),
        )]);
        let err = g
            .get_batched_execution_plan(|n| Ok(n.task_name() == "t"))
            .expect_err("the prerequisite chain loops back through siblings");
        assert_eq!(
            err.kind(),
            TaskExecutionGraphErrorKind::SiblingDependencyCycle
        );
    }
}
