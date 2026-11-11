use std::{path::PathBuf, time::Duration};

use bytes::Bytes;
use derive_new::new;
use maps::{UnorderedMap, unordered_map};
use omni_cache::{
    CachedTaskExecution, CachedTaskExecutionHash, NewCacheInfo,
    TaskExecutionCacheStore, TaskExecutionInfoExt as _,
};
use omni_collector::{CollectConfig, Collector, CollectorSys, ProjectTaskInfo};
use omni_process::TaskChildProcessResult;
use omni_task_context::TaskContext;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::Force;

#[derive(Debug, bon::Builder, new)]
pub struct CacheManager<TCacheStore, TSys>
where
    TCacheStore: TaskExecutionCacheStore,
    TSys: CollectorSys,
{
    store: TCacheStore,

    #[builder(default = false)]
    dry_run: bool,

    #[builder(default = Force::None)]
    force: Force,

    #[builder(default = false)]
    no_cache: bool,

    sys: TSys,

    root_dir: PathBuf,

    cache_dir: PathBuf,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, new)]
pub enum TaskResultContext<'a> {
    Completed {
        task_context: &'a TaskContext<'a>,
        result: TaskChildProcessResult,
        tries: u8,
    },
    Error {
        task_context: &'a TaskContext<'a>,
        /// The failure as shown to users, already sanitized for the task's
        /// operation. It outlives the task's redactor, so it is redacted
        /// before the operation ends (RFC 0026).
        message: String,
        tries: u8,
    },
}

impl<'a> TaskResultContext<'a> {
    pub fn task_context(&self) -> &TaskContext<'a> {
        match self {
            TaskResultContext::Completed { task_context, .. }
            | TaskResultContext::Error { task_context, .. } => task_context,
        }
    }

    pub fn exit_code(&self) -> u32 {
        match self {
            TaskResultContext::Completed { result, .. } => result.exit_code,
            TaskResultContext::Error { .. } => 1,
        }
    }

    pub fn tries(&self) -> u8 {
        match self {
            TaskResultContext::Completed { tries: t, .. } => *t,
            TaskResultContext::Error { tries: t, .. } => *t,
        }
    }

    pub fn elapsed(&self) -> std::time::Duration {
        match self {
            TaskResultContext::Completed { result, .. } => result.elapsed,
            TaskResultContext::Error { .. } => Duration::ZERO,
        }
    }

    pub fn logs(&self) -> Option<&Bytes> {
        match self {
            TaskResultContext::Completed { result, .. } => result.logs.as_ref(),
            TaskResultContext::Error { .. } => None,
        }
    }
}

