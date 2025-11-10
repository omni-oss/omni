use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use globset::{Glob, GlobSet, GlobSetBuilder};
use omni_configurations::MetaConfiguration;
use omni_core::{Project, TaskExecutionNode};
use omni_expressions::Evaluator;
use omni_scm::{Scm, get_scm_implementation};
use omni_types::{OmniPath, Root, enum_map};
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{ProjectFilter, ScmAffectedFilter, TaskFilter};

pub struct DefaultProjectFilter {
    project_matcher: Option<GlobSet>,
    fast_path_include_all: bool,
}

impl DefaultProjectFilter {
    pub fn new(project_filters: &[&str]) -> Result<Self, FilterError> {
        let project_matcher = if project_filters.is_empty() {
            None
        } else {
            let mut project_matcher = GlobSetBuilder::new();
            for filter in project_filters {
                project_matcher
                    .add(Glob::new(filter).map_err(FilterErrorInner::Glob)?);
            }
            Some(project_matcher.build().map_err(FilterErrorInner::Glob)?)
        };

        Ok(Self {
            project_matcher,
            fast_path_include_all: if project_filters.is_empty() {
                true
            } else {
                false
            },
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
    task_matcher: Option<GlobSet>,
    meta_filter: Option<Evaluator>,
    dir_matcher: Option<GlobSet>,
    project_matcher: Option<GlobSet>,
    get_task_meta: TGetTaskMetaFn,
}

impl<'b, TGetTaskMetaFn> DefaultTaskFilter<'b, TGetTaskMetaFn>
where
    TGetTaskMetaFn:
        for<'a> Fn(&'a TaskExecutionNode) -> Option<&'b MetaConfiguration>,
{
    pub fn new(
        task_filters: &[&str],
        project_filters: &[&str],
        dir_filters: &[&str],
        workspace_root_dir: &Path,
        meta_filter: Option<&str>,
        get_task_meta: TGetTaskMetaFn,
    ) -> Result<Self, FilterError> {
        let task_matcher = if task_filters.is_empty() {
            None
        } else {
            let mut task_matcher = GlobSetBuilder::new();
            for filter in task_filters {
                task_matcher
                    .add(Glob::new(filter).map_err(FilterErrorInner::Glob)?);
            }
            Some(task_matcher.build().map_err(FilterErrorInner::Glob)?)
        };

        let meta_filter = meta_filter
            .map(|filter| {
                omni_expressions::parse(filter)
                    .map_err(FilterErrorInner::Expression)
            })
            .transpose()?;

        let project_matcher = if project_filters.is_empty() {
            None
        } else {
            let mut project_matcher = GlobSetBuilder::new();
            for filter in project_filters {
                project_matcher
                    .add(Glob::new(filter).map_err(FilterErrorInner::Glob)?);
            }
            Some(project_matcher.build().map_err(FilterErrorInner::Glob)?)
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
            let mut dir_matcher = GlobSetBuilder::new();
            for filter in dir_filters {
                let filter: Cow<str> = if filter.starts_with("/") {
                    Cow::Borrowed(filter)
                } else {
                    Cow::Owned(format!("{}/{}", workspace_root_dir_str, filter))
                };

                dir_matcher
                    .add(Glob::new(&filter).map_err(FilterErrorInner::Glob)?);
            }
            Some(dir_matcher.build().map_err(FilterErrorInner::Glob)?)
        };

        Ok(Self {
            task_matcher,
            meta_filter,
            project_matcher,
            dir_matcher,
            get_task_meta,
        })
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
            && self.meta_filter.is_none()
            && self.project_matcher.is_none()
            && self.dir_matcher.is_none()
        {
            return Ok(true);
        }

        let get_meta = |node: &TaskExecutionNode| {
            if let Some(meta) = (self.get_task_meta)(node) {
                meta.clone().into_expression_context()
            } else {
                Ok(omni_expressions::Context::default())
            }
        };

        if let Some(pm) = &self.project_matcher
            && !pm.is_match(node.project_name())
        {
            return Ok(false);
        }

        if let Some(tm) = &self.task_matcher
            && !tm.is_match(node.task_name())
        {
            return Ok(false);
        }

        if let Some(dm) = &self.dir_matcher
            && !dm.is_match(node.project_dir().to_string_lossy().as_ref())
        {
            return Ok(false);
        }

        if let Some(mf) = &self.meta_filter
            && !mf.coerce_to_bool(&get_meta(node)?).unwrap_or(false)
        {
            return Ok(false);
        }

        // if let Some(changed_files) = &self.changed_files {
        // }

        Ok(true)
    }
}

pub struct DefaultTaskScmAffectedFilter<'b, TGetCacheInputFilesFn>
where
    TGetCacheInputFilesFn: for<'a> Fn(&'a TaskExecutionNode) -> &'b [OmniPath],
{
    get_cache_input_files: TGetCacheInputFilesFn,
    changed_files: Vec<PathBuf>,
    workspace_root_dir: PathBuf,
}

impl<'b, TGetCacheInputFilesFn>
    DefaultTaskScmAffectedFilter<'b, TGetCacheInputFilesFn>
where
    TGetCacheInputFilesFn: for<'a> Fn(&'a TaskExecutionNode) -> &'b [OmniPath],
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
            &scm_affected_filter
                .base
                .as_deref()
                .unwrap_or(scm.default_base()),
            &scm_affected_filter
                .target
                .as_deref()
                .unwrap_or(scm.default_target()),
        )?;

        #[cfg(feature = "enable-tracing")]
        {
            for path in &changed {
                tracing::trace!("changed file: {:?}", path);
            }
        }

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
    TGetCacheInputFilesFn: for<'a> Fn(&'a TaskExecutionNode) -> &'b [OmniPath],
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
        let mut builder = GlobSetBuilder::new();

        for file in cache_input_files {
            let resolved = file.resolve(&root_map);
            let resolved = if !resolved.is_absolute() {
                node.project_dir().join(path_clean::clean(resolved))
            } else {
                resolved.to_path_buf()
            };
            let resolved = resolved.to_string_lossy();
            let pattern: Cow<str> = if cfg!(windows) {
                Cow::Owned(resolved.replace("\\", "/"))
            } else {
                Cow::Borrowed(&resolved)
            };

            builder.add(Glob::new(&pattern)?);
        }

        let globset = builder.build()?;

        for file in &self.changed_files {
            if globset.is_match(file.to_string_lossy().as_ref()) {
                return Ok(true);
            }
        }

        return Ok(false);
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct FilterError(pub(crate) FilterErrorInner);

