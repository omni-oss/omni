use std::{
    borrow::Cow,
    collections::HashMap,
    hash::{Hash, Hasher as _},
};

use clap::ValueEnum;
use derive_builder::Builder;
use derive_new::new;
use futures::{AsyncReadExt, future::join_all, io::AllowStdIo};
use maps::{UnorderedMap, unordered_map};
use omni_cache::{
    NewCacheInfo, TaskExecutionCacheStore as _, TaskExecutionInfo,
    impls::LocalTaskExecutionCacheStoreError,
};
use omni_core::{
    BatchedExecutionPlan, ProjectGraphError, Task, TaskDependency,
    TaskExecutionGraphError, TaskExecutionNode,
};
use omni_hasher::impls::DefaultHash;
use strum::{Display, EnumDiscriminants, EnumIs, IntoDiscriminant as _};
use system_traits::impls::RealSys;

use crate::{
    context::{CacheInfo, Context, ContextSys},
    executor::{ExecutionResult, TaskExecutor},
    utils::env::EnvVarsMap,
};

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, new, Display, EnumIs,
)]
pub enum Call {
    #[strum(to_string = "command '{command} {args:?}'")]
    Command {
        #[new(into)]
        command: String,
        args: Vec<String>,
    },

    #[strum(to_string = "task '{0}'")]
    Task(#[new(into)] String),
}

impl<TSys: ContextSys> TaskOrchestratorBuilder<TSys> {
    pub fn call(&mut self, call: impl Into<Call>) -> &mut Self {
        let call: Call = call.into();

        // default handling for commands is to run them with no dependencies and never consider the cache
        if matches!(call, Call::Command { .. }) {
            if self.ignore_dependencies.is_none() {
                self.ignore_dependencies = Some(true);
            }

            if self.force.is_none() {
                self.force = Some(true);
            }

            if self.no_cache.is_none() {
                self.no_cache = Some(true);
            }

            if self.on_failure.is_none() {
                self.on_failure = Some(OnFailure::Continue);
            }
        }

        self.call = Some(call);

        self
    }
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    new,
    EnumIs,
    ValueEnum,
    Hash,
    Copy,
    Display,
    Default,
)]
#[repr(u8)]
pub enum OnFailure {
    /// Ignore the failure and continue to the next batches
    #[strum(to_string = "continue")]
    Continue,
    /// Continue the execution of the current batch and skip the rest of the tasks in the next batch
    #[strum(to_string = "skip-next-batches")]
    SkipNextBatches,
    /// Skip only the downstream tasks of the failed task
    #[strum(to_string = "skip-dependents")]
    #[default]
    SkipDependents,
}

#[derive(Builder)]
#[builder(setter(into, strip_option))]
pub struct TaskOrchestrator<TSys: ContextSys + 'static = RealSys> {
    context: Context<TSys>,
    #[builder(setter(custom))]
    call: Call,

    /// if true, it will run all tasks ignoring the dependency graph
    ignore_dependencies: bool,

    /// Glob pattern to filter the projects
    #[builder(default)]
    project_filter: Option<String>,

    // Filter the projects/tasks based on the meta configuration
    #[builder(default)]
    meta_filter: Option<String>,

    /// if true, it will not consider the cache and will always execute the task
    force: bool,

    /// if true, it will not cache the execution result, future runs will not see the cached result
    no_cache: bool,

    /// How to handle failures
    on_failure: OnFailure,
}

impl<TSys: ContextSys> TaskOrchestrator<TSys> {
    pub fn builder() -> TaskOrchestratorBuilder<TSys> {
        TaskOrchestratorBuilder::default()
    }
}

#[derive(Debug, new, EnumIs, Display)]
pub enum SkipReason {
    #[strum(to_string = "task in a previous batch failed")]
    PreviousBatchFailure,
    #[strum(to_string = "dependee task failed")]
    DependeeTaskFailure,
}

#[derive(Debug, new, EnumIs)]
pub enum TaskExecutionResult {
    Completed {
        execution: ExecutionResult,
        hash: Option<DefaultHash>,
    },
    CacheHit {
        execution: ExecutionResult,
        hash: Option<DefaultHash>,
    },
    ErrorBeforeComplete {
        task: TaskExecutionNode,
        error: eyre::Report,
    },
    Skipped {
        task: TaskExecutionNode,
        reason: SkipReason,
    },
}

