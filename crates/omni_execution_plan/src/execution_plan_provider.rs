use std::{
    hash::{Hash as _, Hasher as _},
    marker::PhantomData,
};

use derive_new::new;
use omni_config_types::TeraExprBoolean;
use omni_configurations::MetaConfiguration;
use omni_core::{
    BatchedExecutionPlan, Task, TaskDependency, TaskExecutionGraphError,
    TaskExecutionNode,
};
use omni_types::OmniPath;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{
    Call, Context, DefaultProjectFilter, DefaultTaskFilter,
    DefaultTaskScmAffectedFilter, ExecutionPlanProvider, FilterError,
    ProjectFilter as _, ProjectFilterExt as _, ScmAffectedFilter,
    TaskFilter as _,
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
        project_filters: &[&str],
        dir_filters: &[&str],
        meta_filter: Option<&str>,
        affected_scm_filter: Option<&ScmAffectedFilter>,
        ignore_deps: bool,
        with_dependents: bool,
    ) -> Result<BatchedExecutionPlan, Self::Error> {
        if ignore_deps {
            self.get_execution_plan_ignored_dependencies(
                call,
                project_filters,
                dir_filters,
                meta_filter,
                affected_scm_filter,
            )
        } else {
            self.get_execution_plan_with_dependencies(
                call,
                project_filters,
                dir_filters,
                meta_filter,
                affected_scm_filter,
                with_dependents,
            )
        }
    }
}

impl<'a, TContext: Context> DefaultExecutionPlanProvider<'a, TContext> {
    fn get_execution_plan_ignored_dependencies(
        &self,
        call: &Call,
        project_filters: &[&str],
        dir_filters: &[&str],
        meta_filter: Option<&str>,
        affected_scm_filter: Option<&ScmAffectedFilter>,
    ) -> Result<BatchedExecutionPlan, ExecutionPlanProviderError> {
        let pf = DefaultProjectFilter::new(project_filters)?;
        // Simple case: just get all matching tasks in one batch
        let projects = pf.filter_projects(self.context.projects());

        if !project_filters.is_empty() && projects.is_empty() {
            Err(ExecutionPlanProviderErrorInner::NoProjectFoundForFilter {
                filter: project_filters.join(", "),
            })?;
        }

        let filtered = match call {
            Call::Command { command, args } => {
                let tfqn = temp_task_name("exec", command, args);
                let full_cmd = format!("{command} {}", args.join(" "));

                projects
                    .iter()
                    .map(|p| {
                        TaskExecutionNode::new(
                            tfqn.clone(),
                            full_cmd.clone(),
                            p.name.clone(),
                            p.dir.clone(),
                            vec![],
                            TeraExprBoolean::Boolean(true),
                            false,
                            false,
                            None,
                            None,
                        )
                    })
                    .collect::<Vec<_>>()
            }
            Call::Tasks(tnames) => {
                let task_names_str =
                    tnames.iter().map(|t| t.as_str()).collect::<Vec<_>>();

                let task_filter = self.get_task_filter(
                    call.is_command(),
                    &task_names_str,
                    project_filters,
                    &dir_filters,
                    meta_filter,
                )?;

                let tfscm =
                    if let Some(affected_scm_filter) = affected_scm_filter {
                        Some(self.get_task_scm_affected_filter(
                            affected_scm_filter,
                        )?)
                    } else {
                        None
                    };

                let passes_tf =
                    |n: &TaskExecutionNode| -> Result<bool, FilterError> {
                        Ok(task_filter.should_include_task(n)?
                            && if let Some(tfscm) = &tfscm {
                                tfscm.should_include_task(n)?
                            } else {
                                true
                            })
                    };

                let mut nodes = vec![];

                for project in projects {
                    for (task_name, task) in project.tasks.iter() {
                        let node = TaskExecutionNode::new(
                            task_name.clone(),
                            task.command.clone(),
                            project.name.clone(),
                            project.dir.clone(),
                            vec![],
                            task.enabled.clone(),
                            task.interactive,
                            task.persistent,
                            task.max_retries,
                            task.retry_interval,
                        );

                        if passes_tf(&node)? {
                            nodes.push(node);
                        }
                    }
                }

                nodes
            }
        };

        Ok(vec![filtered])
    }

