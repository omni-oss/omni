use std::collections::{BTreeSet, HashMap};

use petgraph::{
    Direction,
    algo::{is_cyclic_directed, toposort},
    graph::{DiGraph, NodeIndex},
    prelude::EdgeIndex,
    visit::{DfsPostOrder, EdgeRef, Reversed},
};

use crate::Project;

#[derive(Debug, thiserror::Error)]
#[error("ProjectGraphError: {source}")]
pub struct ProjectGraphError {
    kind: ProjectGraphErrorKind,
    #[source]
    source: ProjectGraphErrorInner,
}

impl ProjectGraphError {
    pub fn already_exists(project_name: &str) -> Self {
        Self {
            kind: ProjectGraphErrorKind::ProjectAlreadyExists,
            source: ProjectGraphErrorInner::ProjectAlreadyExists(
                project_name.to_string(),
            ),
        }
    }

    pub fn not_found(project_name: &str) -> Self {
        Self {
            kind: ProjectGraphErrorKind::ProjectNotFound,
            source: ProjectGraphErrorInner::ProjectNotFound(
                project_name.to_string(),
            ),
        }
    }

    pub fn cyclic_dependency(from: String, to: String) -> Self {
        Self {
            kind: ProjectGraphErrorKind::CyclicDependency,
            source: ProjectGraphErrorInner::CyclicDependency { from, to },
        }
    }

    pub fn cycle_detected(project: String) -> Self {
        Self {
            kind: ProjectGraphErrorKind::CycleDetected,
            source: ProjectGraphErrorInner::CycleDetected { project },
        }
    }

    pub fn unknown(source: eyre::Report) -> Self {
        Self {
            kind: ProjectGraphErrorKind::Unknown,
            source: ProjectGraphErrorInner::Unknown(source),
        }
    }
}

impl ProjectGraphError {
    pub fn kind(&self) -> ProjectGraphErrorKind {
        self.kind
    }
}

#[derive(Debug, thiserror::Error)]
enum ProjectGraphErrorInner {
    #[error("Project with name '{0}' already exists")]
    ProjectAlreadyExists(String),

    #[error("Project '{0}' is not found")]
    ProjectNotFound(String),

    #[error("Adding dependency from '{from}' to '{to}' will create a cycle")]
    CyclicDependency { from: String, to: String },

    #[error("Cycle detected")]
    CycleDetected { project: String },

    #[error(transparent)]
    Unknown(#[from] eyre::Report),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectGraphErrorKind {
    ProjectAlreadyExists,
    ProjectNotFound,
    CyclicDependency,
    CycleDetected,
    Unknown,
}

pub type ProjectGraphResult<T> = Result<T, ProjectGraphError>;

#[derive(Debug, Default)]
pub struct ProjectGraph {
    di_graph: DiGraph<Project, ()>,
    node_map: HashMap<String, NodeIndex>,
}

impl ProjectGraph {
    pub fn new() -> Self {
        Self {
            di_graph: DiGraph::new(),
            node_map: HashMap::new(),
        }
    }

    pub fn from_projects(projects: Vec<Project>) -> ProjectGraphResult<Self> {
        let mut graph = Self::new();

        for project in projects.clone() {
            graph.add_project(project)?;
        }

        for project in projects {
            for dependency in project.dependencies {
                graph.add_dependency_by_name(&project.name, &dependency)?;
            }
        }

        Ok(graph)
    }
}

impl ProjectGraph {
    // #[inline(always)]
    // pub(crate) fn raw_graph(&self) -> &DiGraph<Project, ()> {
    //     &self.di_graph
    // }

    #[inline(always)]
    pub fn count(&self) -> usize {
        self.di_graph.node_count()
    }

    pub fn is_empty(&self) -> bool {
        self.di_graph.node_count() == 0
    }

    fn add_project(
        &mut self,
        project: Project,
    ) -> ProjectGraphResult<NodeIndex> {
        if self.is_project_exists(&project.name) {
            return Err(ProjectGraphError::already_exists(&project.name));
        }

        let project_name = project.name.clone();
        let node_index = self.di_graph.add_node(project);
        self.node_map.insert(project_name.clone(), node_index);

        Ok(node_index)
    }

