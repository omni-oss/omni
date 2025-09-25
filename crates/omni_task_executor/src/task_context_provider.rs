use std::sync::Arc;

use maps::UnorderedMap;
use omni_context::{ContextSys, EnvVarsMap, LoadedContext, LoadedContextError};
use omni_core::TaskExecutionNode;
use omni_hasher::impls::DefaultHash;
use omni_task_context::{
    CacheInfo, Context as ContextTrait,
    DefaultTaskContextProvider as Implementation, TaskContext,
    TaskContextProvider, TaskContextProviderError, TaskHashProvider,
};

use crate::TaskExecutionResult;

pub struct DefaultTaskContextProvider<'a, TSys: ContextSys> {
    inner: Implementation<
        'a,
        OverallResultsTashHashProvider<'a>,
        ContextWrapper<'a, TSys>,
    >,
}

impl<'a, TSys: ContextSys> DefaultTaskContextProvider<'a, TSys> {
    #[inline(always)]
    pub fn new(
        context: &'a LoadedContext<TSys>,
        overall_results: &'a UnorderedMap<String, TaskExecutionResult>,
    ) -> Self {
        Self {
            inner: Implementation::new(
                OverallResultsTashHashProvider { overall_results },
                ContextWrapper { context },
            ),
        }
    }
}

impl<'a, TSys: ContextSys> TaskContextProvider<'a>
    for DefaultTaskContextProvider<'a, TSys>
{
    type Error = TaskContextProviderError;

    fn get_task_context(
        &'a self,
        node: &'a TaskExecutionNode,
        ignore_dependencies: bool,
    ) -> Result<TaskContext<'a>, Self::Error> {
        self.inner.get_task_context(node, ignore_dependencies)
    }
}

struct ContextWrapper<'a, TSys: ContextSys> {
    context: &'a LoadedContext<TSys>,
}

impl<'a, TSys: ContextSys> ContextTrait for ContextWrapper<'a, TSys> {
    type Error = LoadedContextError;

    fn get_task_env_vars(
        &self,
        node: &TaskExecutionNode,
    ) -> Result<Option<Arc<EnvVarsMap>>, Self::Error> {
        self.context.get_task_env_vars(node)
    }

    fn get_cache_info(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&CacheInfo> {
        self.context.get_cache_info(project_name, task_name)
    }
}

struct OverallResultsTashHashProvider<'a> {
    overall_results: &'a UnorderedMap<String, TaskExecutionResult>,
}

impl<'a> TaskHashProvider for OverallResultsTashHashProvider<'a> {
    fn get_task_hash(&self, task_full_name: &str) -> Option<DefaultHash> {
        Some(
            self.overall_results
                .get(task_full_name)
                .map(|r| r.hash())??,
        )
    }
}
