use std::error::Error;

use omni_configurations::MetaConfiguration;
use omni_core::{Project, ProjectGraph, TaskExecutionNode};

use crate::Call;

pub trait ExecutionPlanProvider {
    type Error: Error + Send + Sync + 'static;

    fn get_execution_plan(
        &self,
        call: &Call,
        project_filter: Option<&str>,
        meta_filter: Option<&str>,
        ignore_deps: bool,
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

    fn get_project_graph(&self) -> Result<ProjectGraph, Self::Error>;
    fn projects(&self) -> &[Project];
}
