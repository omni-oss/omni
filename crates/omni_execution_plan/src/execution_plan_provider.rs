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
    TaskExecutionNode, TaskKey,
};
use omni_glob::GlobPatterns;
use omni_types::OmniPath;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{
    Call, Context, DefaultProjectFilter, DefaultTaskFilter,
    DefaultTaskScmAffectedFilter, ExecutionPlanProvider, FilterError,
    MetaErrorOptions, MetaFilterError, MetaOnError, ProjectFilter as _,
    ProjectFilterExt as _, ProjectSelector, ScmAffectedFilter, TaskFilter as _,
    TaskSelector,
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
    /// Skip the check that every exact task selector names a task in scope.
    /// Off by default. The cache store turns it on: cache entries outlive the
    /// tasks that created them, so an exact name for a removed task must still
    /// be usable to find (and prune) its entries.
    #[new(default)]
    allow_missing_exact: bool,
}

impl<'a, TContext: Context + 'a> DefaultExecutionPlanProvider<'a, TContext> {
    /// See the `allow_missing_exact` field.
    pub fn with_allow_missing_exact(mut self, allow: bool) -> Self {
        self.allow_missing_exact = allow;
        self
    }

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
        project_filters: &[ProjectSelector],
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

fn try_task_key(
    project: &str,
    task: &str,
) -> Result<TaskKey, ExecutionPlanProviderError> {
    TaskKey::try_new(project, task).map_err(|e| {
        TaskExecutionGraphError::invalid_name(&e.project, &e.task).into()
    })
}

