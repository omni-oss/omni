use std::error::Error;

use derive_new::new;
use maps::UnorderedMap;
use omni_context::{ContextSys, LoadedContext};
use omni_core::TaskExecutionNode;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{TaskExecutionResult, task_context::TaskContext};

pub trait TaskContextProvider {
    type Error: Error + Send + Sync + 'static;

    fn get_task_contexts<'a>(
        &'a self,
        batch: &'a [TaskExecutionNode],
        ignore_dependencies: bool,
        hashes: &UnorderedMap<String, TaskExecutionResult>,
    ) -> Result<Vec<TaskContext<'a>>, Self::Error>;
}

#[derive(Debug, Clone, new)]
pub struct ContextTaskContextProvider<'a, TSys: ContextSys> {
    context: &'a LoadedContext<TSys>,
}

impl<'b, TSys: ContextSys> TaskContextProvider
    for ContextTaskContextProvider<'b, TSys>
{
    type Error = TaskContextProviderError;

    fn get_task_contexts<'a>(
        &'a self,
        batch: &'a [TaskExecutionNode],
        ignore_dependencies: bool,
        overall_results: &UnorderedMap<String, TaskExecutionResult>,
    ) -> Result<Vec<TaskContext<'a>>, TaskContextProviderError> {
        let mut task_ctxs = Vec::with_capacity(batch.len());
        for node in batch {
            let env_vars =
                self.context.get_task_env_vars(node).ok_or_else(|| {
                    TaskContextProviderErrorInner::NoEnvVarsForTask {
                        full_task_name: node.full_task_name().to_string(),
                    }
                })?;

            let cache_info = self
                .context
                .get_cache_info(node.project_name(), node.task_name());

            let dependency_hashes = if !ignore_dependencies {
                node.dependencies()
                    .iter()
                    .filter_map(|d| {
                        overall_results.get(d).and_then(|r| r.hash())
                    })
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

            trace::debug!(
                "added task context to queue: '{}#{}'",
                node.project_name(),
                node.task_name()
            );

            task_ctxs.push(ctx);
        }
        Ok(task_ctxs)
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
}
