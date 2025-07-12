use std::collections::HashMap;

use petgraph::{
    algo::is_cyclic_directed,
    graph::{DiGraph, NodeIndex},
    prelude::EdgeIndex,
    visit::EdgeRef,
};

use crate::Project;

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
}

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

    #[error(transparent)]
    Unknown(#[from] eyre::Report),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectGraphErrorKind {
    ProjectAlreadyExists,
    ProjectNotFound,
    CyclicDependency,
    Unknown,
}

pub type ProjectGraphResult<T> = Result<T, ProjectGraphError>;

impl ProjectGraph {
    pub fn add_project(
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

    pub fn add_dependency_using_names(
        &mut self,
        from: &str,
        to: &str,
    ) -> ProjectGraphResult<EdgeIndex> {
        let from_index = self.get_project_index(from)?;
        let to_index = self.get_project_index(to)?;

        self.add_dependency_using_index(from_index, to_index)
    }

    pub fn add_dependency_using_index(
        &mut self,
        from: NodeIndex,
        to: NodeIndex,
    ) -> ProjectGraphResult<EdgeIndex> {
        let idx = self.di_graph.add_edge(from, to, ());

        let from_project = self.di_graph[from].name.clone();
        let to_project = self.di_graph[to].name.clone();

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

    pub fn get_dependencies_using_name(
        &self,
        project_name: &str,
    ) -> ProjectGraphResult<Vec<(NodeIndex, Project)>> {
        let project_index = self.get_project_index(project_name)?;

        self.get_dependencies_using_index(project_index)
    }

    pub fn get_dependencies_using_index(
        &self,
        project_index: NodeIndex,
    ) -> ProjectGraphResult<Vec<(NodeIndex, Project)>> {
        let projects = self
            .di_graph
            .edges(project_index)
            .map(|edge| (edge.target(), self.di_graph[edge.target()].clone()))
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

        assert_eq!(graph.node_map.len(), 1);
        assert_eq!(graph.node_map.get("project1").unwrap(), &project_index);
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
            graph
                .add_dependency_using_names("project1", "project2")
                .is_ok(),
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
            graph
                .add_dependency_using_index(project1_index, project2_index)
                .is_ok(),
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
                .add_dependency_using_names("project1", "project2")
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
            .add_dependency_using_index(project1_index, project2_index)
            .expect("Can't add dependency");

        assert!(
            graph
                .add_dependency_using_index(project2_index, project1_index)
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

        let project1_index =
            graph.add_project(project1).expect("Can't add project1");
        let project2_index =
            graph.add_project(project2).expect("Can't add project2");
        let project3_index =
            graph.add_project(project3).expect("Can't add project3");

        graph
            .add_dependency_using_index(project1_index, project2_index)
            .expect("Can't add dependency");

        graph
            .add_dependency_using_index(project1_index, project3_index)
            .expect("Can't add dependency");

        let dependencies = graph
            .get_dependencies_using_name("project1")
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
    fn test_is_project_exists() {
        let mut graph = ProjectGraph::new();

        let project1 = create_project("project1");

        graph.add_project(project1).expect("Can't add project1");

        assert!(graph.is_project_exists("project1"), "Project1 should exist");
    }
}
