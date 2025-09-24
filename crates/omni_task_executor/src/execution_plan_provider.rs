use derive_new::new;
use omni_context::{ContextSys, LoadedContext, LoadedContextError};
use omni_execution_plan::{
    Call, Context as ContextTrait, DefaultExecutionPlanProvider,
    ExecutionPlanProvider, ExecutionPlanProviderError,
};

#[derive(Debug)]
pub struct ContextExecutionPlanProvider<'a, TSys: ContextSys> {
    inner: DefaultExecutionPlanProvider<'a, ContextWrapper<'a, TSys>>,
}

impl<'a, TSys: ContextSys> ContextExecutionPlanProvider<'a, TSys> {
    pub fn new(context: &'a LoadedContext<TSys>) -> Self {
        Self {
            inner: DefaultExecutionPlanProvider::new(ContextWrapper {
                inner: context,
            }),
        }
    }
}

impl<'a, TSys: ContextSys> ExecutionPlanProvider
    for ContextExecutionPlanProvider<'a, TSys>
{
    type Error = ExecutionPlanProviderError;

    #[inline(always)]
    fn get_execution_plan(
        &self,
        call: &Call,
        project_filter: Option<&str>,
        meta_filter: Option<&str>,
        ignore_deps: bool,
    ) -> Result<Vec<Vec<omni_core::TaskExecutionNode>>, Self::Error> {
        self.inner.get_execution_plan(
            call,
            project_filter,
            meta_filter,
            ignore_deps,
        )
    }
}

#[derive(Debug, new)]
struct ContextWrapper<'a, TSys: ContextSys> {
    inner: &'a LoadedContext<TSys>,
}

impl<'a, TSys: ContextSys> ContextTrait for ContextWrapper<'a, TSys> {
    type Error = LoadedContextError;

    #[inline(always)]
    fn get_project_meta_config(
        &self,
        project_name: &str,
    ) -> Option<&omni_configurations::MetaConfiguration> {
        self.inner.get_project_meta_config(project_name)
    }

    #[inline(always)]
    fn get_task_meta_config(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&omni_configurations::MetaConfiguration> {
        self.inner.get_task_meta_config(project_name, task_name)
    }

    #[inline(always)]
    fn get_project_graph(
        &self,
    ) -> Result<omni_core::ProjectGraph, Self::Error> {
        self.inner.get_project_graph()
    }

    #[inline(always)]
    fn projects(&self) -> &[omni_core::Project] {
        self.inner.projects()
    }
}
