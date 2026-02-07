use std::{borrow::Cow, time::Duration};

use derive_new::new;
use futures::future::join_all;
use maps::{UnorderedMap, unordered_map};
use omni_cache::{CachedTaskExecution, TaskExecutionCacheStore};
use omni_config_types::TeraExprBoolean;
use omni_context::LoadedContext;
use omni_core::TaskExecutionNode;
use omni_hasher::impls::DefaultHash;
use omni_process::{TaskChildProcess, TaskChildProcessResult};
use omni_task_context::{TaskContext, TaskContextProviderExt as _};
use omni_term_ui::mux_output_presenter::{
    MuxOutputPresenter, MuxOutputPresenterError, MuxOutputPresenterExt,
    MuxOutputPresenterStatic, StreamHandleError,
};
use omni_types::{OmniPath, Root, RootMap, enum_map};
use owo_colors::{OwoColorize as _, Style};
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{
    OnFailure, SkipReason, TaskDetails, TaskExecutionResult, TaskExecutorSys,
    cache_manager::{CacheManager, TaskResultContext},
    task_context_provider::DefaultTaskContextProvider,
};

#[derive(new)]
pub struct BatchExecutor<'s, TCacheStore, TSys>
where
    TCacheStore: TaskExecutionCacheStore,
    TSys: TaskExecutorSys,
{
    context: &'s LoadedContext<TSys>,
    cache_manager: CacheManager<TCacheStore, TSys>,
    sys: TSys,
    presenter: &'s MuxOutputPresenterStatic,
    max_concurrent_tasks: usize,
    ignore_dependencies: bool,
    on_failure: OnFailure,
    dry_run: bool,
    replay_cached_logs: bool,
    max_retries: Option<u8>,
    retry_interval: Option<Duration>,
    no_cache: bool,
    add_task_details: bool,
}