    pub fn is_project_exists(&self, project_name: &str) -> bool {
        self.node_map.contains_key(project_name)
    }

    fn add_dependency_by_name(
        &mut self,
        dependent: &str,
        dependee: &str,
    ) -> ProjectGraphResult<EdgeIndex> {
        let dependent = self.get_project_index(dependent)?;
        let dependee = self.get_project_index(dependee)?;

        self.add_dependency(dependent, dependee)
    }

    fn add_dependency(
        &mut self,
        dependent: NodeIndex,
        dependee: NodeIndex,
    ) -> ProjectGraphResult<EdgeIndex> {
        let idx = self.di_graph.add_edge(dependee, dependent, ());

        let from_project = self.di_graph[dependent].name.clone();
        let to_project = self.di_graph[dependee].name.clone();

        if is_cyclic_directed(&self.di_graph) {
            self.di_graph.remove_edge(idx);
            return Err(ProjectGraphError::cyclic_dependency(
                from_project,
                to_project,
            ));
        }

        Ok(idx)
    }

    pub fn get_project_index(
        &self,
        project_name: &str,
    ) -> ProjectGraphResult<NodeIndex> {
        let project_index = self
            .node_map
            .get(project_name)
            .ok_or_else(|| ProjectGraphError::not_found(project_name))?;

        Ok(*project_index)
    }

    #[inline(always)]
    pub fn get_direct_dependencies_by_name(
        &self,
        project_name: &str,
    ) -> ProjectGraphResult<Vec<(NodeIndex, Project)>> {
        let project_index = self.get_project_index(project_name)?;

        self.get_direct_dependencies(project_index)
    }

    pub fn get_direct_dependencies(
        &self,
        project_index: NodeIndex,
    ) -> ProjectGraphResult<Vec<(NodeIndex, Project)>> {
        let projects = self
            .di_graph
            .edges_directed(project_index, Direction::Incoming)
            .map(|edge| (edge.source(), self.di_graph[edge.source()].clone()))
            .collect::<Vec<_>>();

        Ok(projects)
    }

    #[inline(always)]
    pub fn get_all_dependencies_by_name(
        &self,
        project_name: &str,
    ) -> ProjectGraphResult<Vec<(NodeIndex, Project)>> {
        let project_index = self.get_project_index(project_name)?;
        self.get_all_dependencies(project_index)
    }

    pub fn get_all_dependencies(
        &self,
        project_index: NodeIndex,
    ) -> ProjectGraphResult<Vec<(NodeIndex, Project)>> {
        let mut visited_idx = BTreeSet::new();
        let reversed_graph = Reversed(&self.di_graph);
        let mut dfs = DfsPostOrder::new(reversed_graph, project_index);

        while let Some(node_index) = dfs.next(reversed_graph) {
            if visited_idx.contains(&node_index) {
                continue;
            }

            visited_idx.insert(node_index);
        }

        Ok(visited_idx
            .iter()
            .filter(|ni| **ni != project_index)
            .map(|node_index| (*node_index, self.di_graph[*node_index].clone()))
            .collect())
    }

    pub fn get_project(
        &self,
        project_index: NodeIndex,
    ) -> ProjectGraphResult<&Project> {
        Ok(&self.di_graph[project_index])
    }

    pub fn get_project_mut(
        &mut self,
        project_index: NodeIndex,
    ) -> ProjectGraphResult<&mut Project> {
        Ok(&mut self.di_graph[project_index])
    }

    pub fn get_project_by_name(
        &self,
        project_name: &str,
    ) -> ProjectGraphResult<&Project> {
        let project_index = self.get_project_index(project_name)?;
        self.get_project(project_index)
    }

    pub fn get_project_by_name_mut(
        &mut self,
        project_name: &str,
    ) -> ProjectGraphResult<&mut Project> {
        let project_index = self.get_project_index(project_name)?;
        self.get_project_mut(project_index)
    }

