use std::{borrow::Cow, future::Future, time::Duration};

use futures::stream::{FuturesUnordered, StreamExt as _};
use maps::{UnorderedMap, unordered_map};
use omni_cache::{CachedTaskExecution, TaskExecutionCacheStore};
use omni_config_types::TeraExprBoolean;
use omni_context::LoadedContext;
use omni_core::TaskExecutionNode;
use omni_hasher::impls::DefaultHash;
use omni_messages::{
    CacheHitEvent, DiagnosticLevel, ExecutionEventSubscriber,
    TaskCompletedEvent, TaskFailedEvent, TaskOutputStream,
    TaskOutputStreamEvent, TaskRetryingEvent, TaskSkipReason, TaskSkippedEvent,
    TaskStartedEvent, publish::diagnostic,
};
use omni_process::{
    ChildProcessError, TaskChildProcess, TaskChildProcessResult,
};
use omni_task_context::{TaskContext, TaskContextProviderExt as _};
use omni_task_output_logs::{EffectiveOutputLogs, LogsDisplay};
use omni_types::{OmniPath, Root, RootMap, enum_map};
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use trace::Level;

use crate::{
    OnFailure, SkipReason, TaskDetails, TaskExecutionResult, TaskExecutorSys,
    cache_manager::{CacheManager, TaskResultContext},
    task_context_provider::DefaultTaskContextProvider,
};

pub struct BatchExecutor<'s, TCacheStore, TSys, S>
where
    TCacheStore: TaskExecutionCacheStore,
    TSys: TaskExecutorSys,
    S: ExecutionEventSubscriber,
{
    context: &'s LoadedContext<TSys>,
    cache_manager: CacheManager<TCacheStore, TSys>,
    sys: TSys,
    subscriber: &'s S,
    wants_task_output_stream: bool,
    wants_task_input_stream: bool,
    max_concurrent_tasks: usize,
    ignore_dependencies: bool,
    on_failure: OnFailure,
    dry_run: bool,
    output_logs: Option<LogsDisplay>,
    output_cached_logs: Option<LogsDisplay>,
    max_retries: Option<u8>,
    retry_interval: Option<Duration>,
    no_cache: bool,
    add_task_details: bool,
    args: &'s UnorderedMap<String, serde_json::Value>,
}

