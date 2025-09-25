use std::{
    hash::{Hash as _, Hasher as _},
    marker::PhantomData,
};

use derive_new::new;
use omni_configurations::MetaConfiguration;
use omni_core::{
    BatchedExecutionPlan, Task, TaskDependency, TaskExecutionGraphError,
    TaskExecutionNode,
};
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{
    Call, Context, DefaultProjectFilter, DefaultTaskFilter,
    ExecutionPlanProvider, FilterError, ProjectFilter as _,
    ProjectFilterExt as _, TaskFilter as _, TaskFilterExt as _,
};

#[derive(Debug, new)]
pub struct DefaultExecutionPlanProvider<'a, TContext: Context + 'a> {
    context: TContext,
    _phantom: PhantomData<&'a ()>,
}

impl<'a, TContext: Context> ExecutionPlanProvider
    for DefaultExecutionPlanProvider<'a, TContext>
{
    type Error = ExecutionPlanProviderError;

    fn get_execution_plan(
        &self,
        call: &Call,
        project_filter: Option<&str>,
        meta_filter: Option<&str>,
        ignore_deps: bool,
    ) -> Result<BatchedExecutionPlan, Self::Error> {
        if ignore_deps {
            self.get_execution_plan_ignored_dependencies(
                call,
                project_filter,
                meta_filter,
            )
        } else {
            self.get_execution_plan_with_dependencies(
                call,
                project_filter,
                meta_filter,
            )
        }
    }
}

impl<'a, TContext: Context> DefaultExecutionPlanProvider<'a, TContext> {
    fn get_execution_plan_ignored_dependencies(
        &self,
        call: &Call,
        project_filter: Option<&str>,
        meta_filter: Option<&str>,
    ) -> Result<BatchedExecutionPlan, ExecutionPlanProviderError> {
        let pf = DefaultProjectFilter::new(project_filter)?;
        // Simple case: just get all matching tasks in one batch
        let projects = pf.filter_projects(self.context.projects());

        if let Some(filter) = project_filter
            && projects.is_empty()
        {
            Err(ExecutionPlanProviderErrorInner::NoProjectFoundForFilter {
                filter: filter.to_string(),
            })?;
        }

        let task_name;

        let all_tasks = match call {
            Call::Command { command, args } => {
                task_name = temp_task_name("exec", command, args);
                let full_cmd = format!("{command} {}", args.join(" "));

                projects
                    .iter()
                    .map(|p| {
                        TaskExecutionNode::new(
                            task_name.clone(),
                            full_cmd.clone(),
                            p.name.clone(),
                            p.dir.clone(),
                            vec![],
                            true,
                            false,
                            false,
                        )
                    })
                    .collect::<Vec<_>>()
            }
            Call::Task(tname) => {
                task_name = tname.clone();
                projects
                    .iter()
                    .filter_map(|p| {
                        p.tasks.get(&task_name).map(|task| {
                            TaskExecutionNode::new(
                                task_name.clone(),
                                task.command.clone(),
                                p.name.clone(),
                                p.dir.clone(),
                                vec![],
                                task.enabled,
                                task.interactive,
                                task.persistent,
                            )
                        })
                    })
                    .collect()
            }
        };

        let tf = self.get_task_filter(
            call.is_command(),
            &task_name,
            project_filter,
            meta_filter,
        )?;

        let filtered = tf.filter_tasks_cloned(&all_tasks);

        Ok(vec![filtered])
    }

    fn get_task_filter(
        &'a self,
        use_project_meta: bool,
        task_name: &str,
        project_filter: Option<&str>,
        meta_filter: Option<&str>,
    ) -> Result<
        DefaultTaskFilter<
            'a,
            impl Fn(&TaskExecutionNode) -> Option<&'a MetaConfiguration>,
        >,
        ExecutionPlanProviderError,
    > {
        let tf = DefaultTaskFilter::new(
            task_name,
            project_filter,
            meta_filter,
            move |n| {
                if use_project_meta {
                    self.context.get_project_meta_config(n.project_name())
                } else {
                    self.context
                        .get_task_meta_config(n.project_name(), n.task_name())
                }
            },
        )?;
        Ok(tf)
    }

    fn get_execution_plan_with_dependencies(
        &self,
        call: &Call,
        project_filter: Option<&str>,
        meta_filter: Option<&str>,
    ) -> Result<BatchedExecutionPlan, ExecutionPlanProviderError> {
        let pf = DefaultProjectFilter::new(project_filter)?;

        if let Some(filter) = project_filter
            && !self
                .context
                .projects()
                .iter()
                .any(|p| pf.should_include_project(p).unwrap_or(false))
        {
            return Err(
                ExecutionPlanProviderErrorInner::NoProjectFoundForFilter {
                    filter: filter.to_string(),
                },
            )?;
        }

        let mut project_graph = self
            .context
            .get_project_graph()
            .map_err(|e| ExecutionPlanProviderErrorInner::Context(e.into()))?;

        let task_name = match call {
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
                            true,
                            false,
                            false,
                            vec![],
                        ),
                    );
                });

                task_name
            }
            Call::Task(task_name) => task_name.clone(),
        };

        let tf = self.get_task_filter(
            call.is_command(),
            &task_name,
            project_filter,
            meta_filter,
        )?;

        let x_graph = project_graph.get_task_execution_graph()?;

        Ok(x_graph
            .get_batched_execution_plan(|n| Ok(tf.should_include_task(n)?))?)
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

#[derive(thiserror::Error, Debug)]
#[error("{inner}")]
pub struct ExecutionPlanProviderError {
    #[source]
    inner: ExecutionPlanProviderErrorInner,
    kind: ExecutionPlanProviderErrorKind,
}

impl ExecutionPlanProviderError {
    #[allow(unused)]
    pub fn kind(&self) -> ExecutionPlanProviderErrorKind {
        self.kind
    }
}

impl<T: Into<ExecutionPlanProviderErrorInner>> From<T>
    for ExecutionPlanProviderError
{
    fn from(value: T) -> Self {
        let inner = value.into();
        let kind = inner.discriminant();
        Self { inner, kind }
    }
}

#[derive(thiserror::Error, Debug, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(ExecutionPlanProviderErrorKind))]
enum ExecutionPlanProviderErrorInner {
    #[error(transparent)]
    Glob(#[from] globset::Error),

    #[error(transparent)]
    Context(eyre::Report),

    #[error(transparent)]
    TaskExecutionGraph(#[from] TaskExecutionGraphError),

    #[error("no project found for filter: {filter}")]
    NoProjectFoundForFilter { filter: String },

    #[error(transparent)]
    FilterError(#[from] FilterError),
}