impl TaskExecutionResult {
    pub fn success(&self) -> bool {
        matches!(self,
            TaskExecutionResult::Completed { execution, .. }
            | TaskExecutionResult::CacheHit { execution, .. }
            if execution.success()
        )
    }

    pub fn hash(&self) -> Option<DefaultHash> {
        match self {
            TaskExecutionResult::Completed { hash, .. } => *hash,
            TaskExecutionResult::CacheHit { hash, .. } => *hash,
            TaskExecutionResult::ErrorBeforeComplete { .. } => None,
            TaskExecutionResult::Skipped { .. } => None,
        }
    }

    pub fn skipped_or_error(&self) -> bool {
        self.is_skipped() || self.is_error_before_complete() || !self.success()
    }

    pub fn task(&self) -> &TaskExecutionNode {
        match self {
            TaskExecutionResult::Completed { execution, .. } => &execution.node,
            TaskExecutionResult::ErrorBeforeComplete { task, .. } => task,
            TaskExecutionResult::Skipped { task, .. } => task,
            TaskExecutionResult::CacheHit { execution, .. } => &execution.node,
        }
    }
}

#[derive(Debug)]
struct TaskContext<'a> {
    node: &'a TaskExecutionNode,
    dependencies: Vec<&'a str>,
    dependency_hashes: Vec<DefaultHash>,
    env_vars: Cow<'a, EnvVarsMap>,
    cache_info: Option<&'a CacheInfo>,
}