    pub fn get_projects(&self) -> ProjectGraphResult<Vec<&Project>> {
        let projects = self
            .di_graph
            .node_indices()
            .map(|node_index| &self.di_graph[node_index])
            .collect::<Vec<_>>();

        Ok(projects)
    }

    pub fn get_projects_toposorted(&self) -> ProjectGraphResult<Vec<&Project>> {
        let indices = toposort(&self.di_graph, None).map_err(|c| {
            ProjectGraphError::cycle_detected(
                self.di_graph[c.node_id()].name.clone(),
            )
        })?;

        let projects = indices
            .iter()
            .map(|node_index| &self.di_graph[*node_index])
            .collect::<Vec<_>>();

        Ok(projects)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_project(name: &str) -> Project {
        Project {
            name: name.to_string(),
            dir: Default::default(),
            dependencies: Default::default(),
            tasks: Default::default(),
        }
    }

    #[test]
    fn test_add_project() {
        let mut graph = ProjectGraph::new();

        let project = create_project("project1");

        let project_index = graph.add_project(project).unwrap();

        assert_eq!(graph.count(), 1);
        assert_eq!(graph.get_project_index("project1").unwrap(), project_index);
    }

    #[test]
    fn test_should_not_allow_duplicate_names() {
        let mut graph = ProjectGraph::new();

        let project = create_project("project1");

        graph
            .add_project(project.clone())
            .expect("Can't add project1");

        assert!(
            graph.add_project(project).unwrap_err().kind()
                == ProjectGraphErrorKind::ProjectAlreadyExists,
            "Should not allow duplicate names"
        );
    }

    #[test]
    fn test_add_dependency_using_names() {
        let mut graph = ProjectGraph::new();

        let project1 = create_project("project1");

        let project2 = create_project("project2");

        graph.add_project(project1).expect("Can't add project1");
        graph.add_project(project2).expect("Can't add project2");

        assert!(
            graph.add_dependency_by_name("project1", "project2").is_ok(),
            "Can't add dependency using names"
        );
    }

    #[test]
    fn test_add_dependency_using_index() {
        let mut graph = ProjectGraph::new();

        let project1 = create_project("project1");
        let project2 = create_project("project2");

        let project1_index =
            graph.add_project(project1).expect("Can't add project1");
        let project2_index =
            graph.add_project(project2).expect("Can't add project2");

        assert!(
            graph.add_dependency(project1_index, project2_index).is_ok(),
            "Can't add dependency using index"
        );
    }

    #[test]
    fn test_should_not_allow_adding_dependency_to_project_that_does_not_exist()
    {
        let mut graph = ProjectGraph::new();

        let project1 = create_project("project1");

        graph.add_project(project1).expect("Can't add project1");

        assert!(
            graph
                .add_dependency_by_name("project1", "project2")
                .unwrap_err()
                .kind()
                == ProjectGraphErrorKind::ProjectNotFound,
            "Should not allow adding dependency to project that does not exist"
        );
    }

    #[test]
    fn test_should_not_allow_adding_cyclic_dependency() {
        let mut graph = ProjectGraph::new();

        let project1 = create_project("project1");
        let project2 = create_project("project2");

        let project1_index =
            graph.add_project(project1).expect("Can't add project1");
        let project2_index =
            graph.add_project(project2).expect("Can't add project2");

        graph
            .add_dependency(project1_index, project2_index)
            .expect("Can't add dependency");

        assert!(
            graph
                .add_dependency(project2_index, project1_index)
                .unwrap_err()
                .kind()
                == ProjectGraphErrorKind::CyclicDependency,
            "Should not allow adding cyclic dependency"
        );
    }

    #[test]
    fn test_get_dependencies_using_name() {
        let mut graph = ProjectGraph::new();

        let project1 = create_project("project1");
        let project2 = create_project("project2");
        let project3 = create_project("project3");
        let project4 = create_project("project4");

        let project1_index =
            graph.add_project(project1).expect("Can't add project1");
        let project2_index =
            graph.add_project(project2).expect("Can't add project2");
        let project3_index =
            graph.add_project(project3).expect("Can't add project3");
        let project4_index =
            graph.add_project(project4).expect("Can't add project4");

        graph
            .add_dependency(project1_index, project2_index)
            .expect("Can't add dependency");

        graph
            .add_dependency(project1_index, project3_index)
            .expect("Can't add dependency");

        // To check that we don't get project that is not a dependency
        graph
            .add_dependency(project2_index, project4_index)
            .expect("Can't add dependency");

        let dependencies = graph
            .get_direct_dependencies_by_name("project1")
            .expect("Can't get dependencies");

        assert_eq!(dependencies.len(), 2);
        let first = &dependencies[0];
        let second = &dependencies[1];
        assert_eq!(first.0, project3_index);
        assert_eq!(first.1.name, "project3");
        assert_eq!(second.0, project2_index);
        assert_eq!(second.1.name, "project2");
    }

    #[test]
    fn test_get_all_dependencies() {
        let project1 = Project {
            dependencies: vec!["project2".to_string(), "project3".to_string()],
            ..create_project("project1")
        };
        let project2 = Project {
            dependencies: vec!["project3".to_string()],
            ..create_project("project2")
        };
        let project3 = Project {
            ..create_project("project3")
        };

        let graph =
            ProjectGraph::from_projects(vec![project1, project2, project3])
                .expect("Can't create graph");

        let dependencies = graph
            .get_all_dependencies_by_name("project1")
            .expect("Can't get dependencies");

        assert_eq!(dependencies.len(), 2);

        let first = &dependencies[0];
        let second = &dependencies[1];
        assert_eq!(first.0, graph.get_project_index("project2").unwrap());
        assert_eq!(first.1.name, "project2");

        assert_eq!(second.0, graph.get_project_index("project3").unwrap());
        assert_eq!(second.1.name, "project3");
    }

    #[test]
    fn test_from_projects() {
        fn dep(name: &str) -> String {
            name.to_string()
        }

        let project1 = Project {
            dependencies: vec![dep("project2"), dep("project3")],
            ..create_project("project1")
        };

        let project2 = Project {
            dependencies: vec![dep("project3")],
            ..create_project("project2")
        };

        let project3 = Project {
            dependencies: vec![dep("project4")],
            ..create_project("project3")
        };

        let project4 = create_project("project4");

        let graph = ProjectGraph::from_projects(vec![
            project1, project2, project3, project4,
        ])
        .expect("Can't create graph");

        assert_eq!(graph.count(), 4);

        let project1_dependencies = graph
            .get_direct_dependencies_by_name("project1")
            .expect("Can't get dependencies");

        assert_eq!(project1_dependencies.len(), 2);

        let dep1 = &project1_dependencies[1];
        let dep2 = &project1_dependencies[0];

        assert_eq!(dep1.0, graph.get_project_index("project2").unwrap());
        assert_eq!(dep1.1.name, "project2");
        assert_eq!(dep2.0, graph.get_project_index("project3").unwrap());
        assert_eq!(dep2.1.name, "project3");

        let project2_dependencies = graph
            .get_direct_dependencies_by_name("project2")
            .expect("Can't get dependencies");

        assert_eq!(project2_dependencies.len(), 1);

        let dep1 = &project2_dependencies[0];
        assert_eq!(dep1.0, graph.get_project_index("project3").unwrap());
        assert_eq!(dep1.1.name, "project3");

        let project3_dependencies = graph
            .get_direct_dependencies_by_name("project3")
            .expect("Can't get dependencies");

        assert_eq!(project3_dependencies.len(), 1);

        let dep1 = &project3_dependencies[0];
        assert_eq!(dep1.0, graph.get_project_index("project4").unwrap());
        assert_eq!(dep1.1.name, "project4");
    }

    #[test]
    fn test_is_project_exists() {
        let mut graph = ProjectGraph::new();

        let project1 = create_project("project1");

        graph.add_project(project1).expect("Can't add project1");

        assert!(graph.is_project_exists("project1"), "Project1 should exist");
    }
}