impl<'s, TCacheStore, TSys, S> BatchExecutor<'s, TCacheStore, TSys, S>
where
    TCacheStore: TaskExecutionCacheStore,
    TSys: TaskExecutorSys,
    S: ExecutionEventSubscriber,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        context: &'s LoadedContext<TSys>,
        cache_manager: CacheManager<TCacheStore, TSys>,
        sys: TSys,
        subscriber: &'s S,
        max_concurrent_tasks: usize,
        ignore_dependencies: bool,
        on_failure: OnFailure,
        dry_run: bool,
        output_logs: Option<LogsDisplay>,
        output_cached_logs: Option<LogsDisplay>,
        max_retries: Option<u8>,
        retry_interval: Option<Duration>,
        no_cache: bool,
        add_task_details: bool,
        args: &'s UnorderedMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            context,
            cache_manager,
            sys,
            wants_task_output_stream: subscriber.wants_task_output_stream(),
            wants_task_input_stream: subscriber.wants_task_input_stream(),
            subscriber,
            max_concurrent_tasks,
            ignore_dependencies,
            on_failure,
            dry_run,
            output_logs,
            output_cached_logs,
            max_retries,
            retry_interval,
            no_cache,
            add_task_details,
            args,
        }
    }

    fn should_skip_batch_on_error(
        &self,
        overall_results: &UnorderedMap<String, TaskExecutionResult>,
    ) -> bool {
        self.on_failure.is_skip_next_batches()
            && overall_results.values().any(|r| r.is_failure())
    }

    fn skipped_results_due_to_error_for_batch(
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

    fn should_skip_task_on_error(
        &self,
        task_ctx: &TaskContext<'_>,
        overall_results: &UnorderedMap<String, TaskExecutionResult>,
    ) -> Option<String> {
        if self.on_failure.is_continue() {
            return None;
        }

        task_ctx
            .node
            .dependencies()
            .iter()
            .find(|d| overall_results.get(*d).is_some_and(|r| r.is_failure()))
            .cloned()
    }

    fn resolve_output_logs(
        &self,
        task_ctx: &TaskContext<'_>,
    ) -> EffectiveOutputLogs {
        EffectiveOutputLogs::resolve(
            self.output_logs,
            self.output_cached_logs,
            task_ctx.output_logs.as_ref(),
            LogsDisplay::Failed,
        )
    }

    async fn replay_cached_results(
        &self,
        task_ctx: &TaskContext<'_>,
        res: &CachedTaskExecution,
    ) -> Result<(), BatchExecutorError> {
        let effective = self.resolve_output_logs(task_ctx);
        let cached_failed = res.exit_code != 0;
        let should_replay = !self.dry_run
            && self.wants_task_output_stream
            && effective.cached.should_show(cached_failed);

        // Emit cache hit event
        self.subscriber
            .on_cache_hit(CacheHitEvent {
                task_id: task_ctx.node.full_task_name().to_string(),
                project: task_ctx.node.project_name().to_string(),
                task: task_ctx.node.task_name().to_string(),
                digest: res.digest.to_vec(),
                replay_logs: should_replay,
                has_logs: res.logs_path.is_some(),
            })
            .await;

        if should_replay && let Some(logs_path) = &res.logs_path {
            let file = tokio::fs::OpenOptions::new()
                .read(true)
                .open(logs_path)
                .await?;

            self.subscriber
                .on_task_output_stream(TaskOutputStreamEvent {
                    task_id: task_ctx.node.full_task_name().to_string(),
                    project: task_ctx.node.project_name().to_string(),
                    task: task_ctx.node.task_name().to_string(),
                    is_replay: true,
                    is_interactive: false,
                    output_logs: effective,
                    stream: TaskOutputStream {
                        reader: Box::new(file),
                        writer: None,
                    },
                })
                .await;
        }

        // hard link the cached files to the original file paths if they don't exist
        if !self.dry_run {
            for file in res.files.iter() {
                let original_path =
                    file.original_path.path().expect("should be resolved");

                if self.sys.fs_exists_async(original_path).await? {
                    diagnostic!(
                        self.subscriber,
                        DiagnosticLevel::Debug,
                        "file already exists {original_path:?}, skipping cache restore",
                    ).await;
                    continue;
                }

                let dir = original_path.parent().expect("should have parent");
                if !self.sys.fs_exists_async(dir).await? {
                    self.sys.fs_create_dir_all_async(dir).await?;
                }

                self.sys
                    .fs_hard_link_async(
                        file.cached_path.as_path(),
                        original_path,
                    )
                    .await?;

                diagnostic!(
                    self.subscriber,
                    DiagnosticLevel::Debug,
                    "restored cached file {:?} to {:?}",
                    file.cached_path,
                    original_path,
                )
                .await;
            }
        }

        Ok(())
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(level = Level::DEBUG, skip_all))]
    fn expand_templates<'a>(
        &self,
        task_contexts: impl IntoIterator<Item = &'a TaskContext<'a>>,
    ) -> Result<Vec<Cow<'a, TaskContext<'a>>>, BatchExecutorError> {
        let mut new_task_contexts = vec![];

        for task_ctx in task_contexts {
            if let Some(ci) = task_ctx.cache_info.as_ref() {
                if (!ci.cache_output_files.is_empty()
                    && ci
                        .cache_output_files
                        .iter()
                        .any(|fi| omni_path_contains_byte(fi, b'{')))
                    || (!ci.key_input_files.is_empty()
                        && ci
                            .key_input_files
                            .iter()
                            .any(|fi| omni_path_contains_byte(fi, b'{')))
                {
                    let mut new_ci = ci.as_ref().clone();

                    if !ci.cache_output_files.is_empty() {
                        let mut new_files =
                            Vec::with_capacity(ci.cache_output_files.len());

                        trace::trace!(
                            ?ci.cache_output_files,
                            "original_cache_output_files"
                        );

                        for file in &ci.cache_output_files {
                            let expanded = expand_omni_path(
                                file,
                                &task_ctx.template_context,
                            )?;
                            new_files.push(expanded);
                        }

                        new_ci.cache_output_files = new_files;
                        trace::trace!(
                            ?new_ci.cache_output_files,
                            "expanded_cache_output_files"
                        );
                    }

                    if !ci.key_input_files.is_empty() {
                        let mut new_files =
                            Vec::with_capacity(ci.key_input_files.len());

                        trace::trace!(
                            ?ci.key_input_files,
                            "original_key_input_files"
                        );

                        for file in &ci.key_input_files {
                            let expanded = expand_omni_path(
                                file,
                                &task_ctx.template_context,
                            )?;
                            new_files.push(expanded);
                        }

                        new_ci.key_input_files = new_files;
                        trace::trace!(
                            ?new_ci.key_input_files,
                            "expanded_key_input_files"
                        );
                    }
                    let mut new_ctx = task_ctx.clone();
                    new_ctx.cache_info = Some(Cow::Owned(new_ci));
                    new_task_contexts.push(Cow::Owned(new_ctx));
                } else {
                    new_task_contexts.push(Cow::Borrowed(task_ctx));
                }
            } else {
                new_task_contexts.push(Cow::Borrowed(task_ctx));
            }
        }

        Ok(new_task_contexts)
    }

    fn assign_task_details(
        &self,
        result: &mut TaskExecutionResult,
        bases: &RootMap,
        task_ctx: Option<&TaskContext>,
    ) {
        let task_name = result.task().task_name().to_string();
        let project_name = result.task().project_name().to_string();
        if result.details().is_none() {
            result.set_details(TaskDetails::default());
        }
        let details = result.details_mut();
        if let Some(details) = details {
            if details.meta.is_none() {
                details.meta = self
                    .context
                    .get_task_meta_config(&project_name, &task_name)
                    .or_else(|| {
                        self.context.get_project_meta_config(&project_name)
                    })
                    .cloned();
            }

            if let Some(ci) = task_ctx.and_then(|t| t.cache_info.as_ref()) {
                if details.output_files.is_none() {
                    details.output_files = Some(
                        ci.cache_output_files
                            .iter()
                            .map(|f| f.resolve(bases).into_owned())
                            .collect(),
                    );
                }

                if details.cache_key_input_files.is_none() {
                    details.cache_key_input_files = Some(
                        ci.key_input_files
                            .iter()
                            .map(|f| f.resolve(bases).into_owned())
                            .collect(),
                    );
                }
            }
        }
    }

    async fn execute_batch_inner<'a>(
        &mut self,
        batch: &'a [TaskExecutionNode],
        task_contexts: &'a [Cow<'a, TaskContext<'a>>],
        overall_results: &'a UnorderedMap<String, TaskExecutionResult>,
    ) -> Result<UnorderedMap<String, TaskExecutionResult>, BatchExecutorError>
    where
        's: 'a,
    {
        // skip this batch if any error was encountered in a previous batch
        // when on_failure is set to skip_next_batches
        if self.should_skip_batch_on_error(overall_results) {
            for task in batch {
                self.subscriber
                    .on_task_skipped(TaskSkippedEvent {
                        task_id: task.full_task_name().to_string(),
                        project: task.project_name().to_string(),
                        task: task.task_name().to_string(),
                        reason: TaskSkipReason::PreviousBatchFailure,
                        dependency: None,
                    })
                    .await;
            }

            let skipped_results =
                self.skipped_results_due_to_error_for_batch(batch);
            return Ok(skipped_results);
        }

        let cached_results = self
            .cache_manager
            .get_cached_results(&task_contexts)
            .await
            .map_err(BatchExecutorErrorInner::new_cant_get_cached_results)?;

        let mut new_results = unordered_map!(cap: task_contexts.len());
        let mut fut_results = Vec::with_capacity(task_contexts.len());
        let mut futs = Vec::with_capacity(task_contexts.len());

        for task_ctx in task_contexts {
            if task_ctx.node.task_exec().is_none() {
                new_results.insert(
                    task_ctx.node.full_task_name().to_string(),
                    TaskExecutionResult::new_skipped(
                        task_ctx.node.clone(),
                        SkipReason::NoCommand,
                    ),
                );

                self.subscriber
                    .on_task_skipped(TaskSkippedEvent {
                        task_id: task_ctx.node.full_task_name().to_string(),
                        project: task_ctx.node.project_name().to_string(),
                        task: task_ctx.node.task_name().to_string(),
                        reason: TaskSkipReason::NoCommand,
                        dependency: None,
                    })
                    .await;
                continue;
            }

            let should_run = evaluate_bool_expr(
                task_ctx.node.enabled(),
                &task_ctx.template_context,
            )?;
            if !should_run {
                new_results.insert(
                    task_ctx.node.full_task_name().to_string(),
                    TaskExecutionResult::new_skipped(
                        task_ctx.node.clone(),
                        SkipReason::Disabled,
                    ),
                );

                self.subscriber
                    .on_task_skipped(TaskSkippedEvent {
                        task_id: task_ctx.node.full_task_name().to_string(),
                        project: task_ctx.node.project_name().to_string(),
                        task: task_ctx.node.task_name().to_string(),
                        reason: TaskSkipReason::Disabled,
                        dependency: None,
                    })
                    .await;
                continue;
            }

            if let Some(failed_dep) =
                self.should_skip_task_on_error(task_ctx, overall_results)
            {
                new_results.insert(
                    task_ctx.node.full_task_name().to_string(),
                    TaskExecutionResult::new_skipped(
                        task_ctx.node.clone(),
                        SkipReason::DependeeTaskFailure,
                    ),
                );
                self.subscriber
                    .on_task_skipped(TaskSkippedEvent {
                        task_id: task_ctx.node.full_task_name().to_string(),
                        project: task_ctx.node.project_name().to_string(),
                        task: task_ctx.node.task_name().to_string(),
                        reason: TaskSkipReason::DependeeTaskFailure,
                        dependency: Some(failed_dep),
                    })
                    .await;
                continue;
            }

            if let Some(cached_result) =
                cached_results.get(task_ctx.node.full_task_name())
            {
                new_results.insert(
                    task_ctx.node.full_task_name().to_string(),
                    TaskExecutionResult::new_completed(
                        cached_result.digest,
                        task_ctx.node.clone(),
                        cached_result.exit_code,
                        cached_result.execution_duration,
                        true,
                        cached_result.tries,
                    ),
                );

                self.replay_cached_results(task_ctx, &cached_result).await?;

                continue;
            }

            let record_logs =
                task_ctx.cache_info.as_ref().is_some_and(|ci| ci.cache_logs);

            if self.dry_run {
                diagnostic!(
                    self.subscriber,
                    DiagnosticLevel::Info,
                    "Executing task '{}'",
                    task_ctx.node.full_task_name(),
                )
                .await;

                let node = task_ctx.node.clone();
                fut_results.push(TaskResultContext::new_completed(
                    task_ctx,
                    TaskChildProcessResult::new(
                        node,
                        0u32,
                        Duration::ZERO,
                        None,
                    ),
                    0,
                ));
            } else {
                let override_command = get_expanded_override_command(
                    task_ctx.node.task_exec(),
                    "command",
                    task_ctx,
                )?;

                let override_retry_command = get_expanded_override_command(
                    task_ctx.node.task_retry_exec(),
                    "retry_command",
                    task_ctx,
                )?;
                futs.push(run_process(
                    self.subscriber,
                    self.wants_task_output_stream,
                    self.wants_task_input_stream,
                    task_ctx,
                    override_command,
                    override_retry_command,
                    record_logs,
                    self.resolve_output_logs(task_ctx),
                    self.max_retries
                        .unwrap_or(task_ctx.node.max_retries().unwrap_or(0)),
                    self.retry_interval.or(task_ctx.node.retry_interval()),
                ));
            }
        }

        // Run the batch's tasks with at most `max_concurrent_tasks` in flight,
        // refilling a slot the instant any task finishes (no `join_all` convoy
        // stalls where a straggler idles the other slots). The inter-batch
        // barrier is unaffected: the pipeline still awaits the whole batch
        // before starting the next one, so cross-batch dependency ordering is
        // preserved exactly as before.
        fut_results.extend(run_bounded(futs, self.max_concurrent_tasks).await);

        let hashes = self
            .cache_manager
            .cache_results(&fut_results)
            .await
            .map_err(BatchExecutorErrorInner::new_cant_cache_results)?;

        for fut_result in &fut_results {
            let fname =
                fut_result.task_context().node.full_task_name().to_string();
            let hash = if self.no_cache
                // never cache persistent tasks
                || fut_result.task_context().node.persistent()
                // never cache tasks that are not enabled
                || fut_result
                    .task_context()
                    .cache_info
                    .as_ref()
                    .is_some_and(|ci| !ci.cache_enabled)
            {
                DefaultHash::default()
            } else {
                hashes.get(&fname).map(|h| h.digest).ok_or_else(|| {
                    BatchExecutorErrorInner::new_cant_get_task_hash(
                        fname.clone(),
                    )
                })
                .inspect_err(|_| {
                    // Fire-and-forget diagnostic; we're in a sync context so we
                    // can't await here. Use log:: as a fallback.
                    log::error!(
                        "Failed to get hash for task '{}', this is a bug, if you see this please report it to the maintainers",
                        fname
                    );
                })?
            };

            let result = match fut_result {
                TaskResultContext::Completed {
                    task_context,
                    result,
                    tries,
                } => TaskExecutionResult::new_completed(
                    hash,
                    task_context.node.clone(),
                    result.exit_code,
                    result.elapsed,
                    false,
                    *tries,
                ),
                TaskResultContext::Error {
                    task_context,
                    error,
                    tries,
                } => TaskExecutionResult::new_errored(
                    task_context.node.clone(),
                    error.to_string(),
                    *tries,
                ),
            };

            new_results.insert(fname, result);
        }

        Ok(new_results)
    }

    #[cfg_attr(
        feature = "enable-tracing",
        tracing::instrument(
            level = Level::DEBUG,
            skip_all,
            fields(batch_size = batch.len(), overall_results_count = overall_results.len())
        )
    )]
    pub async fn execute_batch<'a>(
        &mut self,
        batch: &'a [TaskExecutionNode],
        overall_results: &'a UnorderedMap<String, TaskExecutionResult>,
    ) -> Result<UnorderedMap<String, TaskExecutionResult>, BatchExecutorError>
    {
        let ctx_provider = DefaultTaskContextProvider::new(
            self.context,
            overall_results,
            Some(self.args),
        );

        let tmp_task_contexts = ctx_provider
            .get_task_contexts(batch, self.ignore_dependencies)
            .map_err(BatchExecutorErrorInner::new_cant_get_task_contexts)?;
        let task_contexts = self.expand_templates(&tmp_task_contexts)?;

        let mut new_results = self
            .execute_batch_inner(batch, &task_contexts, overall_results)
            .await?;

        if self.add_task_details {
            let task_ctx_map = task_contexts
                .iter()
                .map(|t| (t.node.full_task_name(), t))
                .collect::<UnorderedMap<_, _>>();
            let ws_dir = self.context.root_dir();
            for result in new_results.values_mut() {
                let task_ctx = task_ctx_map.get(result.task().full_task_name());
                let project_dir = task_ctx
                    .map(|t| t.node.project_dir())
                    .expect("should have project dir, if seen please report this as a bug");

                let root_map = enum_map! {
                    Root::Project => project_dir,
                    Root::Workspace => ws_dir.into(),
                };
                self.assign_task_details(
                    result,
                    &root_map,
                    task_ctx.map(|t| t.as_ref()),
                );
            }
        }

        Ok(new_results)
    }
}

