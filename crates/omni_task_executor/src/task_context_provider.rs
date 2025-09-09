use std::collections::HashMap;

use derive_new::new;
use omni_context::{ContextSys, LoadedContext};
use omni_core::TaskExecutionNode;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{TaskExecutionResult, task_context::TaskContext};

pub trait TaskContextProvider<'a>: 'a {
    type Error;

    fn get_task_contexts<'b>(
        &self,
        batch: &'b [TaskExecutionNode],
        ignore_dependencies: bool,
        overall_results: &HashMap<String, TaskExecutionResult>,
    ) -> Result<Vec<TaskContext<'b>>, Self::Error>
    where
        'a: 'b;
}

#[derive(Debug, Clone, new)]
pub struct ContextTaskContextProvider<'a, TSys: ContextSys> {
    context: &'a LoadedContext<TSys>,
}

impl<'a, TSys: ContextSys> TaskContextProvider<'a>
    for ContextTaskContextProvider<'a, TSys>
{
    type Error = TaskContextProviderError;

    fn get_task_contexts<'b>(
        &self,
        batch: &'b [TaskExecutionNode],
        ignore_dependencies: bool,
        overall_results: &HashMap<String, TaskExecutionResult>,
    ) -> Result<Vec<TaskContext<'b>>, TaskContextProviderError>
    where
        'a: 'b,
    {
        let mut task_ctxs = Vec::with_capacity(batch.len());
        for node in batch {
            let dependencies = if ignore_dependencies {
                node.dependencies()
            } else {
                &[]
            };

            let envs =
                self.context.get_task_env_vars(node).ok_or_else(|| {
                    TaskContextProviderErrorInner::NoEnvVarsForTask {
                        full_task_name: node.full_task_name().to_string(),
                    }
                })?;

            let cache_info = self
                .context
                .get_cache_info(node.project_name(), node.task_name());

            let dep_hashes = dependencies
                .iter()
                .filter_map(|d| overall_results.get(d).and_then(|r| r.hash()))
                .collect::<Vec<_>>();
            let ctx = TaskContext {
                node,
                dependencies,
                env_vars: envs,
                cache_info,
                dependency_hashes: dep_hashes,
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
