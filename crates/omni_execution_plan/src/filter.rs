use std::{
    borrow::Cow,
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use globset::GlobSet;
use omni_configurations::MetaConfiguration;
use omni_core::{Project, TaskExecutionNode};
use omni_expressions::Evaluator;
use omni_glob::{GlobMatcher, GlobOptions, GlobPatterns, include_set};
use omni_scm::{Scm, get_scm_implementation};
use omni_types::{OmniPath, Root, enum_map};
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{
    MetaFilterCollector, MetaFilterError, MetaFilterErrorLimits, ProjectFilter,
    ScmAffectedFilter, TaskFilter, TaskSelector,
};

pub struct DefaultProjectFilter {
    project_matcher: Option<Arc<GlobSet>>,
    fast_path_include_all: bool,
}

impl DefaultProjectFilter {
    pub fn new(project_filters: &[&str]) -> Result<Self, FilterError> {
        let project_matcher = if project_filters.is_empty() {
            None
        } else {
            Some(
                include_set(project_filters, GlobOptions::default())
                    .map_err(FilterErrorInner::Glob)?,
            )
        };

        Ok(Self {
            project_matcher,
            fast_path_include_all: project_filters.is_empty(),
        })
    }
}

impl ProjectFilter for DefaultProjectFilter {
    type Error = FilterError;

    fn should_include_project(
        &self,
        project: &Project,
    ) -> Result<bool, Self::Error> {
        if self.fast_path_include_all {
            return Ok(true);
        }

        if let Some(matcher) = &self.project_matcher {
            Ok(matcher.is_match(&project.name))
        } else {
            Ok(true)
        }
    }
}

pub struct DefaultTaskFilter<'b, TGetTaskMetaFn>
where
    TGetTaskMetaFn:
        for<'a> Fn(&'a TaskExecutionNode) -> Option<&'b MetaConfiguration>,
{
    task_matcher: Option<Arc<GlobSet>>,
    exact_task_names: HashSet<String>,
    meta_filter: Option<Evaluator>,
    meta_filter_source: Option<String>,
    collector: Option<MetaFilterCollector>,
    dir_matcher: Option<Arc<GlobSet>>,
    project_matcher: Option<Arc<GlobSet>>,
    get_task_meta: TGetTaskMetaFn,
}

impl<'b, TGetTaskMetaFn> DefaultTaskFilter<'b, TGetTaskMetaFn>
where
    TGetTaskMetaFn:
        for<'a> Fn(&'a TaskExecutionNode) -> Option<&'b MetaConfiguration>,
{
    pub fn new(
        task_filters: &[TaskSelector],
        project_filters: &[&str],
        dir_filters: &[&str],
        workspace_root_dir: &Path,
        meta_filter: Option<&str>,
        get_task_meta: TGetTaskMetaFn,
    ) -> Result<Self, FilterError> {
        let select_all = task_filters.is_empty()
            || task_filters.iter().any(|s| matches!(s, TaskSelector::All));
        let mut exact_task_names = HashSet::new();
        let mut task_patterns: Vec<&str> = vec![];
        if !select_all {
            for selector in task_filters {
                match selector {
                    TaskSelector::Exact(n) => {
                        exact_task_names.insert(n.clone());
                    }
                    TaskSelector::Pattern(p) => task_patterns.push(p),
                    TaskSelector::All => {}
                }
            }
        }
        let task_matcher = if task_patterns.is_empty() {
            None
        } else {
            Some(
                include_set(&task_patterns, GlobOptions::default())
                    .map_err(FilterErrorInner::Glob)?,
            )
        };

        let meta_filter_source = meta_filter.map(|f| f.to_string());
        let meta_filter = meta_filter
            .map(|filter| {
                omni_expressions::parse(filter)
                    .map_err(FilterErrorInner::Expression)
            })
            .transpose()?;

        let project_matcher = if project_filters.is_empty() {
            None
        } else {
            Some(
                include_set(project_filters, GlobOptions::default())
                    .map_err(FilterErrorInner::Glob)?,
            )
        };

        let string = workspace_root_dir.to_string_lossy();
        let workspace_root_dir_str: Cow<str> = {
            if cfg!(windows) {
                Cow::Owned(string.replace("\\", "/"))
            } else {
                Cow::Borrowed(&string)
            }
        };

        let dir_matcher = if dir_filters.is_empty() {
            None
        } else {
            let dir_patterns = dir_filters
                .iter()
                .map(|filter| {
                    if filter.starts_with('/') {
                        (*filter).to_string()
                    } else {
                        format!("{}/{}", workspace_root_dir_str, filter)
                    }
                })
                .collect::<Vec<_>>();

            Some(
                include_set(&dir_patterns, GlobOptions::default())
                    .map_err(FilterErrorInner::Glob)?,
            )
        };

        Ok(Self {
            task_matcher,
            meta_filter,
            meta_filter_source,
            collector: None,
            exact_task_names,
            project_matcher,
            dir_matcher,
            get_task_meta,
        })
    }

    /// Instead of returning an error for the first task whose meta filter
    /// fails to evaluate, record failures in a bounded collector, exclude the
    /// task, and keep going. Retrieve the aggregate with
    /// [`Self::take_meta_error`].
    pub fn collect_meta_errors(
        mut self,
        limits: MetaFilterErrorLimits,
    ) -> Self {
        if let Some(source) = &self.meta_filter_source {
            self.collector = Some(MetaFilterCollector::new(source, limits));
        }
        self
    }

    pub fn take_meta_error(&self) -> Option<MetaFilterError> {
        self.collector.as_ref().and_then(|c| c.take())
    }
}

