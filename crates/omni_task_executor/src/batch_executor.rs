use std::{borrow::Cow, time::Duration};

use futures::future::join_all;
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
    replay_cached_logs: bool,
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
        replay_cached_logs: bool,
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
            replay_cached_logs,
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

    async fn replay_cached_results(
        &self,
        task_ctx: &TaskContext<'_>,
        res: &CachedTaskExecution,
    ) -> Result<(), BatchExecutorError> {
        // Emit cache hit event
        self.subscriber
            .on_cache_hit(CacheHitEvent {
                task_id: task_ctx.node.full_task_name().to_string(),
                project: task_ctx.node.project_name().to_string(),
                task: task_ctx.node.task_name().to_string(),
                digest: res.digest.to_vec(),
                replay_logs: self.replay_cached_logs
                    && self.wants_task_output_stream,
                has_logs: res.logs_path.is_some(),
            })
            .await;

        if self.replay_cached_logs
            && self.wants_task_output_stream
            && let Some(logs_path) = &res.logs_path
        {
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
                    self.max_retries
                        .unwrap_or(task_ctx.node.max_retries().unwrap_or(0)),
                    self.retry_interval.or(task_ctx.node.retry_interval()),
                ));
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

async fn run_process<'a, S: ExecutionEventSubscriber>(
    subscriber: &'a S,
    wants_task_output_stream: bool,
    wants_task_input_stream: bool,
    task_ctx: &'a TaskContext<'a>,
    override_command: Option<String>,
    override_retry_command: Option<String>,
    record_logs: bool,
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