fn get_expanded_override_command<'a>(
    command: Option<&'a str>,
    template_name: &str,
    task_ctx: &Cow<'_, TaskContext<'a>>,
) -> Result<Option<String>, BatchExecutorError> {
    let override_command = if let Some(cmd) = command {
        let expanded =
            omni_tera::one_off(cmd, template_name, &task_ctx.template_context)?;
        if expanded != cmd {
            Some(expanded)
        } else {
            None
        }
    } else {
        None
    };
    Ok(override_command)
}

fn evaluate_bool_expr(
    expr: &TeraExprBoolean,
    context: &omni_tera::Context,
) -> omni_tera::Result<bool> {
    Ok(match expr {
        TeraExprBoolean::Boolean(b) => *b,
        TeraExprBoolean::Expr(expr) => {
            let normal = expr.as_str().trim().to_lowercase();
            match normal.as_str() {
                "true" | "yes" | "y" | "1" => return Ok(true),
                "false" | "no" | "n" | "0" => return Ok(false),
                _ => {}
            }

            let result = omni_tera::one_off(expr.as_str(), "expr", &context)?;

            result.trim().to_lowercase() == "true"
        }
    })
}

fn omni_path_contains_byte(path: &OmniPath, byte: u8) -> bool {
    path.unresolved_path()
        .as_os_str()
        .as_encoded_bytes()
        .iter()
        .any(|b| *b == byte)
}

