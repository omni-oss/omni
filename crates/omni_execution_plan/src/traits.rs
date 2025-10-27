use std::{error::Error, path::Path};

use omni_configurations::MetaConfiguration;
use omni_core::{Project, ProjectGraph, TaskExecutionNode};
use omni_types::OmniPath;

use crate::{Call, ScmAffectedFilter};

pub trait ExecutionPlanProvider {
    type Error: Error + Send + Sync + 'static;

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
    ) -> &[OmniPath];

    fn get_project_graph(&self) -> Result<ProjectGraph, Self::Error>;
    fn projects(&self) -> &[Project];
    fn root_dir(&self) -> &Path;
}