impl<TCacheStore, TSys> CacheManager<TCacheStore, TSys>
where
    TCacheStore: TaskExecutionCacheStore,
    TSys: CollectorSys,
{
    pub async fn get_cached_results<'a>(
        &self,
        inputs: &[impl AsRef<TaskContext<'a>>],
    ) -> Result<UnorderedMap<String, CachedTaskExecution>, CacheManagerError>
    {
        if self.force.is_all() {
            return Ok(unordered_map!());
        }

        let inputs = inputs
            .iter()
            .filter(|i| is_cache_eligible(i.as_ref()))
            .filter_map(|i| i.as_ref().execution_info())
            .collect::<Vec<_>>();

        if inputs.is_empty() {
            return Ok(unordered_map!());
        }

        let cached_items = self.store.get_many(&inputs).await.map_err(|e| {
            CacheManagerErrorInner::GetCacheFailed { source: e.into() }
        })?;

        if cached_items.is_empty() {
            return Ok(unordered_map!());
        }

        Ok(cached_items
            .into_iter()
            .filter_map(|c| {
                let c = c?;

                if c.exit_code != 0 && self.force.is_failed() {
                    return None;
                }

                Some((format!("{}#{}", c.project_name, c.task_name), c))
            })
            .collect())
    }

    pub async fn cache_results<'a>(
        &'a self,
        cache_contexts: &'a [TaskResultContext<'a>],
    ) -> Result<
        UnorderedMap<String, CachedTaskExecutionHash<'a>>,
        CacheManagerError,
    > {
        if self.no_cache || cache_contexts.is_empty() {
            return Ok(unordered_map!());
        }

        let to_cache = cache_contexts
            .iter()
            .filter_map(|r| {
                if is_cache_eligible(r.task_context())
                    && let Some(exec_info) = r.task_context().execution_info()
                {
                    Some(NewCacheInfo {
                        execution_duration: r.elapsed(),
                        exit_code: r.exit_code(),
                        task: exec_info,
                        logs: r.logs(),
                        tries: r.tries(),
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        if self.dry_run {
            let collect_task_infos = to_cache
                .iter()
                .map(|info| ProjectTaskInfo {
                    input_files: info.task.input_files,
                    input_files_exclude: info.task.input_files_exclude,
                    output_files: info.task.output_files,
                    output_files_exclude: info.task.output_files_exclude,
                    project_dir: info.task.project_dir,
                    project_name: info.task.project_name,
                    task_exec: info.task.task_exec,
                    task_retry_exec: info.task.task_retry_exec,
                    task_name: info.task.task_name,
                    dependency_digests: info.task.dependency_digests,
                    env_vars: info.task.env_vars,
                    input_env_keys: info.task.input_env_keys,
                    args: info.task.args,
                })
                .collect::<Vec<_>>();

            let collector = Collector::new(
                &self.root_dir,
                &self.cache_dir,
                self.sys.clone(),
            );

            let results = collector
                .collect(
                    &collect_task_infos,
                    &CollectConfig {
                        digests: true,
                        ..Default::default()
                    },
                )
                .await?;

            Ok(results
                .into_iter()
                .map(|c| {
                    (
                        format!("{}#{}", c.task.project_name, c.task.task_name),
                        CachedTaskExecutionHash {
                            digest: c
                                .digest
                                .expect("should have digest at this point"),
                            project_name: c.task.project_name,
                            task_name: c.task.task_name,
                        },
                    )
                })
                .collect())
        } else {
            let cached_items =
                self.store.cache_many(&to_cache).await.map_err(|e| {
                    CacheManagerErrorInner::CacheFailed { source: e.into() }
                })?;

            Ok(cached_items
                .into_iter()
                .map(|c| (format!("{}#{}", c.project_name, c.task_name), c))
                .collect())
        }
    }
}

/// Whether a task may read, replay or write execution-cache entries.
pub(crate) fn is_cache_eligible(task: &TaskContext<'_>) -> bool {
    task.cache_info.as_ref().is_some_and(|ci| ci.cache_enabled)
        && !task.node.persistent()
}

#[derive(Debug, thiserror::Error, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
pub struct CacheManagerError(pub(crate) CacheManagerErrorInner);

impl CacheManagerError {
    #[allow(unused)]
    pub fn kind(&self) -> CacheManagerErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<CacheManagerErrorInner>> From<T> for CacheManagerError {
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

#[derive(
    Debug, thiserror::Error, EnumDiscriminants, omni_diagnostic::Diagnostic,
)]
#[strum_discriminants(name(CacheManagerErrorKind), vis(pub))]
pub(crate) enum CacheManagerErrorInner {
    #[error("failed to get cached results")]
    #[diagnostic(delegate)]
    GetCacheFailed {
        #[source]
        source: eyre::Report,
    },

    #[error("failed to cache results")]
    #[diagnostic(delegate)]
    CacheFailed {
        #[source]
        source: eyre::Report,
    },

    #[error(transparent)]
    #[diagnostic(skip)]
    Collector(#[from] omni_collector::error::Error),
}

#[cfg(test)]
mod tests {
    use std::{borrow::Cow, path::PathBuf, sync::Arc};

    use omni_command_config::CommandConfig;
    use omni_config_types::TeraExprBoolean;
    use omni_core::TaskExecutionNode;
    use omni_task_context::{CacheInfo, TaskContext};

    use super::is_cache_eligible;

    fn node(persistent: bool) -> TaskExecutionNode {
        TaskExecutionNode::new(
            "t".to_string(),
            Some(CommandConfig::Shell("echo".to_string())),
            None,
            "p".to_string(),
            PathBuf::from(""),
            vec![],
            TeraExprBoolean::Boolean(true),
            false,
            persistent,
            None,
            None,
        )
    }

    fn info(enabled: bool) -> CacheInfo {
        CacheInfo {
            cache_enabled: enabled,
            key_defaults: true,
            key_env_keys: vec![],
            key_input_files: Default::default(),
            cache_output_files: Default::default(),
            cache_logs: false,
            args: Default::default(),
        }
    }

    fn eligible(node: &TaskExecutionNode, info: Option<CacheInfo>) -> bool {
        let ctx = TaskContext {
            node,
            dependency_hashes: vec![],
            env_vars: Arc::new(Default::default()),
            cache_info: info.map(Cow::Owned),
            output_logs: None,
            template_context: Default::default(),
        };
        is_cache_eligible(&ctx)
    }

    #[test]
    fn enabled_non_persistent_is_eligible() {
        assert!(eligible(&node(false), Some(info(true))));
    }

    #[test]
    fn disabled_task_is_not_eligible() {
        assert!(!eligible(&node(false), Some(info(false))));
    }

    #[test]
    fn persistent_task_is_not_eligible() {
        assert!(!eligible(&node(true), Some(info(true))));
    }

    #[test]
    fn task_without_cache_info_is_not_eligible() {
        assert!(!eligible(&node(false), None));
    }
}
