use std::{
    borrow::Cow,
    collections::HashMap,
    hash::{Hash, Hasher as _},
};

use clap::ValueEnum;
use derive_builder::Builder;
use derive_new::new;
use futures::{future::join_all, io::AllowStdIo};
use omni_core::{
    BatchedExecutionPlan, ProjectGraphError, Task, TaskDependency,
    TaskExecutionGraphError, TaskExecutionNode,
};
use omni_hasher::impls::DefaultHash;
use strum::{Display, EnumDiscriminants, EnumIs, IntoDiscriminant as _};
use system_traits::impls::RealSys;

use crate::{
    context::{CacheInfo, Context, ContextSys},
    executor::{ExecutionResult, TaskExecutor, TaskExecutorError},
    utils::{dir_walker::create_default_dir_walker, env::EnvVarsMap},
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, new, Display)]
pub enum Call {
    #[strum(to_string = "command '{0}'")]
    Command(#[new(into)] String),

    #[strum(to_string = "task '{0}'")]
    Task(#[new(into)] String),
}

impl<TSys: ContextSys> TaskOrchestratorBuilder<TSys> {
    pub fn call(&mut self, call: impl Into<Call>) -> &mut Self {
        let call: Call = call.into();

        // default handling for commands is to run them with no dependencies and never consider the cache
        if matches!(call, Call::Command(_)) {
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
pub struct TaskOrchestrator<TSys: ContextSys = RealSys> {
    context: Context<TSys>,
    #[builder(setter(custom))]
    call: Call,

    /// if true, it will run all tasks ignoring the dependency graph
    ignore_dependencies: bool,

    /// Glob pattern to filter the projects
    #[builder(default)]
    project_filter: Option<String>,

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

#[derive(Debug, new, EnumIs)]
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
        is_cache_used: bool,
        hash: Option<DefaultHash>,
    },
    ErrorBeforeComplete {
        task: TaskExecutionNode,
        error: TaskExecutorError,
    },
    Skipped {
        task: TaskExecutionNode,
        reason: SkipReason,
    },
}

impl TaskExecutionResult {
    pub fn success(&self) -> bool {
        matches!(self, TaskExecutionResult::Completed { execution, .. } if execution.success())
    }

    pub fn skipped_or_error(&self) -> bool {
        self.is_skipped() || self.is_error_before_complete()
    }

    pub fn task(&self) -> &TaskExecutionNode {
        match self {
            TaskExecutionResult::Completed { execution, .. } => &execution.node,
            TaskExecutionResult::ErrorBeforeComplete { task, .. } => task,
            TaskExecutionResult::Skipped { task, .. } => task,
        }
    }
}

impl<TSys: ContextSys> TaskOrchestrator<TSys> {
    pub async fn execute(
        &self,
    ) -> Result<Vec<TaskExecutionResult>, TaskOrchestratorError> {
        struct TaskContext<'a> {
            node: &'a TaskExecutionNode,
            dependencies: Vec<&'a str>,
            env_vars: Cow<'a, EnvVarsMap>,
            cache_info: Option<&'a CacheInfo>,
        }

        let mut ctx = self.context.clone();
        if let Call::Task(task) = &self.call
            && task.is_empty()
        {
            return Err(TaskOrchestratorErrorInner::TaskIsEmpty.into());
        }
        // if command.task.is_empty() {
        //     eyre::bail!("Task cannot be empty");
        // }
        ctx.load_projects(&create_default_dir_walker())?;

        let filter = self.project_filter.as_deref().unwrap_or("*");

        // serves as a flag as well to signal whether it needs to consider dependencies
        // to execute the task
        let mut task_execution_graph = None;

        let execution_plan: BatchedExecutionPlan = if self.ignore_dependencies {
            let projects = ctx.get_filtered_projects(filter)?;

            if projects.is_empty() {
                Err(TaskOrchestratorErrorInner::NoProjectFoundCriteria {
                    filter: filter.to_string(),
                })?;
            }

            let all_tasks = match &self.call {
                Call::Command(c) => {
                    let command_temp_name = format!("exec-{}", hash_command(c));

                    projects
                        .iter()
                        .map(|p| {
                            TaskExecutionNode::new(
                                command_temp_name.clone(),
                                c.clone(),
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

            vec![all_tasks]
        } else {
            let mut project_graph = ctx.get_project_graph()?;

            let task_name = match &self.call {
                Call::Command(command) => {
                    let cmd = hash_command(command);

                    project_graph.mutate_nodes(|p| {
                        p.tasks.insert(
                            cmd.clone(),
                            Task::new(
                                command.clone(),
                                vec![TaskDependency::Upstream {
                                    task: cmd.clone(),
                                }],
                                None,
                            ),
                        );
                    });

                    cmd
                }
                Call::Task(task_name) => task_name.clone(),
            };

            let matcher = ctx.get_filter_matcher(filter)?;
            let x_graph = project_graph.get_task_execution_graph()?;

            let plan = x_graph.get_batched_execution_plan(|n| {
                n.task_name() == task_name && matcher.is_match(n.project_name())
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

        let _cache_store = ctx.create_local_cache_store();

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

                task_ctxs.push(TaskContext {
                    node,
                    dependencies,
                    env_vars: envs,
                    cache_info,
                });
            }

            let mut futs = Vec::with_capacity(task_ctxs.len());
            'inner_loop: for task_ctx in task_ctxs {
                // Short circuit if dependee task failed and the user wants to skip the dependent tasks
                if self.on_failure == OnFailure::SkipDependents
                    && !task_ctx.dependencies.is_empty()
                    && task_ctx.dependencies.iter().any(|d| {
                        overall_results
                            .get(*d)
                            .is_some_and(|r| r.is_error_before_complete())
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

                futs.push(async move {
                    let mut executor = TaskExecutor::new(task_ctx.node.clone());

                    executor
                        .set_output_writer(AllowStdIo::new(std::io::stdout()))
                        .set_env_vars(&task_ctx.env_vars);

                    executor.exec().await.map_err(|e| (task_ctx, e))
                });
            }
            // run all tasks in a batch concurrently
            let task_results = join_all(futs).await;

            overall_results.extend(task_results.into_iter().map(|result| {
                match result {
                    Ok(result) => (
                        result.node.full_task_name().to_string(),
                        TaskExecutionResult::new_completed(result, false, None),
                    ),
                    Err((task, error)) => (
                        task.node.full_task_name().to_string(),
                        TaskExecutionResult::new_error_before_complete(
                            task.node.clone(),
                            error,
                        ),
                    ),
                }
            }));
        }

        Ok(overall_results.into_values().collect())
    }
}

fn hash_command(command: &str) -> String {
    // utilize default hasher so that the hash is consistent across platforms and versions
    let mut hasher = ahash::AHasher::default();
    command.hash(&mut hasher);

    let hash = hasher.finish();

    bs58::encode(hash.to_le_bytes()).into_string()
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
    CantGetEnvVars(eyre::Report),

    #[error("task is empty")]
    TaskIsEmpty,

    #[error("command is empty")]
    CommandIsEmpty,

    #[error("task '{task}' not found")]
    TaskNotFound { task: String },

    #[error("no project found for criteria: filter = '{filter}'")]
    NoProjectFoundCriteria { filter: String },

    #[error("no task to execute: {0} not found")]
    NothingToExecute(Call),

    #[error(transparent)]
    TaskExecutionGraph(#[from] TaskExecutionGraphError),

    #[error(transparent)]
    ProjectGraph(#[from] ProjectGraphError),

    #[error(transparent)]
    Unknown(#[from] eyre::Report),
}