    fn get_task_filter(
        &'a self,
        use_project_meta: bool,
        task_names: &[&str],
        project_filters: &[&str],
        dir_filters: &[&str],
        meta_filter: Option<&str>,
    ) -> Result<
        DefaultTaskFilter<
            'a,
            impl Fn(&TaskExecutionNode) -> Option<&'a MetaConfiguration>,
        >,
        ExecutionPlanProviderError,
    > {
        let root_dir = self.context.root_dir();

        let tf = DefaultTaskFilter::new(
            task_names,
            project_filters,
            dir_filters,
            root_dir,
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

    fn get_task_scm_affected_filter(
        &'a self,
        affected_scm_filter: &'_ ScmAffectedFilter,
    ) -> Result<
        DefaultTaskScmAffectedFilter<
            'a,
            impl Fn(&'_ TaskExecutionNode) -> &'a [OmniPath],
        >,
        ExecutionPlanProviderError,
    > {
        let root_dir = self.context.root_dir();

        Ok(DefaultTaskScmAffectedFilter::new(
            &root_dir,
            affected_scm_filter,
            |n| {
                self.context
                    .get_cache_input_files(n.project_name(), n.task_name())
            },
        )?)
    }

    fn get_execution_plan_with_dependencies(
        &self,
        call: &Call,
        project_filters: &[&str],
        dir_filters: &[&str],
        meta_filter: Option<&str>,
        affected_scm_filter: Option<&ScmAffectedFilter>,
        with_dependents: bool,
    ) -> Result<BatchedExecutionPlan, ExecutionPlanProviderError> {
        let pf = DefaultProjectFilter::new(project_filters)?;

        if !project_filters.is_empty()
            && !self
                .context
                .projects()
                .iter()
                .any(|p| pf.should_include_project(p).unwrap_or(false))
        {
            return Err(
                ExecutionPlanProviderErrorInner::NoProjectFoundForFilter {
                    filter: project_filters.join(", "),
                },
            )?;
        }

        let mut project_graph = self
            .context
            .get_project_graph()
            .map_err(|e| ExecutionPlanProviderErrorInner::Context(e.into()))?;

        let task_names = match call {
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
                            TeraExprBoolean::Boolean(true),
                            false,
                            false,
                            vec![],
                            None,
                            None,
                        ),
                    );
                });

                vec![task_name]
            }
            Call::Tasks(task_names) => task_names.clone(),
        };

        let task_names =
            task_names.iter().map(|t| t.as_str()).collect::<Vec<_>>();

        let tf = self.get_task_filter(
            call.is_command(),
            &task_names,
            project_filters,
            dir_filters,
            meta_filter,
        )?;

        let x_graph = project_graph.get_task_execution_graph()?;

        let tfscm = if let Some(affected_scm_filter) = affected_scm_filter {
            Some(self.get_task_scm_affected_filter(affected_scm_filter)?)
        } else {
            None
        };

        Ok(if with_dependents {
            let tf_dependents = self.get_task_filter(
                call.is_command(),
                &task_names,
                &[],
                &[],
                None,
            )?;

            x_graph.get_batched_execution_plan_with_dependents(
                |n| {
                    Ok(tf.should_include_task(n)?
                        && if let Some(tfscm) = &tfscm {
                            tfscm.should_include_task(n)?
                        } else {
                            true
                        })
                },
                |n| Ok(tf_dependents.should_include_task(n)?),
            )?
        } else {
            x_graph.get_batched_execution_plan(|n| {
                Ok(tf.should_include_task(n)?
                    && if let Some(tfscm) = &tfscm {
                        tfscm.should_include_task(n)?
                    } else {
                        true
                    })
            })?
        })
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
#[error(transparent)]
pub struct ExecutionPlanProviderError(
    pub(crate) ExecutionPlanProviderErrorInner,
);

impl ExecutionPlanProviderError {
    #[allow(unused)]
    pub fn kind(&self) -> ExecutionPlanProviderErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ExecutionPlanProviderErrorInner>> From<T>
    for ExecutionPlanProviderError
{
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

#[derive(thiserror::Error, Debug, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(ExecutionPlanProviderErrorKind))]
pub(crate) enum ExecutionPlanProviderErrorInner {
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
