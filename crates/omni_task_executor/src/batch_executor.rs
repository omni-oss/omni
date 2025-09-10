use std::time::Duration;

use derive_new::new;
use futures::{AsyncReadExt as _, future::join_all, io::AllowStdIo};
use maps::{UnorderedMap, unordered_map};
use omni_cache::{CachedTaskExecution, TaskExecutionCacheStore};
use omni_core::TaskExecutionNode;
use omni_process::{ChildProcess, ChildProcessResult};
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{
    OnFailure, SkipReason, TaskExecutionResult, TaskExecutorSys,
    cache_manager::{CacheManager, TaskResultContext},
    task_context::TaskContext,
    task_context_provider::TaskContextProvider,
};

#[derive(Debug, thiserror::Error, new)]
pub struct BatchExecutor<TCacheStore, TTaskContextProvider, TSys>
where
    TCacheStore: TaskExecutionCacheStore,
    TTaskContextProvider: TaskContextProvider,
    TSys: TaskExecutorSys,
{
    task_context_provider: TTaskContextProvider,
    cache_manager: CacheManager<TCacheStore>,
    sys: TSys,

    max_concurrent_tasks: usize,
    ignore_dependencies: bool,
    on_failure: OnFailure,
    dry_run: bool,
    replay_cached_logs: bool,
}

impl<TCacheStore, TTaskContextProvider, TSys>
    BatchExecutor<TCacheStore, TTaskContextProvider, TSys>
where
    TCacheStore: TaskExecutionCacheStore,
    TTaskContextProvider: TaskContextProvider,
    TSys: TaskExecutorSys,
{
    fn should_skip_batch(
        &self,
        overall_results: &UnorderedMap<String, TaskExecutionResult>,
    ) -> bool {
        self.on_failure.is_skip_next_batches()
            && overall_results.values().any(|r| r.is_failure())
    }

    fn skipped_results_for_batch(
        &self,
        batch: &[TaskExecutionNode],
    ) -> UnorderedMap<String, TaskExecutionResult> {
        batch
            .iter()
            .map(|t| {
                (
                    t.full_task_name().to_string(),
                    TaskExecutionResult::new_skipped(
                        t.clone(),
                        SkipReason::PreviousBatchFailure,
                    ),
                )
            })
            .collect()
    }

    fn should_skip_task(
        &self,
        node: &TaskExecutionNode,
        overall_results: &UnorderedMap<String, TaskExecutionResult>,
    ) -> bool {
        if self.on_failure.is_continue() {
            return false;
        }

        let fname = node.full_task_name();

        overall_results.get(fname).map_or(false, |r| r.is_failure())
    }

    async fn replay_cached_results(
        &self,
        res: &CachedTaskExecution,
    ) -> Result<(), BatchExecutorError> {
        if self.replay_cached_logs
            && let Some(logs_path) = &res.logs_path
        {
            let file = AllowStdIo::new(
                std::fs::OpenOptions::new().read(true).open(logs_path)?,
            );
            let mut stdout = AllowStdIo::new(std::io::stdout());

            futures::io::copy(&mut file.take(u64::MAX), &mut stdout).await?;
        }

        // hard link the cached files to the original file paths if they don't exist
        if !self.dry_run {
            for file in res.files.iter() {
                let original_path =
                    file.original_path.path().expect("should be resolved");

                if self.sys.fs_exists_async(original_path).await? {
                    continue;
                }

                let dir = original_path.parent().expect("should have parent");
                // check if dir exists
                if !self.sys.fs_exists_async(dir).await? {
                    self.sys.fs_create_dir_all_async(dir).await?;
                }

                self.sys
                    .fs_hard_link_async(
                        file.cached_path.as_path(),
                        original_path,
                    )
                    .await?;
            }
        }

        Ok(())
    }

    pub async fn execute_batch<'a>(
        &mut self,
        batch: &'a [TaskExecutionNode],
        overall_results: &'a UnorderedMap<String, TaskExecutionResult>,
    ) -> Result<UnorderedMap<String, TaskExecutionResult>, BatchExecutorError>
    {
        // skip this batch if any error was encountered in a previous batch
        // when on_failure is set to skip_next_batches
        if self.should_skip_batch(overall_results) {
            let skipped_results = self.skipped_results_for_batch(batch);
            return Ok(skipped_results);
        }

        let task_contexts = self
            .task_context_provider
            .get_task_contexts(batch, self.ignore_dependencies, overall_results)
            .map_err(BatchExecutorErrorInner::new_cant_get_task_contexts)?;

        let cached_results = self
            .cache_manager
            .get_cached_results(&task_contexts)
            .await
            .map_err(BatchExecutorErrorInner::new_cant_get_cached_results)?;

        let mut new_results = unordered_map!(cap: task_contexts.len());
        let mut fut_results = Vec::with_capacity(task_contexts.len());
        let mut futs = Vec::with_capacity(task_contexts.len());

        for task_ctx in &task_contexts {
            if self.should_skip_task(task_ctx.node, overall_results) {
                new_results.insert(
                    task_ctx.node.full_task_name().to_string(),
                    TaskExecutionResult::new_skipped(
                        task_ctx.node.clone(),
                        SkipReason::DependeeTaskFailure,
                    ),
                );
                continue;
            }

            if let Some(cached_result) =
                cached_results.get(task_ctx.node.full_task_name())
            {
                self.replay_cached_results(&cached_result).await?;

                new_results.insert(
                    task_ctx.node.full_task_name().to_string(),
                    TaskExecutionResult::new_completed(
                        Some(cached_result.execution_hash),
                        task_ctx.node.clone(),
                        cached_result.exit_code,
                        cached_result.execution_duration,
                        true,
                    ),
                );

                continue;
            }

            let record_logs =
                task_ctx.cache_info.is_some_and(|ci| ci.cache_logs);

            if self.dry_run {
                trace::info!(
                    "Executing task '{}'",
                    task_ctx.node.full_task_name()
                );
                let node = task_ctx.node.clone();
                fut_results.push(TaskResultContext::new_completed(
                    task_ctx,
                    ChildProcessResult::new(node, 0u32, Duration::ZERO, None),
                ));
            } else {
                futs.push(run_process(task_ctx, record_logs));
            }

            if futs.len() >= self.max_concurrent_tasks {
                fut_results.extend(join_all(futs.drain(..)).await);
            }
        }

        if !futs.is_empty() {
            fut_results.extend(join_all(futs.drain(..)).await);
        }

        let hashes = self
            .cache_manager
            .cache_results(&fut_results)
            .await
            .map_err(BatchExecutorErrorInner::new_cant_cache_results)?;

        for fut_result in &fut_results {
            let fname =
                fut_result.task_context().node.full_task_name().to_string();
            let hash = hashes.get(&fname).map(|h| h.execution_hash);

            let result = match fut_result {
                TaskResultContext::Completed {
                    task_context,
                    result,
                } => TaskExecutionResult::new_completed(
                    hash,
                    task_context.node.clone(),
                    result.exit_code,
                    result.elapsed,
                    false,
                ),
                TaskResultContext::Error {
                    task_context,
                    error,
                } => TaskExecutionResult::new_error(
                    task_context.node.clone(),
                    error.to_string(),
                ),
            };

            new_results.insert(fname, result);
        }

        Ok(new_results)
    }
}