fn expand_omni_path(
    path: &OmniPath,
    context: &omni_tera::Context,
) -> omni_tera::Result<OmniPath> {
    let text = path.unresolved_path().to_str().unwrap_or("");
    let root = path.root();

    let expanded = omni_tera::one_off(&text, "path", context)?;

    Ok(if let Some(root) = root {
        OmniPath::new_rooted(expanded, root)
    } else {
        OmniPath::new(expanded)
    })
}

/// Drives every future in `futures` to completion while keeping at most
/// `max_concurrency` (clamped to a minimum of 1) in flight at once, returning
/// all of their outputs.
///
/// A permit is acquired before a future is allowed to make progress and is
/// released the instant it completes, so a freed slot is refilled immediately
/// rather than waiting for a fixed-size chunk to drain (which would idle slots
/// behind a straggler). Outputs are returned in completion order; callers that
/// need a stable result must key them (the batch executor keys by task name),
/// which keeps the overall result deterministic regardless of the order in
/// which tasks happen to finish.
///
/// Invariants (see tests):
/// - the number of futures past the permit gate never exceeds
///   `max(max_concurrency, 1)`;
/// - every future is polled to completion exactly once and all outputs are
///   returned;
/// - an empty input completes immediately without acquiring anything.
async fn run_bounded<F>(
    futures: Vec<F>,
    max_concurrency: usize,
) -> Vec<F::Output>
where
    F: Future,
{
    if futures.is_empty() {
        return Vec::new();
    }

    let semaphore = tokio::sync::Semaphore::new(max_concurrency.max(1));
    let mut running = FuturesUnordered::new();

    for fut in futures {
        let semaphore = &semaphore;
        running.push(async move {
            let _permit = semaphore
                .acquire()
                .await
                .expect("task semaphore is never closed");
            fut.await
        });
    }

    let mut results = Vec::with_capacity(running.len());
    while let Some(result) = running.next().await {
        results.push(result);
    }
    results
}