impl<'b, TGetTaskMetaFn> TaskFilter for DefaultTaskFilter<'b, TGetTaskMetaFn>
where
    TGetTaskMetaFn:
        for<'a> Fn(&'a TaskExecutionNode) -> Option<&'b MetaConfiguration>,
{
    type Error = FilterError;

    fn should_include_task(
        &self,
        node: &TaskExecutionNode,
    ) -> Result<bool, Self::Error> {
        if self.task_matcher.is_none()
            && self.exact_task_names.is_empty()
            && self.meta_filter.is_none()
            && self.project_matcher.is_none()
            && self.dir_matcher.is_none()
        {
            return Ok(true);
        }

        if let Some(pm) = &self.project_matcher
            && !pm.is_match(node.project_name())
        {
            return Ok(false);
        }

        let by_exact = self.exact_task_names.contains(node.task_name());
        let by_pattern = self
            .task_matcher
            .as_ref()
            .is_some_and(|tm| tm.is_match(node.task_name()));
        if (self.task_matcher.is_some() || !self.exact_task_names.is_empty())
            && !by_exact
            && !by_pattern
        {
            return Ok(false);
        }

        if let Some(dm) = &self.dir_matcher
            && !dm.is_match(node.project_dir().to_string_lossy().as_ref())
        {
            return Ok(false);
        }

        if let Some(mf) = &self.meta_filter {
            let meta = (self.get_task_meta)(node);
            let result = match meta {
                Some(m) => m.to_expression_context(),
                None => MetaConfiguration::empty_expression_context(),
            }
            .and_then(|ctx| mf.evaluate_bool(&ctx));

            match result {
                Ok(true) => {}
                Ok(false) => return Ok(false),
                Err(e) => {
                    if let Some(collector) = &self.collector {
                        collector.record(node, &e, meta);
                        return Ok(false);
                    }
                    let one = MetaFilterCollector::new(
                        self.meta_filter_source.clone().unwrap_or_default(),
                        MetaFilterErrorLimits::with_samples(1),
                    );
                    one.record(node, &e, meta);
                    let err = one.take().expect("one failure was recorded");
                    return Err(FilterErrorInner::MetaFilter(err).into());
                }
            }
        }

        // if let Some(changed_files) = &self.changed_files {
        // }

        Ok(true)
    }
}

pub struct DefaultTaskScmAffectedFilter<'b, TGetCacheInputFilesFn>
where
    TGetCacheInputFilesFn:
        for<'a> Fn(&'a TaskExecutionNode) -> &'b GlobPatterns<OmniPath>,
{
    get_cache_input_files: TGetCacheInputFilesFn,
    changed_files: Vec<PathBuf>,
    workspace_root_dir: PathBuf,
}

impl<'b, TGetCacheInputFilesFn>
    DefaultTaskScmAffectedFilter<'b, TGetCacheInputFilesFn>
