use derive_new::new;
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

        let cache_info = self
            .context
            .get_cache_info(node.project_name(), node.task_name());

        let dependency_hashes = if !ignore_dependencies {
            node.dependencies()
                .iter()
                .filter_map(|d| self.hash_provider.get_task_hash(d))
                .collect::<Vec<_>>()
        } else {
            vec![]
        };

        let ctx = TaskContext {
            node,
            env_vars,
            cache_info,
            dependency_hashes,
        };

        Ok(ctx)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct TaskContextProviderError {
    #[source]
    inner: TaskContextProviderErrorInner,
    kind: TaskContextProviderErrorKind,
}

impl TaskContextProviderError {
    #[allow(unused)]
    pub fn kind(&self) -> TaskContextProviderErrorKind {
        self.kind
    }
}

impl<T: Into<TaskContextProviderErrorInner>> From<T>
    for TaskContextProviderError
{
    fn from(value: T) -> Self {
        let inner = value.into();
        let kind = inner.discriminant();
        Self { inner, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(TaskContextProviderErrorKind), vis(pub))]
enum TaskContextProviderErrorInner {
    #[error("no env vars for task: {full_task_name}")]
    NoEnvVarsForTask { full_task_name: String },

    #[error(transparent)]
    ContextError(eyre::Report),
}