impl<'a> TaskContext<'a> {
    pub(self) fn execution_info(&'a self) -> Option<TaskExecutionInfo<'a>> {
        let ci = self.cache_info?;
        Some(TaskExecutionInfo {
            dependency_hashes: &self.dependency_hashes,
            env_vars: &self.env_vars,
            input_env_keys: &ci.key_env_keys,
            input_files: &ci.key_input_files,
            output_files: &ci.cache_output_files,
            project_dir: self.node.project_dir(),
            project_name: self.node.project_name(),
            task_command: self.node.task_command(),
            task_name: self.node.task_name(),
        })
    }
}

impl<TSys: ContextSys> TaskOrchestrator<TSys> {
    pub async fn execute(
        &self,
    ) -> Result<Vec<TaskExecutionResult>, TaskOrchestratorError> {
        let mut ctx = self.context.clone();
        if let Call::Task(task) = &self.call
            && task.is_empty()
        {
            return Err(TaskOrchestratorErrorInner::TaskIsEmpty.into());
        }

        ctx.load_projects().await?;

        let filter = self.project_filter.as_deref().unwrap_or("*");

        // serves as a flag as well to signal whether it needs to consider dependencies
        // to execute the task
        let mut task_execution_graph = None;

        let use_project_meta = self.call.is_command();

        let get_meta = |node: &TaskExecutionNode| {
            if use_project_meta {
                ctx.get_project_meta_config(node.project_name())
            } else {
                ctx.get_task_meta_config(node.project_name(), node.task_name())
            }
        };

        let is_meta_matched = if let Some(meta_filter) = &self.meta_filter {
            let meta_filter = omni_expressions::parse(meta_filter)?;

            Some(move |node: &TaskExecutionNode| {
                if let Some(meta) = get_meta(node) {
                    let ctx = meta.clone().into_expression_context()?;
                    meta_filter
                        .coerce_to_bool(&ctx)
                        .inspect_err(|e| {
                            tracing::debug!(
                                "meta filter {} errored, disregarding the error and flagging the task as not matched",
                                e
                            );
                        })
                        .map_err(TaskOrchestratorErrorInner::MetaFilter)
                } else {
                    Ok(true)
                }
            })
        } else {
            None
        };

        // collect the execution plan
        let execution_plan: BatchedExecutionPlan = if self.ignore_dependencies {
            let projects = ctx.get_filtered_projects(filter)?;

            if projects.is_empty() {
                Err(TaskOrchestratorErrorInner::NoProjectFound {
                    filter: filter.to_string(),
                })?;
            }

            let all_tasks = match &self.call {
                Call::Command { command, args } => {
                    let task_name = temp_task_name("exec", command, args);
                    let full_cmd = format!("{command} {}", args.join(" "));

                    projects
                        .iter()
                        .map(|p| {
                            TaskExecutionNode::new(
                                task_name.clone(),
                                full_cmd.clone(),
                                p.name.clone(),
                                p.dir.clone(),
                            )
                        })
                        .collect::<Vec<_>>()
                }
                Call::Task(task_name) => projects
                    .iter()
                    .filter_map(|p| {
                        p.tasks.get(task_name).map(|task| {
                            TaskExecutionNode::new(
                                task_name.clone(),
                                task.command.clone(),
                                p.name.clone(),
                                p.dir.clone(),
                            )
                        })
                    })
                    .collect(),
            };

            let filtered = if self.meta_filter.is_some() {
                let mut filtered = Vec::with_capacity(all_tasks.len());

                for node in all_tasks {
                    // if there is a filter, it must be matched, if error, consider it as not matched
                    if let Some(filter) = &is_meta_matched
                        && !filter(&node).unwrap_or(true)
                    {
                        continue;
                    }

                    filtered.push(node);
                }

                filtered
            } else {
                all_tasks
            };

            vec![filtered]
        } else {
            let mut project_graph = ctx.get_project_graph()?;

            let task_name = match &self.call {
                Call::Command { command, args } => {
                    let task_name = temp_task_name("exec", command, args);
                    let full_cmd = format!("{command} {}", args.join(" "));

                    project_graph.mutate_nodes(|p| {
                        p.tasks.insert(
                            task_name.clone(),
                            Task::new(
                                full_cmd.clone(),
                                vec![TaskDependency::Upstream {
                                    task: task_name.clone(),
                                }],
                                None,
                            ),
                        );
                    });

                    task_name
                }
                Call::Task(task_name) => task_name.clone(),
            };

            let matcher = ctx.get_filter_matcher(filter)?;
            let x_graph = project_graph.get_task_execution_graph()?;

            let plan = x_graph.get_batched_execution_plan(|n| {
                Ok(n.task_name() == task_name
                    && matcher.is_match(n.project_name())
                    && if let Some(filter) = &is_meta_matched {
                        filter(n).unwrap_or(false)
                    } else {
                        true
                    })
            })?;

            // signal to the executor that it needs to consider dependencies
            task_execution_graph = Some(x_graph);
            plan
        };

        let task_count: usize = execution_plan.iter().map(|b| b.len()).sum();

        if task_count == 0 {
            Err(TaskOrchestratorErrorInner::NothingToExecute(
                self.call.clone(),
            ))?;
        }

        let mut overall_results =
            HashMap::<String, TaskExecutionResult>::with_capacity(task_count);

        let cache_store = if !self.force || !self.no_cache {
            Some(ctx.create_local_cache_store())
        } else {
            None
        };

        'main_loop: for batch in &execution_plan {
            // Short circuit if any task failed and the user wants to skip the next batches if any
            // task failed
            let any_failed = overall_results
                .values()
                .any(|r| r.is_error_before_complete());
            if any_failed && self.on_failure == OnFailure::SkipNextBatches {
                for task in batch {
                    overall_results.insert(
                        task.full_task_name().to_string(),
                        TaskExecutionResult::new_skipped(
                            task.clone(),
                            SkipReason::PreviousBatchFailure,
                        ),
                    );
                }
                continue 'main_loop;
            }

            let mut task_ctxs = Vec::with_capacity(batch.len());

            for node in batch {
                let dependencies =
                    if let Some(deps) = task_execution_graph.as_ref() {
                        deps.get_direct_dependencies_ref_by_name(
                            node.project_name(),
                            node.task_name(),
                        )?
                        .into_iter()
                        .map(|(_, node)| node.full_task_name())
                        .collect()
                    } else {
                        vec![]
                    };

                let envs = ctx.get_task_env_vars(node)?;
                let cache_info =
                    ctx.get_cache_info(node.project_name(), node.task_name());

                let dep_hashes = dependencies
                    .iter()
                    .filter_map(|d| {
                        overall_results.get(*d).and_then(|r| r.hash())
                    })
                    .collect::<Vec<_>>();
                let ctx = TaskContext {
                    node,
                    dependencies,
                    env_vars: envs,
                    cache_info,
                    dependency_hashes: dep_hashes,
                };

                trace::debug!(
                    task_context = ?ctx,
                    "added task context to queue: '{}#{}'",
                    node.project_name(),
                    node.task_name()
                );

                task_ctxs.push(ctx);
            }

            let cache_inputs = task_ctxs
                .iter()
                .filter_map(|c| {
                    if c.cache_info.is_some_and(|ci| ci.cache_execution) {
                        c.execution_info()
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();

            let cached_results = if !cache_inputs.is_empty()
                && let Some(cache_store) = cache_store.as_ref()
            {
                cache_store
                    .get_many(&cache_inputs[..])
                    .await?
                    .into_iter()
                    .filter_map(|r| {
                        r.map(|r| {
                            (format!("{}#{}", r.project_name, r.task_name), r)
                        })
                    })
                    .collect::<UnorderedMap<_, _>>()
            } else {
                unordered_map!()
            };

            let mut futs = Vec::with_capacity(task_ctxs.len());
            'inner_loop: for task_ctx in task_ctxs {
                // Short circuit if dependee task failed and the user wants to skip the dependent tasks
                if self.on_failure == OnFailure::SkipDependents
                    && !task_ctx.dependencies.is_empty()
                    && task_ctx.dependencies.iter().any(|d| {
                        overall_results
                            .get(*d)
                            .is_some_and(|r| r.skipped_or_error())
                    })
                {
                    overall_results.insert(
                        task_ctx.node.full_task_name().to_string(),
                        TaskExecutionResult::new_skipped(
                            task_ctx.node.clone(),
                            SkipReason::DependeeTaskFailure,
                        ),
                    );

                    continue 'inner_loop;
                }

                // Replay the cache hits
                if !self.force
                    && let Some(res) =
                        cached_results.get(task_ctx.node.full_task_name())
                {
                    overall_results.insert(
                        task_ctx.node.full_task_name().to_string(),
                        TaskExecutionResult::new_cache_hit(
                            ExecutionResult::new(
                                task_ctx.node.clone(),
                                res.exit_code,
                                res.execution_duration,
                                None,
                            ),
                            Some(res.execution_hash),
                        ),
                    );

                    if let Some(logs_path) = &res.logs_path {
                        let file = AllowStdIo::new(
                            std::fs::OpenOptions::new()
                                .read(true)
                                .open(logs_path)?,
                        );
                        let mut stdout = AllowStdIo::new(std::io::stdout());

                        futures::io::copy(
                            &mut file.take(u64::MAX),
                            &mut stdout,
                        )
                        .await?;
                    }

                    // hard link the cached files to the original file paths if they don't exist
                    let sys = self.context.sys();
                    for file in res.files.iter() {
                        let original_path = file
                            .original_path
                            .path()
                            .expect("should be resolved");

                        if sys.fs_exists_async(original_path).await? {
                            continue;
                        }

                        sys.fs_hard_link_async(
                            file.cached_path.as_path(),
                            original_path,
                        )
                        .await?;
                    }

                    continue;
                }

                let record_logs =
                    task_ctx.cache_info.is_some_and(|ci| ci.cache_logs);

                futs.push(async move {
                    let mut executor = TaskExecutor::new(task_ctx.node.clone());

                    executor
                        .output_writer(AllowStdIo::new(std::io::stdout()))
                        .record_logs(record_logs)
                        .env_vars(&task_ctx.env_vars);

                    let this = executor.exec().await;
                    match this {
                        Ok(t) => Ok((task_ctx, t)),
                        Err(e) => Err((task_ctx, e)),
                    }
                });
            }
            // run all tasks in a batch concurrently
            let task_results = join_all(futs).await;

            // hoist execution info to the cache to not drop it
            let exec_infos;
            let saved_caches = if !self.no_cache
                && !task_results.is_empty()
                && let Some(cache_store) = cache_store.as_ref()
            {
                exec_infos = task_results
                    .iter()
                    .filter_map(|r| {
                        if let Ok((task_ctx, result)) = r
                            && task_ctx
                                .cache_info
                                .is_some_and(|ci| ci.cache_execution)
                            && let Some(exec_info) = task_ctx.execution_info()
                        {
                            Some(NewCacheInfo {
                                execution_duration: result.elapsed,
                                exit_code: result.exit_code(),
                                task: exec_info,
                                logs: result.logs.as_ref(),
                            })
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();

                if !exec_infos.is_empty() {
                    trace::debug!(
                        task_execution_infos = ?exec_infos,
                        "caching task executions"
                    );

                    let results = cache_store.cache_many(&exec_infos).await?;

                    trace::debug!(
                        results = ?results,
                        "cached task execution info successfully"
                    );

                    results
                        .into_iter()
                        .map(|cte| {
                            (
                                format!(
                                    "{}#{}",
                                    cte.project_name, cte.task_name
                                ),
                                cte,
                            )
                        })
                        .collect()
                } else {
                    unordered_map!()
                }
            } else {
                unordered_map!()
            };

            for task_result in &task_results {
                let (key, result) = match task_result {
                    Ok((ctx, result)) => {
                        let fname = result.node.full_task_name().to_string();

                        let exec_result = if !self.no_cache
                            && ctx
                                .cache_info
                                .is_some_and(|ci| ci.cache_execution)
                            && let Some(cte) = saved_caches.get(&fname)
                        {
                            TaskExecutionResult::new_completed(
                                result.clone(),
                                Some(cte.execution_hash),
                            )
                        } else {
                            TaskExecutionResult::new_completed(
                                result.clone(),
                                None,
                            )
                        };

                        (fname, exec_result)
                    }
                    Err((task, error)) => (
                        task.node.full_task_name().to_string(),
                        TaskExecutionResult::new_error_before_complete(
                            task.node.clone(),
                            eyre::eyre!("{error}"),
                        ),
                    ),
                };

                overall_results.insert(key, result);
            }
        }

        Ok(overall_results.into_values().collect())
    }
}

fn temp_task_name(prefix: &str, command: &str, args: &[String]) -> String {
    // utilize default hasher so that the hash is consistent across platforms and versions
    let mut hasher = ahash::AHasher::default();
    let full_cmd = format!("{command} {args:?}");
    full_cmd.hash(&mut hasher);

    let hash = hasher.finish();

    let enc = bs58::encode(hash.to_le_bytes()).into_string();

    format!("{prefix}-{enc}")
}

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct TaskOrchestratorError {
    kind: TaskOrchestratorErrorKind,
    #[source]
    inner: TaskOrchestratorErrorInner,
}

impl TaskOrchestratorError {
    pub fn kind(&self) -> TaskOrchestratorErrorKind {
        self.kind
    }
}

impl<T: Into<TaskOrchestratorErrorInner>> From<T> for TaskOrchestratorError {
    fn from(value: T) -> Self {
        let inner = value.into();
        let kind = inner.discriminant();
        Self { inner, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(TaskOrchestratorErrorKind), vis(pub))]
enum TaskOrchestratorErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    // #[error(transparent)]
    // CantGetEnvVars(eyre::Report),
    #[error("task is empty")]
    TaskIsEmpty,

    // #[error("command is empty")]
    // CommandIsEmpty,

    // #[error("task '{task}' not found")]
    // TaskNotFound { task: String },
    #[error("no project found for criteria: filter = '{filter}'")]
    NoProjectFound { filter: String },

    #[error("no task to execute: {0} not found")]
    NothingToExecute(Call),

    #[error(transparent)]
    TaskExecutionGraph(#[from] TaskExecutionGraphError),

    #[error(transparent)]
    ProjectGraph(#[from] ProjectGraphError),

    #[error(transparent)]
    Unknown(#[from] eyre::Report),

    #[error(transparent)]
    LocalTaskExecutionCacheStore(#[from] LocalTaskExecutionCacheStoreError),

    #[error(transparent)]
    MetaFilter(#[from] omni_expressions::Error),
}