where
    TGetCacheInputFilesFn:
        for<'a> Fn(&'a TaskExecutionNode) -> &'b GlobPatterns<OmniPath>,
{
    pub fn new(
        workspace_root_dir: &Path,
        scm_affected_filter: &ScmAffectedFilter,
        get_cache_input_files: TGetCacheInputFilesFn,
    ) -> Result<Self, FilterError> {
        let str = workspace_root_dir.to_string_lossy();
        let workspace_root_dir_str: Cow<str> = if cfg!(windows)
            && str.contains("\\")
        {
            Cow::Owned(workspace_root_dir.to_string_lossy().replace("\\", "/"))
        } else {
            str
        };

        let scm = get_scm_implementation(
            &workspace_root_dir_str,
            scm_affected_filter.scm,
        )
        .ok_or_else(|| {
            FilterErrorInner::Scm(omni_scm::error::Error::no_repository_found())
        })?;

        let changed = scm.changed_files(
            scm_affected_filter
                .base
                .as_deref()
                .unwrap_or(scm.default_base()),
            scm_affected_filter
                .target
                .as_deref()
                .unwrap_or(scm.default_target()),
        )?;

        let changed = changed
            .into_iter()
            .map(|path| {
                if path.is_absolute() {
                    path
                } else {
                    workspace_root_dir.join(path)
                }
            })
            .collect::<Vec<_>>();

        Ok(Self {
            get_cache_input_files,
            changed_files: changed,
            workspace_root_dir: workspace_root_dir.to_path_buf(),
        })
    }
}

impl<'b, TGetCacheInputFilesFn> TaskFilter
    for DefaultTaskScmAffectedFilter<'b, TGetCacheInputFilesFn>
where
    TGetCacheInputFilesFn:
        for<'a> Fn(&'a TaskExecutionNode) -> &'b GlobPatterns<OmniPath>,
{
    type Error = FilterError;

    fn should_include_task(
        &self,
        node: &TaskExecutionNode,
    ) -> Result<bool, Self::Error> {
        let cache_input_files = (self.get_cache_input_files)(node);
        let root_map = enum_map! {
            Root::Project => node.project_dir(),
            Root::Workspace => self.workspace_root_dir.as_path(),
        };
        let resolve = |files: &[OmniPath]| {
            files
                .iter()
                .map(|file| {
                    let resolved = file.resolve(&root_map);
                    let resolved = if !resolved.is_absolute() {
                        node.project_dir().join(path_clean::clean(resolved))
                    } else {
                        resolved.to_path_buf()
                    };
                    let resolved = resolved.to_string_lossy();
                    if cfg!(windows) {
                        resolved.replace("\\", "/")
                    } else {
                        resolved.into_owned()
                    }
                })
                .collect::<Vec<String>>()
        };

        let include = resolve(&cache_input_files.include);
        let exclude = resolve(&cache_input_files.exclude);

        // A changed file counts only when it matches the include set and no
        // exclude pattern, keeping this filter in lockstep with what the cache
        // hashes.
        let matcher = GlobMatcher::from_globs(
            &include,
            &exclude,
            GlobOptions::default(),
        )?;

        for file in &self.changed_files {
            if matcher.is_match(file.to_string_lossy().as_ref()) {
                return Ok(true);
            }
        }

        Ok(false)
    }
}

#[derive(Debug, thiserror::Error, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
pub struct FilterError(pub(crate) FilterErrorInner);

impl FilterError {
    pub fn meta_filter_error(&self) -> Option<&MetaFilterError> {
        match &self.0 {
            FilterErrorInner::MetaFilter(e) => Some(e),
            _ => None,
        }
    }

    #[allow(unused)]
    pub fn kind(&self) -> FilterErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<FilterErrorInner>> From<T> for FilterError {
    fn from(value: T) -> Self {
        let repr = value.into();
        Self(repr)
    }
}

pub trait ProjectFilterExt: ProjectFilter {
    fn filter_projects<'a>(&self, projects: &'a [Project]) -> Vec<&'a Project> {
        projects
            .iter()
            .filter(|p| self.should_include_project(p).unwrap_or(false))
            .collect::<Vec<_>>()
    }

    #[allow(unused)]
    fn filter_projects_cloned(&self, projects: &[Project]) -> Vec<Project> {
        projects
            .iter()
            .filter(|p| self.should_include_project(p).unwrap_or(false))
            .cloned()
            .collect::<Vec<_>>()
    }
}

impl<T: ProjectFilter> ProjectFilterExt for T {}

