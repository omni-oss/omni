use std::borrow::Cow;

use derive_new::new;
use maps::Map;
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

        let cache_info = self
            .context
            .get_cache_info(node.project_name(), node.task_name())
            .map(|ci| Cow::Borrowed(ci));

        let dependency_hashes = if !ignore_dependencies {
            node.dependencies()
                .iter()
                .filter_map(|d| self.hash_provider.get_task_hash(d))
                .collect::<Vec<_>>()
        } else {
            vec![]
        };
        let template_context = create_template_context(
            &env_vars,
            meta,
            cache_info.as_ref().map(|c| &c.args),
        );

        let ctx = TaskContext {
            node,
            env_vars,
            cache_info,
            dependency_hashes,
            template_context,
        };

        Ok(ctx)
    }
}

fn create_template_context<'a>(
    env_vars: &'a Map<String, String>,
    meta: Option<&MetaConfiguration>,
    args: Option<&'a Map<String, serde_json::Value>>,
) -> omni_tera::Context {
    let mut context = omni_tera::Context::new();

    context.insert("env", env_vars);
    if let Some(meta) = meta {
        context.insert("meta", meta);
    } else {
        context.insert("meta", &MetaConfiguration::default());
    }

    if let Some(args) = args {
        context.insert("args", args);
    } else {
        context.insert("args", &Map::<String, ()>::default());
    }

    context
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
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

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(TaskContextProviderErrorKind), vis(pub))]
pub(crate) enum TaskContextProviderErrorInner {
    #[error("no env vars for task: {full_task_name}")]
    NoEnvVarsForTask { full_task_name: String },

    #[error(transparent)]
    ContextError(eyre::Report),

    #[error(transparent)]
    Custom(#[from] eyre::Report),
}
