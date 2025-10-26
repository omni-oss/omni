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
                project_matcher.add(
                    Glob::new(filter).map_err(TaskFilterErrorInner::Glob)?,
                );
            }
            Some(
                project_matcher
                    .build()
                    .map_err(TaskFilterErrorInner::Glob)?,
            )
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

pub struct DefaultTaskFilter<'b, TGetTaskMetaFn, TGetCacheInputFilesFn>
where
    TGetTaskMetaFn:
        for<'a> Fn(&'a TaskExecutionNode) -> Option<&'b MetaConfiguration>,
    TGetCacheInputFilesFn: for<'a> Fn(&'a TaskExecutionNode) -> &'b [OmniPath],
{
    task_matcher: Option<GlobSet>,
    meta_filter: Option<Evaluator>,
    dir_matcher: Option<GlobSet>,
    project_matcher: Option<GlobSet>,
    changed_files: Option<Vec<PathBuf>>,
    get_task_meta: TGetTaskMetaFn,
    get_cache_input_files: TGetCacheInputFilesFn,
    workspace_root_dir: PathBuf,
}

impl<'b, TGetTaskMetaFn, TGetCacheInputFilesFn>
    DefaultTaskFilter<'b, TGetTaskMetaFn, TGetCacheInputFilesFn>
where
    TGetTaskMetaFn:
        for<'a> Fn(&'a TaskExecutionNode) -> Option<&'b MetaConfiguration>,
    TGetCacheInputFilesFn: for<'a> Fn(&'a TaskExecutionNode) -> &'b [OmniPath],
{
    pub fn new(
        task_filters: &[&str],
        project_filters: &[&str],
        dir_filters: &[&str],
        workspace_root_dir: &Path,
        meta_filter: Option<&str>,
        scm_affected_filter: Option<&ScmAffectedFilter>,
        get_task_meta: TGetTaskMetaFn,
        get_cache_input_files: TGetCacheInputFilesFn,
    ) -> Result<Self, FilterError> {
        let task_matcher = if task_filters.is_empty() {
            None
        } else {
            let mut task_matcher = GlobSetBuilder::new();
            for filter in task_filters {
                task_matcher.add(
                    Glob::new(filter).map_err(TaskFilterErrorInner::Glob)?,
                );
            }
            Some(task_matcher.build().map_err(TaskFilterErrorInner::Glob)?)
        };

        let meta_filter = meta_filter
            .map(|filter| {
                omni_expressions::parse(filter)
                    .map_err(TaskFilterErrorInner::Expression)
            })
            .transpose()?;

        let project_matcher = if project_filters.is_empty() {
            None
        } else {
            let mut project_matcher = GlobSetBuilder::new();
            for filter in project_filters {
                project_matcher.add(
                    Glob::new(filter).map_err(TaskFilterErrorInner::Glob)?,
                );
            }
            Some(
                project_matcher
                    .build()
                    .map_err(TaskFilterErrorInner::Glob)?,
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
            let mut dir_matcher = GlobSetBuilder::new();
            for filter in dir_filters {
                let filter: Cow<str> = if filter.starts_with("/") {
                    Cow::Borrowed(filter)
                } else {
                    Cow::Owned(format!("{}/{}", workspace_root_dir_str, filter))
                };

                dir_matcher.add(
                    Glob::new(&filter).map_err(TaskFilterErrorInner::Glob)?,
                );
            }
            Some(dir_matcher.build().map_err(TaskFilterErrorInner::Glob)?)
        };

        let changed_files = if let Some(scm_f) = scm_affected_filter
            && let Some(scm) =
                get_scm_implementation(&workspace_root_dir_str, scm_f.scm)
        {
            let changed = scm
                .changed_files(
                    scm_f.base.as_deref().unwrap_or(scm.default_base()),
                    scm_f.target.as_deref().unwrap_or(scm.default_target()),
                )?
                .iter()
                .map(|p| workspace_root_dir.join(p))
                .collect::<Vec<_>>();

            Some(changed)
        } else {
            None
        };

        Ok(Self {
            task_matcher,
            meta_filter,
            project_matcher,
            dir_matcher,
            get_task_meta,
            changed_files,
            get_cache_input_files,
            workspace_root_dir: workspace_root_dir.to_path_buf(),
        })
    }
}

impl<'b, TGetTaskMetaFn, TGetCacheInputFilesFn> TaskFilter
    for DefaultTaskFilter<'b, TGetTaskMetaFn, TGetCacheInputFilesFn>
where
    TGetTaskMetaFn:
        for<'a> Fn(&'a TaskExecutionNode) -> Option<&'b MetaConfiguration>,
    TGetCacheInputFilesFn: for<'a> Fn(&'a TaskExecutionNode) -> &'b [OmniPath],
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
            && self.changed_files.is_none()
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

        if let Some(changed_files) = &self.changed_files {
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

            for file in changed_files {
                if globset.is_match(file.to_string_lossy().as_ref()) {
                    return Ok(true);
                }
            }

            return Ok(false);
        }

        Ok(true)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct FilterError {
    #[source]
    inner: TaskFilterErrorInner,
    kind: TaskFilterErrorKind,
}

impl FilterError {
    #[allow(unused)]
    pub fn kind(&self) -> TaskFilterErrorKind {
        self.kind
    }
}

impl<T: Into<TaskFilterErrorInner>> From<T> for FilterError {
    fn from(value: T) -> Self {
        let repr = value.into();
        let kind = repr.discriminant();
        Self { inner: repr, kind }
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
#[strum_discriminants(name(TaskFilterErrorKind), vis(pub))]
enum TaskFilterErrorInner {
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
            None,
            |_| Some(&meta),
            |_| &[],
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
            None,
            |_| None,
            |_| &[],
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
        );

        assert!(
            !filter
                .should_include_task(&node,)
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
            None,
            |_| None,
            |_| &[],
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
        );

        assert!(
            !filter
                .should_include_task(&node)
                .expect("should have value")
        );
    }
}