impl<'a, TContext: Context> DefaultExecutionPlanProvider<'a, TContext> {
    fn get_execution_plan_ignored_dependencies(
        &self,
        call: &Call,
        project_filters: &[ProjectSelector],
        dir_filters: &[&str],
        meta_filter: Option<&str>,
        affected_scm_filter: Option<&ScmAffectedFilter>,
    ) -> Result<BatchedExecutionPlan, ExecutionPlanProviderError> {
        let pf = DefaultProjectFilter::new(project_filters)?;
        // Simple case: just get all matching tasks in one batch
        let projects = pf.filter_projects(self.context.projects());

        self.check_project_selectors(project_filters)?;

        self.check_exact_selectors(call, project_filters, dir_filters)?;

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
                        try_task_key(p.name.as_str(), tfqn.as_str())?,
                        Some(command_config.clone()),
                        None,
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
                            try_task_key(
                                project.name.as_str(),
                                task_name.as_str(),
                            )?,
                            task.exec.clone(),
                            task.retry_exec.clone(),
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

    /// Every exact project selector must name a project, and the selectors
    /// together must match at least one. With `allow_missing_exact`, exact names
    /// that match nothing are tolerated (cache entries outlive projects), but a
    /// glob that matches nothing is still an error.
    fn check_project_selectors(
        &self,
        project_filters: &[ProjectSelector],
    ) -> Result<(), ExecutionPlanProviderError> {
        if project_filters.is_empty() {
            return Ok(());
        }
        let projects = self.context.projects();
        if !self.allow_missing_exact {
            for selector in project_filters {
                if let ProjectSelector::Exact(name) = selector
                    && !projects.iter().any(|p| p.name == *name)
                {
                    return Err(
                        ExecutionPlanProviderErrorInner::NoProjectFoundForFilter {
                            filter: name.clone(),
                            hint: exact_project_hint(name),
                        }
                        .into(),
                    );
                }
            }
        }
        // Every glob must match a project on its own, so a typo in a pattern is
        // not hidden by an exact name that exists beside it. Exact names are
        // checked above (and skipped for the cache store).
        for selector in project_filters {
            if matches!(selector, ProjectSelector::Exact(_)) {
                continue;
            }
            let pf = DefaultProjectFilter::new(std::slice::from_ref(selector))?;
            if !projects
                .iter()
                .any(|p| pf.should_include_project(p).unwrap_or(false))
            {
                return Err(
                    ExecutionPlanProviderErrorInner::NoProjectFoundForFilter {
                        filter: selector.to_string(),
                        hint: "check the project filter against the `name` in each project configuration".to_string(),
                    }
                    .into(),
                );
            }
        }
        Ok(())
    }

    /// Every exact selector must name a task in the project and directory scope.
    /// Checked before the meta and affected narrowing, so a task those filters
    /// later exclude is not reported as missing.
    fn check_exact_selectors(
        &self,
        call: &Call,
        project_filters: &[ProjectSelector],
        dir_filters: &[&str],
    ) -> Result<(), ExecutionPlanProviderError> {
        if self.allow_missing_exact {
            return Ok(());
        }
        let Call::Tasks(selectors) = call else {
            return Ok(());
        };
        let exact: Vec<&String> = selectors
            .iter()
            .filter_map(|s| match s {
                TaskSelector::Exact(n) => Some(n),
                _ => None,
            })
            .collect();
        if exact.is_empty() {
            return Ok(());
        }

        // One filter for the project and directory scope, applied once per
        // project (the scope does not depend on the task). This is a pure
        // probe: no executable node is built. Each exact name is then a map lookup in the in-scope projects.
        let scope = DefaultTaskFilter::new(
            &[],
            project_filters,
            dir_filters,
            self.context.root_dir(),
            None,
            |_| None,
        )?;
        let mut in_scope: Vec<&omni_core::Project> = vec![];
        for project in self.context.projects() {
            if project.tasks.is_empty() {
                continue;
            }
            if scope.includes_scope(&project.name, &project.dir) {
                in_scope.push(project);
            }
        }
        for name in exact {
            if !in_scope.iter().any(|p| p.tasks.contains_key(name.as_str())) {
                return Err(
                    ExecutionPlanProviderErrorInner::ExactTaskNotFound {
                        task: name.clone(),
                        hint: exact_task_hint(name),
                    }
                    .into(),
                );
            }
        }
        Ok(())
    }

    fn get_task_filter(
        &'a self,
        use_project_meta: bool,
        task_names: &[TaskSelector],
        project_filters: &[ProjectSelector],
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
        project_filters: &[ProjectSelector],
        dir_filters: &[&str],
        meta_filter: Option<&str>,
        affected_scm_filter: Option<&ScmAffectedFilter>,
        with_dependents: bool,
    ) -> Result<BatchedExecutionPlan, ExecutionPlanProviderError> {
        self.check_project_selectors(project_filters)?;

        self.check_exact_selectors(call, project_filters, dir_filters)?;

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

/// Positional task names used to be globs. If the name looks like one, point at
/// the flag that still selects by pattern.
fn exact_task_hint(name: &str) -> String {
    if name.contains(['*', '?', '[', ']', '{', '}', '!']) {
        format!(
            "positional task names are exact; to select by glob use `--task-pattern '{name}'`"
        )
    } else {
        "check the name against the tasks defined in the project configurations"
            .to_string()
    }
}

/// Project names used to be globs. If the name looks like one, point at the
/// flag that still selects by pattern.
fn exact_project_hint(name: &str) -> String {
    if name.contains(['*', '?', '[', ']', '{', '}', '!']) {
        format!(
            "project names are exact; to select by glob use `--project-pattern '{name}'`"
        )
    } else {
        "check the name against the `name` in each project configuration"
            .to_string()
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
    #[diagnostic(code = "plan.no_project_found_for_filter", hint = "{hint}")]
    NoProjectFoundForFilter {
        #[diagnostic(detail)]
        filter: String,
        hint: String,
    },

    /// An exact task selector names no task in the selected projects.
    #[error("no task named `{task}` in the selected projects and directories")]
    #[diagnostic(code = "plan.exact_task_not_found", hint = "{hint}")]
    ExactTaskNotFound {
        #[diagnostic(detail)]
        task: String,
        hint: String,
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
    fn try_task_key_rejects_empty_names_with_invalid_name() {
        let err = try_task_key("app", "").unwrap_err();
        assert!(matches!(
            err.0,
            ExecutionPlanProviderErrorInner::TaskExecutionGraph(ref e)
                if e.kind() == omni_core::TaskExecutionGraphErrorKind::InvalidName
        ));
        assert!(try_task_key("app", "build").is_ok());
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

    fn plan_with(
        call: &Call,
        project_filters: &[ProjectSelector],
        meta_filter: Option<&str>,
        ignore_deps: bool,
    ) -> Result<Vec<String>, ExecutionPlanProviderError> {
        plan_with_options(
            call,
            project_filters,
            meta_filter,
            ignore_deps,
            false,
        )
    }

    fn plan_with_options(
        call: &Call,
        project_filters: &[ProjectSelector],
        meta_filter: Option<&str>,
        ignore_deps: bool,
        allow_missing_exact: bool,
    ) -> Result<Vec<String>, ExecutionPlanProviderError> {
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
                Default::default(),
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
            .with_allow_missing_exact(allow_missing_exact)
            .get_execution_plan(
                call,
                project_filters,
                &[],
                meta_filter,
                None,
                ignore_deps,
                false,
            )?;
        let mut names: Vec<String> = plan
            .into_iter()
            .flatten()
            .map(|n| n.task_name().to_string())
            .collect();
        names.sort();
        Ok(names)
    }

    fn planned(call: &Call) -> Vec<String> {
        plan_with(call, &[], None, true).unwrap()
    }

    fn missing_task(error: ExecutionPlanProviderError) -> (String, String) {
        match error.0 {
            ExecutionPlanProviderErrorInner::ExactTaskNotFound {
                task,
                hint,
            } => (task, hint),
            other => panic!("expected ExactTaskNotFound, got {other:?}"),
        }
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

    #[test]
    fn an_exact_task_that_exists_nowhere_is_an_error_on_both_plan_paths() {
        for ignore_deps in [true, false] {
            let error = plan_with(
                &Call::exact_tasks(["missing"]),
                &[],
                None,
                ignore_deps,
            )
            .unwrap_err();
            let (task, hint) = missing_task(error);
            assert_eq!(task, "missing");
            assert!(!hint.contains("--task-pattern"), "{hint}");
        }
    }

    #[test]
    fn a_glob_like_exact_name_that_matches_nothing_points_at_task_pattern() {
        let error = plan_with(&Call::exact_tasks(["bui*"]), &[], None, true)
            .unwrap_err();
        let (task, hint) = missing_task(error);
        assert_eq!(task, "bui*");
        assert!(hint.contains("--task-pattern 'bui*'"), "{hint}");
    }

    #[test]
    fn every_exact_selector_must_exist_even_beside_ones_that_do() {
        let call = Call::Tasks(vec![
            TaskSelector::exact("build"),
            TaskSelector::exact("nope"),
        ]);
        let error = plan_with(&call, &[], None, true).unwrap_err();
        assert_eq!(missing_task(error).0, "nope");
    }

    #[test]
    fn a_task_excluded_by_a_later_filter_is_not_missing() {
        // The meta filter removes every task; the exact task still exists.
        let planned =
            plan_with(&Call::exact_tasks(["build"]), &[], Some("false"), true)
                .unwrap();
        assert!(planned.is_empty());
    }

    #[test]
    fn an_exact_task_must_exist_within_the_project_scope() {
        assert_eq!(
            plan_with(
                &Call::exact_tasks(["build"]),
                &[ProjectSelector::exact("app")],
                None,
                true
            )
            .unwrap(),
            vec!["build"]
        );
    }

    #[test]
    fn a_pattern_that_matches_nothing_is_not_a_missing_task_error() {
        assert!(
            plan_with(&Call::pattern_tasks(["zzz*"]), &[], None, true)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn duplicate_exact_and_pattern_selectors_are_planned_once() {
        let call = Call::Tasks(vec![
            TaskSelector::exact("build"),
            TaskSelector::exact("build"),
            TaskSelector::pattern("bu*"),
        ]);
        assert_eq!(planned(&call), vec!["build"]);
    }

    #[test]
    fn allow_missing_exact_turns_the_check_off_and_plans_nothing_for_it() {
        for ignore_deps in [true, false] {
            let call = Call::Tasks(vec![
                TaskSelector::exact("removed"),
                TaskSelector::exact("build"),
            ]);
            assert!(
                plan_with_options(&call, &[], None, ignore_deps, false)
                    .is_err()
            );
            // The existing task is still planned; the missing one just is not.
            assert_eq!(
                plan_with_options(&call, &[], None, ignore_deps, true).unwrap(),
                vec!["build"]
            );
        }
    }

    fn plan_projects(
        selectors: &[ProjectSelector],
        ignore_deps: bool,
        allow_missing_exact: bool,
    ) -> Result<Vec<String>, ExecutionPlanProviderError> {
        plan_projects_call(
            &Call::exact_tasks(["build"]),
            selectors,
            ignore_deps,
            allow_missing_exact,
        )
    }

    /// Four projects that each have `build`; `lib` also has `libonly`.
    fn plan_projects_call(
        call: &Call,
        selectors: &[ProjectSelector],
        ignore_deps: bool,
        allow_missing_exact: bool,
    ) -> Result<Vec<String>, ExecutionPlanProviderError> {
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
                Default::default(),
            )
        };
        let projects = ["app", "app2", "app*", "lib"]
            .into_iter()
            .map(|name| {
                let mut p = omni_core::Project::new(
                    name,
                    format!("/ws/{name}").into(),
                    vec![],
                    Default::default(),
                );
                p.tasks.insert("build".to_string(), task());
                if name == "lib" {
                    p.tasks.insert("libonly".to_string(), task());
                }
                p
            })
            .collect();
        let ctx = FixtureContext {
            projects,
            patterns: GlobPatterns::default(),
        };
        let plan = DefaultExecutionPlanProvider::new(ctx)
            .with_allow_missing_exact(allow_missing_exact)
            .get_execution_plan(
                call,
                selectors,
                &[],
                None,
                None,
                ignore_deps,
                false,
            )?;
        let mut names: Vec<String> = plan
            .into_iter()
            .flatten()
            .map(|n| omni_types::task_label(n.project_name(), n.task_name()))
            .collect();
        names.sort();
        Ok(names)
    }

    fn missing_project(error: ExecutionPlanProviderError) -> (String, String) {
        match error.0 {
            ExecutionPlanProviderErrorInner::NoProjectFoundForFilter {
                filter,
                hint,
            } => (filter, hint),
            other => panic!("expected NoProjectFoundForFilter, got {other:?}"),
        }
    }

    #[test]
    fn an_exact_project_name_selects_only_that_project() {
        for ignore_deps in [true, false] {
            assert_eq!(
                plan_projects(
                    &[ProjectSelector::exact("app")],
                    ignore_deps,
                    false
                )
                .unwrap(),
                ["app#build"]
            );
        }
    }

    #[test]
    fn a_glob_like_exact_project_name_never_matches_its_neighbours() {
        assert_eq!(
            plan_projects(&[ProjectSelector::exact("app*")], true, false)
                .unwrap(),
            ["app*#build"]
        );
        assert_eq!(
            plan_projects(&[ProjectSelector::pattern("app*")], true, false)
                .unwrap(),
            ["app#build", "app*#build", "app2#build"]
        );
    }

    #[test]
    fn exact_and_pattern_project_selectors_union() {
        assert_eq!(
            plan_projects(
                &[
                    ProjectSelector::exact("lib"),
                    ProjectSelector::pattern("app[0-9]"),
                ],
                true,
                false
            )
            .unwrap(),
            ["app2#build", "lib#build"]
        );
    }

    #[test]
    fn an_exact_project_that_does_not_exist_is_an_error_even_beside_ones_that_do()
     {
        for ignore_deps in [true, false] {
            let error = plan_projects(
                &[
                    ProjectSelector::exact("app"),
                    ProjectSelector::exact("nope"),
                ],
                ignore_deps,
                false,
            )
            .unwrap_err();
            let (filter, hint) = missing_project(error);
            assert_eq!(filter, "nope");
            assert!(!hint.contains("--project-pattern"), "{hint}");
        }
    }

    #[test]
    fn a_glob_like_exact_project_that_matches_nothing_points_at_project_pattern()
     {
        let error =
            plan_projects(&[ProjectSelector::exact("ap*x")], true, false)
                .unwrap_err();
        let (filter, hint) = missing_project(error);
        assert_eq!(filter, "ap*x");
        assert!(hint.contains("--project-pattern 'ap*x'"), "{hint}");
    }

    #[test]
    fn a_pattern_that_matches_no_project_is_still_an_error() {
        for allow in [false, true] {
            let error =
                plan_projects(&[ProjectSelector::pattern("zzz*")], true, allow)
                    .unwrap_err();
            missing_project(error);
        }
    }

    #[test]
    fn allow_missing_exact_tolerates_an_unknown_exact_project() {
        assert!(
            plan_projects(&[ProjectSelector::exact("gone")], true, true)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            plan_projects(
                &[
                    ProjectSelector::exact("gone"),
                    ProjectSelector::exact("lib")
                ],
                false,
                true
            )
            .unwrap(),
            ["lib#build"]
        );
    }

    #[test]
    fn every_project_glob_must_match_even_beside_an_exact_name_that_exists() {
        for allow in [false, true] {
            for ignore_deps in [true, false] {
                let error = plan_projects(
                    &[
                        ProjectSelector::exact("app"),
                        ProjectSelector::pattern("zzz*"),
                    ],
                    ignore_deps,
                    allow,
                )
                .unwrap_err();
                assert_eq!(missing_project(error).0, "zzz* (pattern)");
            }
        }
    }

    #[test]
    fn an_exact_task_must_exist_in_the_selected_projects_not_just_anywhere() {
        let call = Call::exact_tasks(["libonly"]);
        for ignore_deps in [true, false] {
            // `libonly` exists, but only in `lib`, which `app` does not select.
            let error = plan_projects_call(
                &call,
                &[ProjectSelector::exact("app")],
                ignore_deps,
                false,
            )
            .unwrap_err();
            match error.0 {
                ExecutionPlanProviderErrorInner::ExactTaskNotFound {
                    task,
                    ..
                } => assert_eq!(task, "libonly"),
                other => panic!("expected ExactTaskNotFound, got {other:?}"),
            }
            assert_eq!(
                plan_projects_call(
                    &call,
                    &[ProjectSelector::exact("lib")],
                    ignore_deps,
                    false
                )
                .unwrap(),
                ["lib#libonly"]
            );
            // With no project filter it is found anywhere.
            assert_eq!(
                plan_projects_call(&call, &[], ignore_deps, false).unwrap(),
                ["lib#libonly"]
            );
        }
    }
}
