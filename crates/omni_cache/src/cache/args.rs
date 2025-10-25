use std::{error::Error, path::Path, sync::Arc, time::Duration};

use bytesize::ByteSize;
use derive_new::new;
use omni_configurations::MetaConfiguration;
use omni_core::{Project, ProjectGraph};
use strum::EnumIs;
use thiserror::Error;

type EnvVars = maps::Map<String, String>;

#[derive(new, Default)]
pub struct PruneCacheArgs<'a, TContext: Context = ()> {
    pub dry_run: bool,
    pub stale_only: PruneStaleOnly<'a, TContext>,
    pub older_than: Option<Duration>,
    /// If not empty, only prune cache for these projects. Otherwise, prune all projects.
    pub project_name_globs: &'a [&'a str],
    /// If not empty, only prune cache for these tasks. Otherwise, prune all tasks.
    pub task_name_globs: &'a [&'a str],
    /// If not empty, only prune cache for projects residing in these directories. Otherwise, prune all directories.
    pub dir_globs: &'a [&'a str],
    pub larger_than: Option<ByteSize>,
}

#[derive(new, Default, EnumIs)]
pub enum PruneStaleOnly<'a, TContext: Context + 'a = ()> {
    #[default]
    Off,
    On {
        context: TContext,
        _phantom: std::marker::PhantomData<&'a ()>,
    },
}

pub trait Context: Send + Sync {
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

    fn get_task_env_vars(
        &self,
        node: &omni_core::TaskExecutionNode,
    ) -> Result<Option<Arc<EnvVars>>, Self::Error>;

    fn get_cache_info(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&omni_task_context::CacheInfo>;

    fn root_dir(&self) -> &Path;
}

impl Context for () {
    type Error = NoError;

    fn get_project_meta_config(
        &self,
        _project_name: &str,
    ) -> Option<&MetaConfiguration> {
        panic!("should not be used")
    }

    fn get_task_meta_config(
        &self,
        _project_name: &str,
        _task_name: &str,
    ) -> Option<&MetaConfiguration> {
        panic!("should not be used")
    }

    fn get_project_graph(&self) -> Result<ProjectGraph, Self::Error> {
        panic!("should not be used")
    }

    fn projects(&self) -> &[Project] {
        panic!("should not be used")
    }

    fn get_task_env_vars(
        &self,
        _node: &omni_core::TaskExecutionNode,
    ) -> Result<Option<Arc<EnvVars>>, Self::Error> {
        panic!("should not be used")
    }

    fn get_cache_info(
        &self,
        _project_name: &str,
        _task_name: &str,
    ) -> Option<&omni_task_context::CacheInfo> {
        panic!("should not be used")
    }

    fn root_dir(&self) -> &Path {
        panic!("should not be used")
    }
}

#[derive(Debug, Error)]
#[error("no error")]
pub struct NoError;
