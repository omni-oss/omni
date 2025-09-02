use globset::{Glob, GlobMatcher};
use omni_configurations::MetaConfiguration;
use omni_core::TaskExecutionNode;
use omni_expressions::Evaluator;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

pub struct TaskFilter {
    project_matcher: Option<GlobMatcher>,
    meta_filter: Option<Evaluator>,
}

impl TaskFilter {
    pub fn new(
        project_filter: Option<&str>,
        meta_filter: Option<&str>,
    ) -> Result<Self, FilterError> {
        let project_matcher = project_filter
            .map(|filter| {
                Glob::new(filter)
                    .map_err(FilterErrorInner::Glob)
                    .map(|g| g.compile_matcher())
            })
            .transpose()?;

        let meta_filter = meta_filter
            .map(|filter| {
                omni_expressions::parse(filter)
                    .map_err(FilterErrorInner::Expression)
            })
            .transpose()?;

        Ok(Self {
            project_matcher,
            meta_filter,
        })
    }
}

impl TaskFilter {
    pub fn should_include<'a, 'b>(
        &self,
        node: &'a TaskExecutionNode,
        get_meta: impl Fn(&'a TaskExecutionNode) -> Option<&'b MetaConfiguration>,
    ) -> Result<bool, FilterError> {
        Ok(match (&self.project_matcher, &self.meta_filter) {
            (None, None) => true,
            (None, Some(m)) => {
                let meta = get_meta(node);
                let meta = if let Some(meta) = meta {
                    meta.clone().into_expression_context()?
                } else {
                    omni_expressions::Context::default()
                };

                m.coerce_to_bool(&meta).unwrap_or(false)
            }
            (Some(p), None) => p.is_match(node.project_name()),
            (Some(p), Some(m)) => {
                let meta = if let Some(meta) = get_meta(node) {
                    meta.clone().into_expression_context()?
                } else {
                    omni_expressions::Context::default()
                };

                p.is_match(node.project_name())
                    && m.coerce_to_bool(&meta).unwrap_or(false)
            }
        })
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct FilterError {
    #[source]
    inner: FilterErrorInner,
    kind: FilterErrorKind,
}

impl FilterError {
    pub fn kind(&self) -> FilterErrorKind {
        self.kind
    }
}

impl<T: Into<FilterErrorInner>> From<T> for FilterError {
    fn from(value: T) -> Self {
        let repr = value.into();
        let kind = repr.discriminant();
        Self { inner: repr, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(FilterErrorKind), vis(pub))]
enum FilterErrorInner {
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
    fn test_project_name_and_meta_filter_matching_all() {
        let filter = TaskFilter::new(Some("project1"), Some("a == 1")).unwrap();

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

        let meta = MetaConfiguration::new(DictConfig::value(maps::map! {
            "a".to_string() => MetaValue::new_integer(1),
        }));

        assert!(
            filter
                .should_include(&node, |_| Some(&meta))
                .expect("should be true")
        );
    }

    #[test]
    fn test_meta_filter_mo_meta_configuration() {
        let filter = TaskFilter::new(None, Some("a == 1")).unwrap();

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
                .should_include(&node, |_| None)
                .expect("should have value")
        );
    }

    #[test]
    fn test_project_filter_not_matching_project_name() {
        let filter = TaskFilter::new(Some("project1"), None).unwrap();

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

        let meta = MetaConfiguration::new(DictConfig::value(maps::map! {
            "a".to_string() => MetaValue::new_integer(1),
        }));

        assert!(
            !filter
                .should_include(&node, |_| Some(&meta))
                .expect("should have value")
        );
    }
}