async fn run_process<'a, S: ExecutionEventSubscriber>(
    subscriber: &'a S,
    wants_task_output_stream: bool,
    wants_task_input_stream: bool,
    task_ctx: &'a TaskContext<'a>,
    override_command: Option<String>,
    override_retry_command: Option<String>,
    record_logs: bool,
    output_logs: EffectiveOutputLogs,
    max_retries: u8,
    retry_duration: Option<Duration>,
) -> TaskResultContext<'a> {
    let mut tries = 0u8;

    let reg_cmd = override_command.as_deref().or(task_ctx.node.task_exec());
    let retry_cmd = override_retry_command
        .as_deref()
        .or(task_ctx.node.task_retry_exec())
        .or(reg_cmd);

    subscriber
        .on_task_started(TaskStartedEvent {
            task_id: task_ctx.node.full_task_name().to_string(),
            project: task_ctx.node.project_name().to_string(),
            task: task_ctx.node.task_name().to_string(),
        })
        .await;

    let result = loop {
        tries += 1;

        let command = if tries > 1 { retry_cmd } else { reg_cmd };

        let command = if let Some(cmd) = command {
            cmd
        } else {
            return TaskResultContext::new_error(
                task_ctx,
                ChildProcessError::no_command(),
                tries,
            );
        };

        let mut proc =
            match TaskChildProcess::new(task_ctx.node.clone(), command) {
                Ok(o) => o,
                Err(e) => {
                    return TaskResultContext::new_error(
                        task_ctx,
                        e.into(),
                        tries,
                    );
                }
            };

        proc.empty_command_is_success(true);

        if wants_task_output_stream {
            let is_interactive =
                task_ctx.node.persistent() || task_ctx.node.interactive();

            let (writer_end, reader_end) = tokio::io::duplex(64 * 1024);

            if wants_task_input_stream && is_interactive {
                let (stdin_reader, stdin_writer) = tokio::io::duplex(4 * 1024);

                proc.output_writer(writer_end).input_reader(stdin_reader);

                subscriber
                    .on_task_output_stream(TaskOutputStreamEvent {
                        task_id: task_ctx.node.full_task_name().to_string(),
                        project: task_ctx.node.project_name().to_string(),
                        task: task_ctx.node.task_name().to_string(),
                        is_replay: false,
                        is_interactive,
                        output_logs,
                        stream: TaskOutputStream {
                            reader: Box::new(reader_end),
                            writer: Some(Box::new(stdin_writer)),
                        },
                    })
                    .await;
            } else {
                proc.output_writer(writer_end);

                subscriber
                    .on_task_output_stream(TaskOutputStreamEvent {
                        task_id: task_ctx.node.full_task_name().to_string(),
                        project: task_ctx.node.project_name().to_string(),
                        task: task_ctx.node.task_name().to_string(),
                        is_replay: false,
                        is_interactive,
                        output_logs,
                        stream: TaskOutputStream {
                            reader: Box::new(reader_end),
                            writer: None,
                        },
                    })
                    .await;
            }
        }

        proc.record_logs(record_logs)
            .env_vars(&task_ctx.env_vars)
            .keep_stdin_open(
                task_ctx.node.persistent() || task_ctx.node.interactive(),
            );

        let result = proc.exec().await;

        if (result.is_err() || result.as_ref().is_ok_and(|f| !f.success()))
            && tries <= max_retries
        {
            if let Some(duration) = retry_duration
                && !duration.is_zero()
            {
                diagnostic!(
                    subscriber,
                    DiagnosticLevel::Warn,
                    "Waiting for '{:?}' before retrying task '{}'",
                    duration,
                    task_ctx.node.full_task_name(),
                )
                .await;
                tokio::time::sleep(duration).await;
            }

            subscriber
                .on_task_retrying(TaskRetryingEvent {
                    task_id: task_ctx.node.full_task_name().to_string(),
                    project: task_ctx.node.project_name().to_string(),
                    task: task_ctx.node.task_name().to_string(),
                    attempt: tries,
                    max_retries,
                    delay: retry_duration,
                })
                .await;

            continue;
        }

        break result;
    };

    match result {
        Ok(t) => {
            let elapsed = t.elapsed;
            let exit_code = t.exit_code();
            if t.success() {
                subscriber
                    .on_task_completed(TaskCompletedEvent {
                        task_id: task_ctx.node.full_task_name().to_string(),
                        project: task_ctx.node.project_name().to_string(),
                        task: task_ctx.node.task_name().to_string(),
                        exit_code,
                        elapsed,
                        cache_hit: false,
                        tries,
                    })
                    .await;
            } else {
                subscriber
                    .on_task_failed(TaskFailedEvent {
                        task_id: task_ctx.node.full_task_name().to_string(),
                        project: task_ctx.node.project_name().to_string(),
                        task: task_ctx.node.task_name().to_string(),
                        error: format!("exit code '{}'", exit_code),
                        tries,
                    })
                    .await;
            }
            TaskResultContext::new_completed(task_ctx, t, tries)
        }
        Err(e) => {
            subscriber
                .on_task_failed(TaskFailedEvent {
                    task_id: task_ctx.node.full_task_name().to_string(),
                    project: task_ctx.node.project_name().to_string(),
                    task: task_ctx.node.task_name().to_string(),
                    error: e.to_string(),
                    tries,
                })
                .await;
            TaskResultContext::new_error(task_ctx, e, tries)
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct BatchExecutorError(pub(crate) BatchExecutorErrorInner);

impl BatchExecutorError {
    #[allow(unused)]
    pub fn kind(&self) -> BatchExecutorErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<BatchExecutorErrorInner>> From<T> for BatchExecutorError {
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, derive_new::new)]
#[strum_discriminants(name(BatchExecutorErrorKind), vis(pub))]
pub(crate) enum BatchExecutorErrorInner {
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

    #[error("can't get task hash: {task_full_name}")]
    CantGetTaskHash { task_full_name: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Tera(#[from] omni_tera::Error),
}

#[cfg(test)]
mod tests {
    use omni_task_output_logs::{
        EffectiveOutputLogs, LogsDisplay, OutputLogsConfiguration,
        OutputLogsSplit,
    };

    /// The built-in default the executor applies when no flag or config sets a
    /// facet.
    const DEFAULT: LogsDisplay = LogsDisplay::Failed;

    // Mirrors BatchExecutor::resolve_output_logs.
    fn resolve(
        flag_new: Option<LogsDisplay>,
        flag_cached: Option<LogsDisplay>,
        task_cfg: Option<&OutputLogsConfiguration>,
    ) -> EffectiveOutputLogs {
        EffectiveOutputLogs::resolve(flag_new, flag_cached, task_cfg, DEFAULT)
    }

    // Mirrors the replay predicate in replay_cached_results.
    fn should_replay(cached: LogsDisplay, cached_failed: bool) -> bool {
        cached.should_show(cached_failed)
    }

    #[test]
    fn resolution_defaults_to_failed_when_unset() {
        let e = resolve(None, None, None);
        assert_eq!(e.new, LogsDisplay::Failed);
        assert_eq!(e.cached, LogsDisplay::Failed);
    }

    #[test]
    fn resolution_flag_beats_task_config() {
        let cfg = OutputLogsConfiguration::Uniform(LogsDisplay::Never);
        let e = resolve(Some(LogsDisplay::All), None, Some(&cfg));
        assert_eq!(e.new, LogsDisplay::All);
        // cached falls back to flag_new before the config
        assert_eq!(e.cached, LogsDisplay::All);
    }

    #[test]
    fn resolution_task_config_beats_default() {
        let cfg = OutputLogsConfiguration::Split(OutputLogsSplit {
            new: Some(LogsDisplay::All),
            cached: Some(LogsDisplay::Never),
        });
        let e = resolve(None, None, Some(&cfg));
        assert_eq!(e.new, LogsDisplay::All);
        assert_eq!(e.cached, LogsDisplay::Never);
    }

    #[test]
    fn cached_replay_all_always_replays() {
        assert!(should_replay(LogsDisplay::All, false));
        assert!(should_replay(LogsDisplay::All, true));
    }

    #[test]
    fn cached_replay_failed_only_on_failed_exit() {
        assert!(!should_replay(LogsDisplay::Failed, false));
        assert!(should_replay(LogsDisplay::Failed, true));
    }

    #[test]
    fn cached_replay_never_never_replays() {
        assert!(!should_replay(LogsDisplay::Never, false));
        assert!(!should_replay(LogsDisplay::Never, true));
    }

    // ---- Scheduling invariants for `run_bounded` (intra-batch execution) ----
    //
    // These lock in the guarantees Tier A relies on: the concurrency bound is
    // never exceeded, every task runs to completion exactly once, and the
    // collected result set is independent of the order tasks finish in (so the
    // batch executor's name-keyed aggregation stays deterministic). Tests use
    // the default current-thread tokio runtime, making the interleaving
    // deterministic.
    use std::{
        future::Future,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    /// Builds `n` instrumented futures that track live concurrency, assert the
    /// bound from the inside, and yield `yields_for(i)` times before completing
    /// (to control completion order). Each future returns its own index.
    fn instrumented_futs(
        n: usize,
        assert_limit: usize,
        current: Arc<AtomicUsize>,
        max_seen: Arc<AtomicUsize>,
        yields_for: impl Fn(usize) -> usize,
    ) -> Vec<impl Future<Output = usize>> {
        (0..n)
            .map(move |i| {
                let current = current.clone();
                let max_seen = max_seen.clone();
                let yields = yields_for(i);
                async move {
                    let cur = current.fetch_add(1, Ordering::SeqCst) + 1;
                    max_seen.fetch_max(cur, Ordering::SeqCst);
                    assert!(
                        cur <= assert_limit,
                        "live concurrency {cur} exceeded limit {assert_limit}"
                    );
                    for _ in 0..yields {
                        tokio::task::yield_now().await;
                    }
                    current.fetch_sub(1, Ordering::SeqCst);
                    i
                }
            })
            .collect()
    }

    #[tokio::test]
    async fn run_bounded_runs_every_task_exactly_once() {
        let n = 50;
        let current = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let futs = instrumented_futs(n, 8, current, max_seen, |_| 2);

        let mut out = super::run_bounded(futs, 8).await;
        out.sort();

        // Completeness: no dropped or duplicated tasks.
        assert_eq!(out, (0..n).collect::<Vec<_>>());
    }

    #[tokio::test]
    async fn run_bounded_never_exceeds_and_saturates_limit() {
        let n = 20;
        let limit = 4;
        let current = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let futs =
            instrumented_futs(n, limit, current, max_seen.clone(), |_| 3);

        let out = super::run_bounded(futs, limit).await;

        assert_eq!(out.len(), n, "all tasks completed");
        // The in-task assertion already guards the upper bound; also assert the
        // scheduler actually fills every available slot (peak == limit).
        assert_eq!(
            max_seen.load(Ordering::SeqCst),
            limit,
            "should saturate all {limit} slots"
        );
    }

    #[tokio::test]
    async fn run_bounded_limit_one_is_strictly_serial() {
        let n = 8;
        let current = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let futs = instrumented_futs(n, 1, current, max_seen.clone(), |_| 2);

        let out = super::run_bounded(futs, 1).await;

        assert_eq!(out.len(), n);
        assert_eq!(
            max_seen.load(Ordering::SeqCst),
            1,
            "limit of 1 must never run two tasks at once"
        );
    }

    #[tokio::test]
    async fn run_bounded_limit_above_task_count_runs_all_at_once() {
        let n = 5;
        let current = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let futs = instrumented_futs(n, n, current, max_seen.clone(), |_| 2);

        let out = super::run_bounded(futs, 100).await;

        assert_eq!(out.len(), n);
        assert_eq!(max_seen.load(Ordering::SeqCst), n);
    }

    #[tokio::test]
    async fn run_bounded_zero_limit_is_clamped_to_one() {
        let n = 3;
        let current = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        // Effective limit is clamped to 1, so the in-task assertion uses 1.
        let futs = instrumented_futs(n, 1, current, max_seen.clone(), |_| 1);

        // Must make progress (not deadlock) despite a 0 request.
        let out = super::run_bounded(futs, 0).await;

        assert_eq!(out.len(), n);
        assert_eq!(max_seen.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn run_bounded_empty_input_returns_empty() {
        let futs: Vec<std::pin::Pin<Box<dyn Future<Output = usize>>>> =
            Vec::new();
        let out = super::run_bounded(futs, 8).await;
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn run_bounded_result_set_is_independent_of_completion_order() {
        let n = 16;
        let limit = 4;

        // Forward: earlier tasks finish first (fewer yields for low indices).
        let c1 = Arc::new(AtomicUsize::new(0));
        let m1 = Arc::new(AtomicUsize::new(0));
        let forward = instrumented_futs(n, limit, c1, m1, |i| i % 4);
        let mut forward_out = super::run_bounded(forward, limit).await;
        forward_out.sort();

        // Reverse: later tasks finish first (more yields for low indices).
        let c2 = Arc::new(AtomicUsize::new(0));
        let m2 = Arc::new(AtomicUsize::new(0));
        let reverse = instrumented_futs(n, limit, c2, m2, |i| (n - i) % 4);
        let mut reverse_out = super::run_bounded(reverse, limit).await;
        reverse_out.sort();

        // Regardless of completion order, the complete set is returned; the
        // batch executor keys these by task name, so the final result is
        // deterministic.
        assert_eq!(forward_out, (0..n).collect::<Vec<_>>());
        assert_eq!(forward_out, reverse_out);
    }
}