async fn run_process<'a>(
    task_ctx: &'a TaskContext<'a>,
    record_logs: bool,
) -> TaskResultContext<'a> {
    let mut proc = ChildProcess::new(task_ctx.node.clone());
    proc.output_writer(AllowStdIo::new(std::io::stdout()))
        .record_logs(record_logs)
        .env_vars(&task_ctx.env_vars)
        .keep_stdin_open(
            task_ctx.node.persistent() || task_ctx.node.interactive(),
        );
    let result = proc.exec().await;
    if let Err(e) = &result {
        trace::error!(
            "Failed to execute task '{}': {}",
            task_ctx.node.full_task_name(),
            e
        );
    }
    if let Ok(t) = &result {
        if t.success() {
            trace::info!(
                "{}",
                format!("Executed task '{}'", task_ctx.node.full_task_name())
            );
        } else {
            trace::error!(
                "{}",
                format!(
                    "Failed to execute task '{}', exit code '{}'",
                    task_ctx.node.full_task_name(),
                    t.exit_code()
                )
            );
        }
    }
    match result {
        Ok(t) => TaskResultContext::new_completed(task_ctx, t),
        Err(e) => TaskResultContext::new_error(task_ctx, e),
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct BatchExecutorError {
    kind: BatchExecutorErrorKind,
    #[source]
    inner: BatchExecutorErrorInner,
}

impl BatchExecutorError {
    #[allow(unused)]
    pub fn kind(&self) -> BatchExecutorErrorKind {
        self.kind
    }
}

impl<T: Into<BatchExecutorErrorInner>> From<T> for BatchExecutorError {
    fn from(value: T) -> Self {
        let inner = value.into();
        let kind = inner.discriminant();
        Self { inner, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(name(BatchExecutorErrorKind), vis(pub))]
enum BatchExecutorErrorInner {
    #[error("can't get task contexts")]
    CantGetTaskContexts {
        #[new(into)]
        #[source]
        source: eyre::Report,
    },

    #[error("can't get cached results")]
    CantGetCachedResults {
        #[new(into)]
        #[source]
        source: eyre::Report,
    },

    #[error("can't cache results")]
    CantCacheResults {
        #[new(into)]
        #[source]
        source: eyre::Report,
    },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
