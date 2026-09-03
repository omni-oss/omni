use std::{error::Error, path::Path};

use omni_configurations::MetaConfiguration;
use omni_core::{Project, ProjectGraph, TaskExecutionNode};
use omni_glob::GlobPatterns;
use omni_types::OmniPath;

use crate::{Call, ScmAffectedFilter};

/// A shared empty include/exclude set to return when a task has no cache info,
/// so [`Context::get_cache_input_files`] can hand back a reference in that case.
pub fn empty_cache_input_files() -> &'static GlobPatterns<OmniPath> {
    static EMPTY: std::sync::LazyLock<GlobPatterns<OmniPath>> =
        std::sync::LazyLock::new(GlobPatterns::default);
    &EMPTY
}

pub trait ExecutionPlanProvider {
    type Error: Error + Send + Sync + 'static;

    #[allow(clippy::result_large_err)]
    fn get_execution_plan(
        &self,
        call: &Call,
        project_filters: &[&str],
        dir_filters: &[&str],
        meta_filter: Option<&str>,
        scm_affected_filter: Option<&ScmAffectedFilter>,
        ignore_deps: bool,
        with_dependents: bool,
    ) -> Result<Vec<Vec<TaskExecutionNode>>, Self::Error>;
}

pub trait ProjectFilter {
    type Error;

    fn should_include_project(
        &self,
        project: &Project,
    ) -> Result<bool, Self::Error>;
}

pub trait TaskFilter {
    type Error;

    fn should_include_task(
        &self,
        node: &TaskExecutionNode,
    ) -> Result<bool, Self::Error>;
}

pub trait Context {
    type Error: Error + Send + Sync + 'static;

    fn get_project_meta_config(
        &self,
        project_name: &str,
    ) -> Option<&MetaConfiguration>;

    fn get_task_meta_config(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&MetaConfiguration>;

    fn get_cache_input_files(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> &GlobPatterns<OmniPath>;

    fn get_project_graph(&self) -> Result<ProjectGraph, Self::Error>;
    fn projects(&self) -> &[Project];
    fn root_dir(&self) -> &Path;
}
