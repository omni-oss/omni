use globset::{Glob, GlobSet, GlobSetBuilder};
use omni_configurations::MetaConfiguration;
use omni_core::{Project, TaskExecutionNode};
use omni_expressions::Evaluator;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{ProjectFilter, TaskFilter};

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

pub struct DefaultTaskFilter<'b, TGetTaskMetaFn>
where
    TGetTaskMetaFn:
        for<'a> Fn(&'a TaskExecutionNode) -> Option<&'b MetaConfiguration>,
{
    task_matcher: Option<GlobSet>,
    meta_filter: Option<Evaluator>,
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
        meta_filter: Option<&str>,
        get_task_meta: TGetTaskMetaFn,
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

        Ok(Self {
            task_matcher,
            meta_filter,
            project_matcher,
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
        // (None, None, None) => true,
        // (None, Some(m)) => {
        //     let meta = (self.get_task_meta)(node);
        //     let meta = if let Some(meta) = meta {
        //         meta.clone().into_expression_context()?
        //     } else {
        //         omni_expressions::Context::default()
        //     };

        //     self.task_matcher.is_match(node.task_name())
        //         && m.coerce_to_bool(&meta).unwrap_or(false)
        // }
        // (Some(p), None) => {
        //     self.task_matcher.is_match(node.task_name())
        //         && p.is_match(node.project_name())
        // }
        // (Some(p), Some(m)) => {
        //     let meta = if let Some(meta) = (self.get_task_meta)(node) {
        //         meta.clone().into_expression_context()?
        //     } else {
        //         omni_expressions::Context::default()
        //     };

        //     self.task_matcher.is_match(node.task_name())
        //         && p.is_match(node.project_name())
        //         && m.coerce_to_bool(&meta).unwrap_or(false)
        // }
        //
        let get_meta = |node: &TaskExecutionNode| {
            if let Some(meta) = (self.get_task_meta)(node) {
                meta.clone().into_expression_context()
            } else {
                Ok(omni_expressions::Context::default())
            }
        };
        Ok(
            match (&self.project_matcher, &self.meta_filter, &self.task_matcher)
            {
                (None, None, None) => true,
                (None, None, Some(tm)) => tm.is_match(node.task_name()),
                (None, Some(mf), None) => {
                    let meta = get_meta(node)?;

                    mf.coerce_to_bool(&meta).unwrap_or(false)
                }
                (None, Some(mf), Some(tm)) => {
                    let meta = get_meta(node)?;

                    tm.is_match(node.task_name())
                        && mf.coerce_to_bool(&meta).unwrap_or(false)
                }
                (Some(pm), None, None) => pm.is_match(node.project_name()),
                (Some(pm), None, Some(tm)) => {
                    pm.is_match(node.project_name())
                        && tm.is_match(node.task_name())
                }
                (Some(pm), Some(mf), None) => {
                    let meta = get_meta(node)?;

                    pm.is_match(node.project_name())
                        && mf.coerce_to_bool(&meta).unwrap_or(false)
                }
                (Some(pm), Some(mf), Some(tm)) => {
                    let meta = get_meta(node)?;

                    pm.is_match(node.project_name())
                        && tm.is_match(node.task_name())
                        && mf.coerce_to_bool(&meta).unwrap_or(false)
                }
            },
        )
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
        );

        assert!(filter.should_include_task(&node).expect("should be true"));
    }

    #[test]
    fn test_default_task_filter_meta_filter_mo_meta_configuration() {
        let filter =
            DefaultTaskFilter::new(&["test"], &[], Some("a == 1"), |_| None)
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
        let filter =
            DefaultTaskFilter::new(&["test"], &["project1"], None, |_| None)
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
                .should_include_task(&node,)
                .expect("should have value")
        );
    }
}
