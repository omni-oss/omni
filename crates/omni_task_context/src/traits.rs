use std::{error::Error, sync::Arc};

use omni_core::TaskExecutionNode;
use omni_hasher::impls::DefaultHash;

use crate::{CacheInfo, TaskContext, aliases::EnvVars};

pub trait TaskHashProvider {
    fn get_task_hash(&self, task_full_name: &str) -> Option<DefaultHash>;
}

pub trait Context {
    type Error: Error + Send + Sync + 'static;

    fn get_task_env_vars(
        &self,
        node: &TaskExecutionNode,
    ) -> Result<Option<Arc<EnvVars>>, Self::Error>;

    fn get_task_meta_config(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&omni_configurations::MetaConfiguration>;

    fn get_project_meta_config(
        &self,
        project_name: &str,
    ) -> Option<&omni_configurations::MetaConfiguration>;

    fn get_cache_info(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&CacheInfo>;
}

pub trait TaskContextProvider<'a>: 'a {
    type Error: Error + Send + Sync + 'static;

    fn get_task_context(
        &'a self,
        node: &'a TaskExecutionNode,
        ignore_dependencies: bool,
    ) -> Result<TaskContext<'a>, Self::Error>;
}

pub trait TaskContextProviderExt<'a>: TaskContextProvider<'a> {
    fn get_task_contexts(
        &'a self,
        nodes: &'a [TaskExecutionNode],
        ignore_dependencies: bool,
    ) -> Result<Vec<TaskContext<'a>>, Self::Error> {
        nodes
            .iter()
            .map(|n| self.get_task_context(n, ignore_dependencies))
            .collect()
    }
}

impl<'a, T: TaskContextProvider<'a>> TaskContextProviderExt<'a> for T {}
