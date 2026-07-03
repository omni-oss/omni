use std::{error::Error, path::Path, sync::Arc, time::Duration};

use bytesize::ByteSize;
use derive_new::new;
use maps::UnorderedMap;
use omni_configurations::MetaConfiguration;
use omni_core::{Project, ProjectGraph};
use thiserror::Error;

type EnvVars = maps::Map<String, String>;

#[derive(new, Default)]
pub struct PruneCacheArgs<'a, TContext: Context = ()> {
    pub dry_run: bool,
    /// Only prune entries whose cached digest no longer matches the freshly
    /// recomputed digest. Requires `context` to be set.
    pub stale_only: bool,
    pub older_than: Option<Duration>,
    /// If not empty, only prune cache for these projects. Otherwise, prune all projects.
    pub project_name_globs: &'a [&'a str],
    /// If not empty, only prune cache for these tasks. Otherwise, prune all tasks.
    pub task_name_globs: &'a [&'a str],
    /// If not empty, only prune cache for projects residing in these directories. Otherwise, prune all directories.
    /// Requires `context` to be set, and only matches tasks present in the current workspace.
    pub dir_globs: &'a [&'a str],
    /// If set, only prune cache for tasks whose meta configuration matches this
    /// CEL expression. Requires `context` to be set, and only matches tasks
    /// present in the current workspace.
    pub meta_filter: Option<&'a str>,
    pub larger_than: Option<ByteSize>,
    /// The loaded workspace context. Required whenever `stale_only`,
    /// `dir_globs`, or `meta_filter` are used, since those filters need the
    /// current workspace configuration (project directories, meta config and
    /// cache inputs) to resolve cached entries.
    pub context: Option<TContext>,
}

#[derive(new, Default)]
pub struct CacheStatsArgs<'a, TContext: Context = ()> {
    /// If not empty, only report stats for these projects. Otherwise, report all projects.
    pub project_name_globs: &'a [&'a str],
    /// If not empty, only report stats for these tasks. Otherwise, report all tasks.
    pub task_name_globs: &'a [&'a str],
    /// If not empty, only report stats for projects residing in these directories.
    /// Requires `context` to be set, and only matches tasks present in the current workspace.
    pub dir_globs: &'a [&'a str],
    /// If set, only report stats for tasks whose meta configuration matches this
    /// CEL expression. Requires `context` to be set, and only matches tasks
    /// present in the current workspace.
    pub meta_filter: Option<&'a str>,
    /// The loaded workspace context. Required whenever `dir_globs` or
    /// `meta_filter` are used.
    pub context: Option<TContext>,
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

    fn get_task_override_args(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&UnorderedMap<String, serde_json::Value>>;

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

    fn get_output_logs(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&omni_task_output_logs::OutputLogsConfiguration>;

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

    fn get_output_logs(
        &self,
        _project_name: &str,
        _task_name: &str,
    ) -> Option<&omni_task_output_logs::OutputLogsConfiguration> {
        panic!("should not be used")
    }

    fn root_dir(&self) -> &Path {
        panic!("should not be used")
    }

    fn get_task_override_args(
        &self,
        _project_name: &str,
        _task_name: &str,
    ) -> Option<&UnorderedMap<String, serde_json::Value>> {
        panic!("should not be used")
    }
}

#[derive(Debug, Error)]
#[error("no error")]
pub struct NoError;
