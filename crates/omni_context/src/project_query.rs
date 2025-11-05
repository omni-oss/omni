use derive_new::new;
use globset::Glob;
use omni_core::{Project, ProjectGraph, ProjectGraphError};
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Clone, Debug, new)]
pub struct ProjectQuery<'a> {
    projects: &'a [Project],
}

impl<'a> ProjectQuery<'a> {
    pub fn all(&self) -> &[Project] {
        self.projects
    }

    pub fn by_name(&self, name: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.name == name)
    }

    pub fn filter_by_glob(
        &self,
        pattern: &str,
    ) -> Result<Vec<&Project>, ProjectQueryError> {
        if pattern == "*" || pattern == "**" {
            return Ok(self.projects.iter().collect());
        }

        let glob = Glob::new(pattern)?;
        let matcher = glob.compile_matcher();

        Ok(self
            .projects
            .iter()
            .filter(|p| matcher.is_match(&p.name))
            .collect())
    }

    pub fn to_graph(&self) -> Result<ProjectGraph, ProjectGraphError> {
        ProjectGraph::from_projects(self.all().to_vec())
    }
}

#[derive(thiserror::Error, Debug, new)]
#[error(transparent)]
pub struct ProjectQueryError(pub(crate) ProjectQueryErrorInner);

impl ProjectQueryError {
    #[allow(unused)]
    pub fn kind(&self) -> ProjectQueryErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ProjectQueryErrorInner>> From<T> for ProjectQueryError {
    fn from(value: T) -> Self {
        let inner = value.into();

        Self::new(inner)
    }
}

#[derive(thiserror::Error, EnumDiscriminants, Debug)]
#[strum_discriminants(vis(pub), name(ProjectQueryErrorKind))]
pub(crate) enum ProjectQueryErrorInner {
    #[error(transparent)]
    Globset(#[from] globset::Error),
    #[error(transparent)]
    ProjectGraph(#[from] ProjectGraphError),
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use maps::OrderedMap;
    use omni_core::Project;

    use crate::project_query::ProjectQuery;

    fn test_fixture() -> Vec<Project> {
        let projects = vec![
            Project::new(
                "project-1",
                PathBuf::new(),
                vec![],
                OrderedMap::new(),
            ),
            Project::new(
                "project-2",
                PathBuf::new(),
                vec![],
                OrderedMap::new(),
            ),
            Project::new(
                "@repo/project-3",
                PathBuf::new(),
                vec![],
                OrderedMap::new(),
            ),
        ];

        return projects;
    }

    #[test]
    fn test_filter_by_glob() {
        let fixture = test_fixture();
        let fixture = ProjectQuery::new(fixture.as_slice());

        let projects = fixture
            .filter_by_glob("project-*")
            .expect("should be able to query by glob");

        assert_eq!(projects.len(), 2, "there should be 2 projects");
        assert_eq!(
            projects.iter().filter(|x| x.name == "project-1").count(),
            1,
            "project-1 should be included"
        );
        assert_eq!(
            projects.iter().filter(|x| x.name == "project-2").count(),
            1,
            "project-2 should be included"
        );
    }

    #[test]
    fn test_filter_by_name() {
        let fixture = test_fixture();
        let fixture = ProjectQuery::new(fixture.as_slice());

        let project = fixture.by_name("project-1");

        assert!(project.is_some(), "project-1 should be retrieved");
    }

    #[test]
    fn test_retrieve_all() {
        let fixture = test_fixture();
        let fixture = ProjectQuery::new(fixture.as_slice());

        assert_eq!(
            fixture.all().len(),
            3,
            "should be able to retrieve all projects"
        );
    }
}