impl FilterError {
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
    ) -> Vec<&'a TaskExecutionNode> {
        tasks
            .iter()
            .filter(|t| self.should_include_task(t).unwrap_or(false))
            .collect::<Vec<_>>()
    }

    fn filter_tasks_cloned(
        &self,
        tasks: &[TaskExecutionNode],
    ) -> Vec<TaskExecutionNode> {
        tasks
            .iter()
            .filter(|t| self.should_include_task(t).unwrap_or(false))
            .cloned()
            .collect::<Vec<_>>()
    }
}

impl<T: TaskFilter> TaskFilterExt for T {}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(FilterErrorKind), vis(pub))]
pub(crate) enum FilterErrorInner {
    #[error(transparent)]
    Glob(#[from] globset::Error),

    #[error(transparent)]
    Expression(#[from] omni_expressions::Error),

    #[error(transparent)]
    Scm(#[from] omni_scm::error::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use config_utils::DictConfig;
    use omni_configurations::{MetaConfiguration, MetaValue};
    use omni_core::TaskExecutionNode;

    #[test]
    fn test_default_task_filter_project_name_and_meta_filter_matching_all() {
        let meta = MetaConfiguration::new(DictConfig::value(maps::map! {
            "a".to_string() => MetaValue::new_integer(1),
        }));

        let filter = DefaultTaskFilter::new(
            &["test"],
            &["project1"],
            &[],
            Path::new(""),
            Some("a == 1"),
            |_| Some(&meta),
        )
        .unwrap();

        let node = TaskExecutionNode::new(
            "test".to_string(),
            "echo test".to_string(),
            "project1".to_string(),
            std::path::PathBuf::from(""),
            vec![],
            true,
            false,
            false,
            None,
            None,
        );

        assert!(filter.should_include_task(&node).expect("should be true"));
    }

    #[test]
    fn test_default_task_filter_meta_filter_mo_meta_configuration() {
        let filter = DefaultTaskFilter::new(
            &["test"],
            &[],
            &[],
            Path::new(""),
            Some("a == 1"),
            |_| None,
        )
        .unwrap();

        let node = TaskExecutionNode::new(
            "test".to_string(),
            "echo test".to_string(),
            "project1".to_string(),
            std::path::PathBuf::from(""),
            vec![],
            true,
            false,
            false,
            None,
            None,
        );

        assert!(
            !filter
                .should_include_task(&node)
                .expect("should have value")
        );
    }

    #[test]
    fn test_default_task_filter_not_matching_project_name() {
        let filter = DefaultTaskFilter::new(
            &["test"],
            &["project1"],
            &[],
            Path::new(""),
            None,
            |_| None,
        )
        .unwrap();

        let node = TaskExecutionNode::new(
            "test".to_string(),
            "echo test".to_string(),
            "project2".to_string(),
            std::path::PathBuf::from(""),
            vec![],
            true,
            false,
            false,
            None,
            None,
        );

        assert!(
            !filter
                .should_include_task(&node)
                .expect("should have value")
        );
    }
}
