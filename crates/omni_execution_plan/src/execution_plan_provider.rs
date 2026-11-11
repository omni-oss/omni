use std::{
    hash::{Hash as _, Hasher as _},
    marker::PhantomData,
};

use derive_new::new;
use omni_command_config::CommandConfig;
use omni_config_types::TeraExprBoolean;
use omni_configurations::MetaConfiguration;
use omni_core::{
    BatchedExecutionPlan, Task, TaskDependency, TaskExecutionGraphError,
    TaskExecutionNode,
};
use omni_glob::GlobPatterns;
use omni_types::OmniPath;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{
    Call, Context, DefaultProjectFilter, DefaultTaskFilter,
    DefaultTaskScmAffectedFilter, ExecutionPlanProvider, FilterError,
    MetaErrorOptions, MetaFilterError, MetaOnError, ProjectFilter as _,
    ProjectFilterExt as _, ScmAffectedFilter, TaskFilter as _, TaskSelector,
};

#[derive(Debug, new)]
pub struct DefaultExecutionPlanProvider<'a, TContext: Context + 'a> {
    context: TContext,
    _phantom: PhantomData<&'a ()>,
    #[new(default)]
    meta_error: MetaErrorOptions,
    /// Tolerated meta filter errors (`--meta-on-error exclude`), for the
    /// caller to report as notices.
    #[new(default)]
    excluded: std::sync::Mutex<Vec<MetaFilterError>>,
}

