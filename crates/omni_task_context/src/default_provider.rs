use std::borrow::Cow;

use derive_new::new;
use maps::{Map, UnorderedMap};
use omni_configurations::MetaConfiguration;
use omni_core::TaskExecutionNode;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{Context, TaskContext, TaskContextProvider, TaskHashProvider};

#[derive(new)]
pub struct DefaultTaskContextProvider<
    'a,
    THashProvider: TaskHashProvider + 'a,
    TContext: Context + 'a,
> {
    hash_provider: THashProvider,
    context: TContext,
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a, THashProvider: TaskHashProvider, TContext: Context>
    TaskContextProvider<'a>
    for DefaultTaskContextProvider<'a, THashProvider, TContext>
{
    type Error = TaskContextProviderError;

    fn get_task_context(
        &'a self,
        node: &'a TaskExecutionNode,
        ignore_dependencies: bool,
    ) -> Result<TaskContext<'a>, Self::Error> {
        let env_vars = self
            .context
            .get_task_env_vars(node)
            .map_err(|e| TaskContextProviderErrorInner::ContextError(e.into()))?
            .ok_or_else(|| TaskContextProviderErrorInner::NoEnvVarsForTask {
                full_task_name: node.full_task_name().to_string(),
            })?;

        let meta = self
            .context
            .get_task_meta_config(node.project_name(), node.task_name())
            .or_else(|| {
                self.context.get_project_meta_config(node.project_name())
            });

        let cache_info_ref = self
            .context
            .get_cache_info(node.project_name(), node.task_name());
        let cache_info = cache_info_ref.map(Cow::Borrowed);

        let output_logs = self
            .context
            .get_output_logs(node.project_name(), node.task_name())
            .copied();

        let dependency_hashes = if !ignore_dependencies {
            node.dependencies()
                .iter()
                .flat_map(|d| {
                    self.hash_provider.get_dependency_hashes(d.as_ref())
                })
                .collect::<Vec<_>>()
        } else {
            vec![]
        };
        let effective_digest =
            crate::effective_task::build(node, cache_info_ref, meta).digest();
        let args = merge_args(
            cache_info_ref.map(|c| &c.args),
            self.context
                .get_task_override_args(node.project_name(), node.task_name()),
        );
        let template_context = create_template_context(&env_vars, meta, &args);

        let ctx = TaskContext {
            node,
            env_vars,
            cache_info,
            args,
            effective_digest,
            output_logs,
            dependency_hashes,
            template_context,
        };

        Ok(ctx)
    }
}

/// The configured args overlaid with the invocation overrides. Computed once
/// and used by both the template context and the cache key, so they cannot
/// diverge.
fn merge_args<'a>(
    args: Option<&'a Map<String, serde_json::Value>>,
    override_args: Option<&UnorderedMap<String, serde_json::Value>>,
) -> Cow<'a, Map<String, serde_json::Value>> {
    let overrides = override_args.filter(|o| !o.is_empty());
    match (args, overrides) {
        (Some(args), None) => Cow::Borrowed(args),
        (args, Some(overrides)) => {
            let mut merged = args.cloned().unwrap_or_default();
            for (k, v) in overrides {
                merged.insert(k.to_owned(), v.to_owned());
            }
            Cow::Owned(merged)
        }
        (None, None) => Cow::Owned(Map::default()),
    }
}

fn create_template_context(
    env_vars: &Map<String, String>,
    meta: Option<&MetaConfiguration>,
    args: &Map<String, serde_json::Value>,
) -> omni_tera::Context {
    let mut context = omni_tera::context::STANDARD.clone();

    context.insert("env", env_vars);
    if let Some(meta) = meta {
        context.insert("meta", meta);
    } else {
        context.insert("meta", &MetaConfiguration::default());
    }
    context.insert("args", args);

    context
}

#[derive(Debug, thiserror::Error, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
pub struct TaskContextProviderError(pub(crate) TaskContextProviderErrorInner);

impl TaskContextProviderError {
    #[allow(unused)]
    pub fn kind(&self) -> TaskContextProviderErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<TaskContextProviderErrorInner>> From<T>
    for TaskContextProviderError
{
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

#[derive(
    Debug, thiserror::Error, EnumDiscriminants, omni_diagnostic::Diagnostic,
)]
#[strum_discriminants(name(TaskContextProviderErrorKind), vis(pub))]
pub(crate) enum TaskContextProviderErrorInner {
    #[error("no env vars for task: {full_task_name}")]
    #[diagnostic(skip)]
    NoEnvVarsForTask { full_task_name: String },

    #[error(transparent)]
    #[diagnostic(delegate)]
    ContextError(eyre::Report),

    #[error(transparent)]
    #[diagnostic(delegate)]
    Custom(#[from] eyre::Report),
}

#[cfg(test)]
mod diagnostic_tests {
    use omni_core::ProjectGraphError;
    use omni_diagnostic::find;

    use super::*;

    #[test]
    fn context_and_custom_reports_delegate_to_their_diagnostic() {
        for inner in [
            TaskContextProviderErrorInner::ContextError(eyre::Report::new(
                ProjectGraphError::not_found("web"),
            )),
            TaskContextProviderErrorInner::Custom(eyre::Report::new(
                ProjectGraphError::not_found("web"),
            )),
        ] {
            let r = eyre::Report::new(TaskContextProviderError::from(inner));
            assert_eq!(find(&r).unwrap().code(), "project.not_found");
        }
    }

    #[test]
    fn plain_failures_stay_uncoded() {
        let missing = eyre::Report::new(TaskContextProviderError::from(
            TaskContextProviderErrorInner::NoEnvVarsForTask {
                full_task_name: "app#build".into(),
            },
        ));
        assert!(find(&missing).is_none());

        let plain = eyre::Report::new(TaskContextProviderError::from(
            TaskContextProviderErrorInner::Custom(eyre::eyre!("boom")),
        ));
        assert!(find(&plain).is_none());
    }
}

#[cfg(test)]
mod merge_args_tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> Map<String, serde_json::Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), serde_json::json!(v)))
            .collect()
    }

    fn overrides(
        pairs: &[(&str, &str)],
    ) -> UnorderedMap<String, serde_json::Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), serde_json::json!(v)))
            .collect()
    }

    #[test]
    fn no_override_borrows_the_configured_args() {
        let configured = map(&[("a", "1")]);
        for none in [None, Some(&overrides(&[]))] {
            let merged = merge_args(Some(&configured), none);
            assert!(matches!(merged, Cow::Borrowed(_)));
            assert_eq!(*merged, configured);
        }
    }

    #[test]
    fn an_override_wins_and_adds() {
        let configured = map(&[("a", "1"), ("b", "2")]);
        let merged = merge_args(
            Some(&configured),
            Some(&overrides(&[("b", "x"), ("c", "3")])),
        );
        assert_eq!(*merged, map(&[("a", "1"), ("b", "x"), ("c", "3")]));
    }

    #[test]
    fn without_configured_args_the_overrides_stand_alone() {
        assert!(merge_args(None, None).is_empty());
        assert_eq!(
            *merge_args(None, Some(&overrides(&[("k", "v")]))),
            map(&[("k", "v")])
        );
    }
}