pub trait TaskFilterExt: TaskFilter {
    #[allow(unused)]
    fn filter_tasks<'a>(
        &self,
        tasks: &'a [TaskExecutionNode],
    ) -> Result<Vec<&'a TaskExecutionNode>, Self::Error> {
        let mut out = vec![];
        for t in tasks {
            if self.should_include_task(t)? {
                out.push(t);
            }
        }
        Ok(out)
    }

    fn filter_tasks_cloned(
        &self,
        tasks: &[TaskExecutionNode],
    ) -> Result<Vec<TaskExecutionNode>, Self::Error> {
        Ok(self.filter_tasks(tasks)?.into_iter().cloned().collect())
    }
}

impl<T: TaskFilter> TaskFilterExt for T {}

#[derive(
    Debug, thiserror::Error, EnumDiscriminants, omni_diagnostic::Diagnostic,
)]
#[strum_discriminants(name(FilterErrorKind), vis(pub))]
pub(crate) enum FilterErrorInner {
    #[error(transparent)]
    #[diagnostic(delegate)]
    Glob(#[from] globset::Error),

    #[error(transparent)]
    #[diagnostic(delegate)]
    Expression(#[from] omni_expressions::Error),

    #[error(transparent)]
    #[diagnostic(delegate)]
    Scm(#[from] omni_scm::error::Error),

    #[error(transparent)]
    #[diagnostic(delegate)]
    MetaFilter(MetaFilterError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use config_utils::{DictConfig, DynValue};
    use omni_command_config::CommandConfig;
    use omni_core::TaskExecutionNode;

    /// What the CLI prints for a failure.
    fn shown(report: eyre::Report) -> String {
        omni_diagnostic::DiagnosticReport::from_report(&report)
            .map(|d| d.to_text())
            .unwrap_or_else(|| format!("{report:#}"))
    }

    #[test_log::test]
    fn test_default_task_filter_project_name_and_meta_filter_matching_all() {
        let meta = MetaConfiguration::new(DictConfig::value(maps::map! {
            "a".to_string() => DynValue::new_integer(-1),
        }));

        let filter = DefaultTaskFilter::new(
            &[TaskSelector::pattern("test")],
            &["project1"],
            &[],
            Path::new(""),
            Some("a == -1"),
            |_| Some(&meta),
        )
        .unwrap();

        let node = TaskExecutionNode::new(
            "test".to_string(),
            Some(CommandConfig::Shell("echo test".to_string())),
            None,
            "project1".to_string(),
            std::path::PathBuf::from(""),
            vec![],
            true.into(),
            false,
            false,
            None,
            None,
        );
        assert!(
            filter
                .should_include_task(&node)
                .expect("should have value"),
            "should include task that matches project name and meta filter"
        );
    }

    fn node(project: &str, task: &str) -> TaskExecutionNode {
        TaskExecutionNode::new(
            task.to_string(),
            Some(CommandConfig::Shell("echo test".to_string())),
            None,
            project.to_string(),
            std::path::PathBuf::from(format!("/ws/{project}")),
            vec![],
            true.into(),
            false,
            false,
            None,
            None,
        )
    }

    fn kind_meta(kind: &str) -> MetaConfiguration {
        MetaConfiguration::new(DictConfig::value(maps::map! {
            "kind".to_string() => DynValue::new_string(kind),
        }))
    }

    #[test_log::test]
    fn test_meta_filter_missing_key_is_an_error() {
        let filter = DefaultTaskFilter::new(
            &[TaskSelector::pattern("test")],
            &[],
            &[],
            Path::new(""),
            Some("a == 1"),
            |_| None,
        )
        .unwrap();

        let err = filter
            .should_include_task(&node("project1", "test"))
            .unwrap_err();
        let FilterErrorInner::MetaFilter(e) = &err.0 else {
            panic!("expected meta filter error, got {err}");
        };
        assert_eq!(e.filter, "a == 1");
        assert_eq!(e.total, 1);
        assert!(e.groups[0].cause.contains("Undeclared reference to 'a'"));
        assert_eq!(e.groups[0].samples[0].project, "project1");
        assert_eq!(e.groups[0].samples[0].task, "test");
    }

    #[test_log::test]
    fn test_meta_filter_has_handles_missing_key() {
        let filter = DefaultTaskFilter::new(
            &[],
            &[],
            &[],
            Path::new(""),
            Some("has(meta.kind) && meta.kind == 'e2e'"),
            |_| None,
        )
        .unwrap();
        assert!(!filter.should_include_task(&node("p", "test")).unwrap());
    }

    #[test_log::test]
    fn test_meta_filter_non_bool_is_an_error() {
        let meta = kind_meta("e2e");
        let filter = DefaultTaskFilter::new(
            &[],
            &[],
            &[],
            Path::new(""),
            Some("kind"),
            |_| Some(&meta),
        )
        .unwrap();
        let err = filter.should_include_task(&node("p", "test")).unwrap_err();
        let shown = shown(eyre::Report::new(err));
        assert!(shown.contains("got string"), "{shown}");
    }

    #[test_log::test]
    fn test_meta_filter_skipped_for_tasks_excluded_by_other_matchers() {
        let filter = DefaultTaskFilter::new(
            &[TaskSelector::pattern("build")],
            &[],
            &[],
            Path::new(""),
            Some("missing == 1"),
            |_| None,
        )
        .unwrap();
        assert!(!filter.should_include_task(&node("p", "test")).unwrap());
    }

    #[test_log::test]
    fn test_collect_aggregates_with_bounded_samples() {
        let filter = DefaultTaskFilter::new(
            &[],
            &[],
            &[],
            Path::new(""),
            Some("knd == 'e2e'"),
            |_| None,
        )
        .unwrap()
        .collect_meta_errors(MetaFilterErrorLimits::default());

        for i in 0..500 {
            assert!(
                !filter
                    .should_include_task(&node(&format!("p{i}"), "test"))
                    .unwrap()
            );
        }
        let e = filter.take_meta_error().unwrap();
        assert_eq!(e.total, 500);
        assert_eq!(e.groups.len(), 1);
        assert_eq!(e.groups[0].count, 500);
        assert_eq!(e.groups[0].samples.len(), 3);
        assert_eq!(e.groups[0].omitted, 497);
        assert_eq!(e.groups[0].samples[0].project, "p0");
        assert!(filter.take_meta_error().is_none());
    }

    #[test_log::test]
    fn test_collect_samples_zero_and_large() {
        for (n, expected) in [(0, 0), (1, 1), (1000, 5)] {
            let filter = DefaultTaskFilter::new(
                &[],
                &[],
                &[],
                Path::new(""),
                Some("knd == 'e2e'"),
                |_| None,
            )
            .unwrap()
            .collect_meta_errors(MetaFilterErrorLimits::with_samples(n));
            for i in 0..5 {
                filter
                    .should_include_task(&node(&format!("p{i}"), "t"))
                    .unwrap();
            }
            let e = filter.take_meta_error().unwrap();
            assert_eq!(e.groups[0].samples.len(), expected);
            assert_eq!(e.total, 5);
        }
    }

    #[test_log::test]
    fn test_collect_limits_causes_and_hints_typo() {
        let meta = kind_meta("unit");
        let filter = DefaultTaskFilter::new(
            &[],
            &[],
            &[],
            Path::new(""),
            Some("knd == 'e2e'"),
            |_| Some(&meta),
        )
        .unwrap()
        .collect_meta_errors(MetaFilterErrorLimits::default());
        filter.should_include_task(&node("web", "test")).unwrap();
        let e = filter.take_meta_error().unwrap();
        let hint = e.groups[0].hint.as_deref().unwrap();
        assert!(hint.contains("Did you mean `kind`"), "{hint}");
        let shown = shown(eyre::Report::new(e));
        assert!(shown.contains("\"kind\": \"unit\""), "{shown}");
    }

    #[test_log::test]
    fn test_collect_more_than_ten_causes_counts_other() {
        let meta = kind_meta("unit");
        let mut exprs = vec![];
        for i in 0..12 {
            exprs.push(format!("missing{i} == 1"));
        }
        // One filter evaluates one expression, so reuse the collector directly.
        let collector = MetaFilterCollector::new("f", Default::default());
        for (i, ex) in exprs.iter().enumerate() {
            let err = omni_expressions::parse(ex)
                .unwrap()
                .evaluate_bool(&Default::default())
                .unwrap_err();
            collector.record(&node(&format!("p{i}"), "t"), &err, Some(&meta));
        }
        let e = collector.take().unwrap();
        assert_eq!(e.total, 12);
        assert_eq!(e.groups.len(), 10);
        assert_eq!(e.other, 2);
    }

    #[test_log::test]
    fn test_filter_tasks_propagates_errors() {
        let filter = DefaultTaskFilter::new(
            &[],
            &[],
            &[],
            Path::new(""),
            Some("nope == 1"),
            |_| None,
        )
        .unwrap();
        assert!(filter.filter_tasks(&[node("p", "t")]).is_err());
    }

    #[test_log::test]
    fn test_default_task_filter_matching_dir_filter() {
        // Non-rooted dir filters are joined onto the workspace root.
        let filter = DefaultTaskFilter::new(
            &[TaskSelector::pattern("test")],
            &[],
            &["packages/*"],
            Path::new("/ws"),
            None,
            |_| None,
        )
        .unwrap();

        let node = TaskExecutionNode::new(
            "test".to_string(),
            Some(CommandConfig::Shell("echo test".to_string())),
            None,
            "project1".to_string(),
            std::path::PathBuf::from("/ws/packages/foo"),
            vec![],
            true.into(),
            false,
            false,
            None,
            None,
        );

        assert!(
            filter
                .should_include_task(&node)
                .expect("should have value"),
            "task whose project dir matches the dir filter should be included"
        );
    }

    #[test_log::test]
    fn test_default_task_filter_not_matching_dir_filter() {
        let filter = DefaultTaskFilter::new(
            &[TaskSelector::pattern("test")],
            &[],
            &["packages/*"],
            Path::new("/ws"),
            None,
            |_| None,
        )
        .unwrap();

        let node = TaskExecutionNode::new(
            "test".to_string(),
            Some(CommandConfig::Shell("echo test".to_string())),
            None,
            "project1".to_string(),
            std::path::PathBuf::from("/ws/apps/bar"),
            vec![],
            true.into(),
            false,
            false,
            None,
            None,
        );

        assert!(
            !filter
                .should_include_task(&node)
                .expect("should have value"),
            "task outside the dir filter should be excluded"
        );
    }

    // A leading `!` in a task filter is a literal character here, not a
    // negation marker: `!build` matches only the literal name "!build", so an
    // ordinary task like "test" stays excluded. This guards against negation
    // leaking into the filters during the include_set migration. When task
    // filters deliberately gain negation, this assertion no longer holds and
    // the test should be removed or inverted.
    #[test_log::test]
    fn test_default_task_filter_leading_bang_is_literal() {
        let filter = DefaultTaskFilter::new(
            &[TaskSelector::pattern("!build")],
            &[],
            &[],
            Path::new(""),
            None,
            |_| None,
        )
        .unwrap();

        let node = TaskExecutionNode::new(
            "test".to_string(),
            Some(CommandConfig::Shell("echo test".to_string())),
            None,
            "project1".to_string(),
            std::path::PathBuf::from(""),
            vec![],
            true.into(),
            false,
            false,
            None,
            None,
        );

        assert!(
            !filter
                .should_include_task(&node)
                .expect("should have value"),
            "a leading `!` must not negate: an unrelated task stays excluded"
        );
    }

    fn selects(selectors: &[TaskSelector], task: &str) -> bool {
        let filter = DefaultTaskFilter::new(
            selectors,
            &[],
            &[],
            Path::new(""),
            None,
            |_| None,
        )
        .unwrap();
        filter.should_include_task(&node("p", task)).unwrap()
    }

    #[test]
    fn exact_selector_matches_literal_star_name_only() {
        let star = [TaskSelector::exact("*")];
        assert!(selects(&star, "*"));
        assert!(!selects(&star, "build"));
    }

    #[test]
    fn exact_selector_does_not_expand_globs() {
        let sel = [TaskSelector::exact("build*")];
        assert!(selects(&sel, "build*"));
        assert!(!selects(&sel, "build"));
        assert!(!selects(&sel, "build:ci"));
    }

    #[test]
    fn pattern_star_selects_every_task() {
        let sel = [TaskSelector::pattern("*")];
        assert!(selects(&sel, "build"));
        assert!(selects(&sel, "a/b"));
    }

    #[test]
    fn all_selector_selects_every_task() {
        assert!(selects(&[TaskSelector::All], "anything:at/all"));
    }

    #[test]
    fn no_selectors_keeps_the_select_everything_default() {
        assert!(selects(&[], "build"));
    }

    #[test]
    fn exact_and_pattern_selectors_union() {
        let sel = [TaskSelector::exact("*"), TaskSelector::pattern("test*")];
        assert!(selects(&sel, "*"));
        assert!(selects(&sel, "test:unit"));
        assert!(!selects(&sel, "build"));
    }
}