impl<'a, TContext: Context + 'a> DefaultExecutionPlanProvider<'a, TContext> {
    pub fn with_meta_error_options(
        mut self,
        options: MetaErrorOptions,
    ) -> Self {
        self.meta_error = options;
        self
    }

    /// Takes the meta filter errors that were tolerated while planning.
    pub fn take_excluded_meta_errors(&self) -> Vec<MetaFilterError> {
        std::mem::take(&mut *self.excluded.lock().unwrap())
    }

    fn handle_meta_error(
        &self,
        error: Option<MetaFilterError>,
    ) -> Result<(), ExecutionPlanProviderError> {
        match (error, self.meta_error.on_error) {
            (None, _) => Ok(()),
            (Some(e), MetaOnError::Fail) => {
                Err(ExecutionPlanProviderErrorInner::MetaFilter(e).into())
            }
            (Some(e), MetaOnError::Exclude) => {
                self.excluded.lock().unwrap().push(e);
                Ok(())
            }
        }
    }
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
                let command_config = command_call_config(command, args);

                // Apply the project/dir/meta filters to the synthetic command
                // task too, so `exec` honors `--dir`/`-m` (not just `-p`).
                let command_task_names = [TaskSelector::exact(tfqn.as_str())];
                let task_filter = self.get_task_filter(
                    call.is_command(),
                    &command_task_names,
                    project_filters,
                    dir_filters,
                    meta_filter,
                )?;

                let mut nodes = vec![];

                for p in projects.iter() {
                    let node = TaskExecutionNode::new(
                        tfqn.clone(),
                        Some(command_config.clone()),
                        None,
                        p.name.clone(),
                        p.dir.clone(),
                        vec![],
                        TeraExprBoolean::Boolean(true),
                        false,
                        false,
                        Default::default(),
                    );

                    if task_filter.should_include_task(&node)? {
                        nodes.push(node);
                    }
                }

                self.handle_meta_error(task_filter.take_meta_error())?;

                nodes
            }
            Call::Tasks(tnames) => {
                let task_filter = self.get_task_filter(
                    call.is_command(),
                    tnames,
                    project_filters,
                    dir_filters,
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
                            task.exec.clone(),
                            task.retry_exec.clone(),
                            project.name.clone(),
                            project.dir.clone(),
                            vec![],
                            task.enabled.clone(),
                            task.interactive,
                            task.persistent,
                            task.retry.clone(),
                        );

                        if passes_tf(&node)? {
                            nodes.push(node);
                        }
                    }
                }

                self.handle_meta_error(task_filter.take_meta_error())?;

                nodes
            }
        };

        Ok(vec![filtered])
    }

    fn get_task_filter(
        &'a self,
        use_project_meta: bool,
        task_names: &[TaskSelector],
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
        )?
        .collect_meta_errors(self.meta_error.limits);
        Ok(tf)
    }

    fn get_task_scm_affected_filter(
        &'a self,
        affected_scm_filter: &'_ ScmAffectedFilter,
    ) -> Result<
        DefaultTaskScmAffectedFilter<
            'a,
            impl Fn(&'_ TaskExecutionNode) -> &'a GlobPatterns<OmniPath>,
        >,
        ExecutionPlanProviderError,
    > {
        let root_dir = self.context.root_dir();

        Ok(DefaultTaskScmAffectedFilter::new(
            root_dir,
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
            Err(ExecutionPlanProviderErrorInner::NoProjectFoundForFilter {
                filter: project_filters.join(", "),
            })?;
        }

        let mut project_graph = self
            .context
            .get_project_graph()
            .map_err(|e| ExecutionPlanProviderErrorInner::Context(e.into()))?;

        let task_names = match call {
            Call::Command { command, args } => {
                let task_name = temp_task_name("exec", command, args);
                let command_config = command_call_config(command, args);

                project_graph.mutate_nodes(|p| {
                    p.tasks.insert(
                        task_name.clone(),
                        Task::new(
                            Some(command_config.clone()),
                            None,
                            vec![TaskDependency::Upstream {
                                task: task_name.clone(),
                            }],
                            None,
                            TeraExprBoolean::Boolean(true),
                            false,
                            false,
                            vec![],
                            Default::default(),
                        ),
                    );
                });

                vec![TaskSelector::exact(task_name)]
            }
            Call::Tasks(selectors) => selectors.clone(),
        };

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

        let plan = if with_dependents {
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
        };

        self.handle_meta_error(tf.take_meta_error())?;

        Ok(plan)
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

/// Build the synthetic command for a `Call::Command` as an explicit argv so
/// that arguments containing spaces survive intact (no join + re-split).
fn command_call_config(command: &str, args: &[String]) -> CommandConfig {
    CommandConfig::Argv(
        std::iter::once(command.to_string())
            .chain(args.iter().cloned())
            .collect(),
    )
}

#[derive(thiserror::Error, Debug, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
pub struct ExecutionPlanProviderError(
    pub(crate) ExecutionPlanProviderErrorInner,
);

impl ExecutionPlanProviderError {
    pub fn meta_filter_error(&self) -> Option<&MetaFilterError> {
        match &self.0 {
            ExecutionPlanProviderErrorInner::MetaFilter(e) => Some(e),
            ExecutionPlanProviderErrorInner::FilterError(f) => {
                f.meta_filter_error()
            }
            _ => None,
        }
    }

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

#[derive(
    thiserror::Error, Debug, EnumDiscriminants, omni_diagnostic::Diagnostic,
)]
#[strum_discriminants(vis(pub), name(ExecutionPlanProviderErrorKind))]
pub(crate) enum ExecutionPlanProviderErrorInner {
    #[error(transparent)]
    #[diagnostic(delegate)]
    Glob(#[from] globset::Error),

    #[error(transparent)]
    #[diagnostic(delegate)]
    Context(eyre::Report),

    #[error(transparent)]
    #[diagnostic(delegate)]
    TaskExecutionGraph(#[from] TaskExecutionGraphError),

    /// No project matches the project filter.
    #[error("no project found for filter: {filter}")]
    #[diagnostic(
        code = "plan.no_project_found_for_filter",
        hint = "check the project filter `{filter}` against the `name` in each project configuration"
    )]
    NoProjectFoundForFilter {
        #[diagnostic(detail)]
        filter: String,
    },

    #[error(transparent)]
    #[diagnostic(delegate)]
    FilterError(#[from] FilterError),

    #[error(transparent)]
    #[diagnostic(delegate)]
    MetaFilter(MetaFilterError),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_meta_error(total: usize) -> MetaFilterError {
        MetaFilterError {
            filter: "knd == 1".into(),
            total,
            groups: vec![crate::MetaFilterGroup {
                cause: "Undeclared reference to 'knd'".into(),
                count: total,
                samples: vec![],
                omitted: 0,
                hint: Some("Did you mean `kind`?".into()),
            }],
            other: 2,
        }
    }

    #[test]
    fn meta_filter_error_is_found_through_transparent_wrappers() {
        use eyre::WrapErr;
        use omni_diagnostic::DiagnosticReport;

        let wrapped = ExecutionPlanProviderError(
            ExecutionPlanProviderErrorInner::FilterError(FilterError(
                crate::filter::FilterErrorInner::MetaFilter(sample_meta_error(
                    500,
                )),
            )),
        );
        let report: eyre::Report = Err::<(), _>(wrapped)
            .wrap_err("planning tasks for `test`")
            .unwrap_err();

        let d = DiagnosticReport::from_report(&report)
            .expect("meta filter diagnostic");
        assert_eq!(d.code, "meta.filter_failed");
        assert_eq!(d.context, vec!["planning tasks for `test`"]);
        assert_eq!(d.message, "meta filter failed to evaluate for 500 task(s)");
        assert_eq!(d.details.as_ref().unwrap()["other"], 2);
        assert_eq!(d.details.as_ref().unwrap()["total"], 500);
        assert_eq!(d.exit_code(), 2);
        let text = d.to_text();
        assert!(text.contains("[500 tasks]"), "{text}");
        assert!(text.contains("hint: Did you mean `kind`?"), "{text}");
        assert_eq!(
            d.hints,
            vec![
                "Did you mean `kind`?".to_string(),
                "To skip failing tasks instead, pass --meta-on-error exclude."
                    .to_string()
            ]
        );
        assert_eq!(d.to_json()["hints"][0], "Did you mean `kind`?");
        assert!(
            text.contains("hint: To skip failing tasks instead"),
            "{text}"
        );
    }

    #[test]
    fn command_call_preserves_spaced_argument() {
        // `omni exec echo "hello world"` must run ["echo", "hello world"],
        // not split the quoted argument back apart.
        let cfg = command_call_config("echo", &["hello world".to_string()]);

        assert_eq!(
            cfg,
            CommandConfig::Argv(vec![
                "echo".to_string(),
                "hello world".to_string(),
            ])
        );
    }

    struct FixtureContext {
        projects: Vec<omni_core::Project>,
        patterns: GlobPatterns<OmniPath>,
    }

    #[derive(Debug, thiserror::Error)]
    #[error("fixture")]
    struct FixtureError;

    impl Context for FixtureContext {
        type Error = FixtureError;

        fn get_project_meta_config(
            &self,
            _: &str,
        ) -> Option<&MetaConfiguration> {
            None
        }

        fn get_task_meta_config(
            &self,
            _: &str,
            _: &str,
        ) -> Option<&MetaConfiguration> {
            None
        }

        fn get_cache_input_files(
            &self,
            _: &str,
            _: &str,
        ) -> &GlobPatterns<OmniPath> {
            &self.patterns
        }

        fn get_project_graph(
            &self,
        ) -> Result<omni_core::ProjectGraph, Self::Error> {
            omni_core::ProjectGraph::from_projects(self.projects.clone())
                .map_err(|_| FixtureError)
        }

        fn projects(&self) -> &[omni_core::Project] {
            &self.projects
        }

        fn root_dir(&self) -> &std::path::Path {
            std::path::Path::new("/ws")
        }
    }

    fn planned(call: &Call) -> Vec<String> {
        let task = || {
            Task::new(
                Some(CommandConfig::Shell("echo".into())),
                None,
                vec![],
                None,
                TeraExprBoolean::Boolean(true),
                false,
                false,
                vec![],
                None,
                None,
            )
        };
        let mut project = omni_core::Project::new(
            "app",
            "/ws/app".into(),
            vec![],
            Default::default(),
        );
        for name in ["*", "build", "test"] {
            project.tasks.insert(name.to_string(), task());
        }
        let ctx = FixtureContext {
            projects: vec![project],
            patterns: GlobPatterns::default(),
        };
        let plan = DefaultExecutionPlanProvider::new(ctx)
            .get_execution_plan(call, &[], &[], None, None, true, false)
            .unwrap();
        let mut names: Vec<String> = plan
            .into_iter()
            .flatten()
            .map(|n| n.task_name().to_string())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn exact_star_selects_only_the_task_named_star() {
        assert_eq!(planned(&Call::exact_tasks(["*"])), vec!["*"]);
    }

    #[test]
    fn pattern_star_selects_all_tasks() {
        assert_eq!(
            planned(&Call::pattern_tasks(["*"])),
            vec!["*", "build", "test"]
        );
    }

    #[test]
    fn all_tasks_selects_all_tasks() {
        assert_eq!(planned(&Call::all_tasks()), vec!["*", "build", "test"]);
    }

    #[test]
    fn mixed_exact_and_pattern_selectors_union() {
        let call = Call::Tasks(vec![
            TaskSelector::exact("*"),
            TaskSelector::pattern("te*"),
        ]);
        assert_eq!(planned(&call), vec!["*", "test"]);
    }
}