impl<'s, TCacheStore, TSys> BatchExecutor<'s, TCacheStore, TSys>
where
    TCacheStore: TaskExecutionCacheStore,
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
        const EXIT_CODE_ERROR_STYLE: Style = Style::new().red().bold();
        const EXIT_CODE_SUCCESS_STYLE: Style = Style::new().green().bold();

        trace::info!(
            "Cache hit for task '{}' with exit code '{}' {}",
            task_ctx.node.full_task_name(),
            res.exit_code.style(if res.exit_code == 0 {
                EXIT_CODE_SUCCESS_STYLE
            } else {
                EXIT_CODE_ERROR_STYLE
            }),
            (if self.replay_cached_logs {
                "(replaying logs)"
            } else {
                "(skipping logs)"
            })
            .dimmed()
        );

        if self.replay_cached_logs
            && let Some(logs_path) = &res.logs_path
        {
            let file = tokio::fs::OpenOptions::new()
                .read(true)
                .open(logs_path)
                .await?;

            let handle = self
                .presenter
                .add_stream_output(
                    task_ctx.node.full_task_name().to_string(),
                    file,
                )
                .await?;

            handle.await?;
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

                        for file in ci.cache_output_files.iter() {
                            new_files.push(expand_omni_path(
                                file,
                                &task_ctx.template_context,
                            )?);
                        }

                        new_ci.cache_output_files = new_files;
                    }

                    if !ci.key_input_files.is_empty() {
                        let mut new_files =
                            Vec::with_capacity(ci.key_input_files.len());

                        for file in ci.key_input_files.iter() {
                            new_files.push(expand_omni_path(
                                file,
                                &task_ctx.template_context,
                            )?);
                        }

                        new_ci.key_input_files = new_files;
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
                    .get_task_meta_config(&task_name, &project_name)
                    .or(self.context.get_project_meta_config(&project_name))
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
    {
        // skip this batch if any error was encountered in a previous batch
        // when on_failure is set to skip_next_batches
        if self.should_skip_batch(overall_results) {
            for task in batch {
                trace::error!(
                    "Skipping task '{}' due to previous batch failure",
                    task.full_task_name()
                );
            }

            let skipped_results = self.skipped_results_for_batch(batch);
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

                trace::info!(
                    "{}",
                    format!(
                        "Skipping disabled task '{}'",
                        task_ctx.node.full_task_name()
                    )
                    .white()
                    .dimmed()
                );
                continue;
            }

            if let Some(error) =
                self.should_skip_task(task_ctx, overall_results)
            {
                new_results.insert(
                    task_ctx.node.full_task_name().to_string(),
                    TaskExecutionResult::new_skipped(
                        task_ctx.node.clone(),
                        SkipReason::DependeeTaskFailure,
                    ),
                );
                trace::error!(
                    "Skipping task '{}' due to failed dependency '{}'",
                    task_ctx.node.full_task_name(),
                    error
                );
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
                trace::info!(
                    "Executing task '{}'",
                    task_ctx.node.full_task_name()
                );
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
                let override_command = omni_tera::one_off(
                    task_ctx.node.task_command(),
                    "command",
                    &task_ctx.template_context,
                )?;

                futs.push(run_process(
                    self.presenter,
                    task_ctx,
                    if override_command != task_ctx.node.task_command() {
                        Some(override_command)
                    } else {
                        None
                    },
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

        self.presenter.wait().await?;

        let hashes = self
            .cache_manager
            .cache_results(&fut_results)
            .await
            .map_err(BatchExecutorErrorInner::new_cant_cache_results)?;

        for fut_result in &fut_results {
            let fname =
                fut_result.task_context().node.full_task_name().to_string();
            let hash = if self.no_cache {
                DefaultHash::default()
            } else {
                hashes.get(&fname).map(|h| h.digest).ok_or_else(|| {
                    trace::error!(
                        "Failed to get hash for task '{}', this is a bug, if you see this please report it to the maintainers",
                        fname
                    );

                    BatchExecutorErrorInner::new_cant_get_task_hash(
                        fname.clone(),
                    )
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

    pub async fn execute_batch<'a>(
        &mut self,
        batch: &'a [TaskExecutionNode],
        overall_results: &'a UnorderedMap<String, TaskExecutionResult>,
    ) -> Result<UnorderedMap<String, TaskExecutionResult>, BatchExecutorError>
    {
        let ctx_provider =
            DefaultTaskContextProvider::new(self.context, overall_results);

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

fn evaluate_bool_expr(
    expr: &TeraExprBoolean,
    context: &omni_tera::Context,
) -> omni_tera::Result<bool> {
    Ok(match expr {
        TeraExprBoolean::Boolean(b) => *b,
        TeraExprBoolean::Expr(expr) => {
            let normal = expr.trim().to_lowercase();
            match normal.as_str() {
                "true" | "yes" | "y" | "1" => return Ok(true),
                "false" | "no" | "n" | "0" => return Ok(false),
                _ => {}
            }

            let result = omni_tera::one_off(expr, "expr", &context)?;

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
    let text = path.unresolved_path().to_string_lossy();

    let expanded = omni_tera::one_off(&text, "path", context)?;

    Ok(OmniPath::new(expanded))
}

async fn run_process<'a>(
    presenter: &'a MuxOutputPresenterStatic,
    task_ctx: &'a TaskContext<'a>,
    override_command: Option<String>,
    record_logs: bool,
    max_retries: u8,
    retry_duration: Option<Duration>,
) -> TaskResultContext<'a> {
    let mut tries = 0u8;

    let result = loop {
        tries += 1;
        let mut proc = TaskChildProcess::new(
            task_ctx.node.clone(),
            override_command.clone(),
        );

        let handle = if presenter.accepts_input() {
            let (out_writer, in_reader, handle) = presenter
                .add_piped_stream_full(task_ctx.node.full_task_name())
                .await
                .expect("failed to add stream");

            proc.output_writer(out_writer).input_reader(in_reader);
            handle
        } else {
            let (stream, handle) = presenter
                .add_piped_stream_output(task_ctx.node.full_task_name())
                .await
                .expect("failed to add stream");

            proc.output_writer(stream);
            handle
        };

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
                trace::warn!(
                    "Wating for '{:?}' before retrying task '{}'",
                    duration,
                    task_ctx.node.full_task_name(),
                );
                tokio::time::sleep(duration).await;
            }

            trace::warn!(
                "Failed task '{}' due to {}, retrying...",
                task_ctx.node.full_task_name(),
                match &result {
                    Ok(t) => format!("exit code '{}'", t.exit_code()),
                    Err(e) => format!("{}", e),
                }
            );

            continue;
        }

        if let Ok(t) = &result {
            if t.success() {
                trace::info!(
                    "{}",
                    format!(
                        "Executed task '{}'",
                        task_ctx.node.full_task_name()
                    )
                );
            } else {
                trace::error!(
                    "{}",
                    format!(
                        "Executed task '{}' but errored with exit code '{}'",
                        task_ctx.node.full_task_name(),
                        t.exit_code()
                    )
                );
            }
        }

        if let Err(e) = &result {
            trace::error!(
                "Failed to execute task '{}': {}",
                task_ctx.node.full_task_name(),
                e
            );
        }

        handle.wait().await.expect("failed to wait for stream");

        break result;
    };

    match result {
        Ok(t) => TaskResultContext::new_completed(task_ctx, t, tries),
        Err(e) => TaskResultContext::new_error(task_ctx, e, tries),
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

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
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
    MuxOutputPresenter(#[from] MuxOutputPresenterError),

    #[error(transparent)]
    StreamHandle(#[from] StreamHandleError),

    #[error(transparent)]
    Tera(#[from] omni_tera::Error),
}
