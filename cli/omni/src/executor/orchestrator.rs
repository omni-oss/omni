use std::{
    borrow::Cow,
    hash::{Hash, Hasher as _},
};

use derive_builder::Builder;
use derive_new::new;
use futures::{future::join_all, io::AllowStdIo};
use omni_core::{
    BatchedExecutionPlan, ProjectGraphError, Task, TaskDependency,
    TaskExecutionGraphError, TaskExecutionNode,
};
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant as _};
use system_traits::impls::RealSys;

use crate::{
    context::{Context, ContextSys},
    executor::{ExecutionResult, TaskExecutor, TaskExecutorError},
    utils::dir_walker::create_default_dir_walker,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, new)]
pub enum Call {
    Command(#[new(into)] String),
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
        }

        self.call = Some(call);

        self
    }
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

    /// if true, it will continue to execute tasks even if one fails
    ignore_failures: bool,
}

impl<TSys: ContextSys> TaskOrchestrator<TSys> {
    pub fn builder() -> TaskOrchestratorBuilder<TSys> {
        TaskOrchestratorBuilder::default()
    }
}

#[derive(Debug, new, EnumIs)]
pub enum TaskExecutionResult {
    Completed {
        execution: ExecutionResult,
    },
    Errored {
        task: TaskExecutionNode,
        error: TaskExecutorError,
    },
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
        // if command.task.is_empty() {
        //     eyre::bail!("Task cannot be empty");
        // }
        ctx.load_projects(&create_default_dir_walker())?;

        let filter = self.project_filter.as_deref().unwrap_or("*");

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

            project_graph
                .get_task_execution_graph()?
                .get_batched_execution_plan(|n| {
                    n.task_name() == task_name
                        && matcher.is_match(n.project_name())
                })?
        };

        let task_count: usize = execution_plan.iter().map(|b| b.len()).sum();

        if task_count == 0 {
            Err(TaskOrchestratorErrorInner::NoTaskToExecute)?;
        }

        let mut overall_results = Vec::with_capacity(task_count);

        for batch in execution_plan {
            let mut tasks = vec![];

            for task in batch {
                let mut envs = Cow::Borrowed(
                    ctx.get_cached_env_vars(task.project_dir())
                        .map_err(TaskOrchestratorErrorInner::CantGetEnvVars)?,
                );

                if let Some(overrides) = ctx.get_task_env_vars(&task) {
                    envs.to_mut().extend(overrides.clone());
                }

                tasks.push(async move {
                    let mut executor = TaskExecutor::new(task.clone());

                    executor
                        .set_output_writer(AllowStdIo::new(std::io::stdout()))
                        .set_env_vars(&envs);

                    executor.exec().await.map_err(|e| (task, e))
                });
            }
            // run all tasks in a batch concurrently
            let task_results = join_all(tasks).await;

            overall_results.extend(task_results);
        }

        Ok(overall_results
            .into_iter()
            .map(|result| match result {
                Ok(e) => TaskExecutionResult::new_completed(e),
                Err((task, error)) => {
                    TaskExecutionResult::new_errored(task, error)
                }
            })
            .collect())
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

    #[error("no task to execute")]
    NoTaskToExecute,

    #[error(transparent)]
    TaskExecutionGraph(#[from] TaskExecutionGraphError),

    #[error(transparent)]
    ProjectGraph(#[from] ProjectGraphError),

    #[error("{0}")]
    Unknown(#[from] eyre::Report),
}
