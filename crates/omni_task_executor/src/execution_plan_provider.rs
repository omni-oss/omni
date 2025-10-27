use derive_new::new;
use omni_context::{ContextSys, LoadedContext, LoadedContextError};
use omni_core::BatchedExecutionPlan;
use omni_execution_plan::{
    Call, Context as ContextTrait, DefaultExecutionPlanProvider,
    ExecutionPlanProvider, ExecutionPlanProviderError, ScmAffectedFilter,
};
use omni_types::OmniPath;

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
        project_filters: &[&str],
        dir_filters: &[&str],
        meta_filter: Option<&str>,
        scm_affected_filter: Option<&ScmAffectedFilter>,
        ignore_deps: bool,
        with_dependents: bool,
    ) -> Result<BatchedExecutionPlan, Self::Error> {
        self.inner.get_execution_plan(
            call,
            project_filters,
            dir_filters,
            meta_filter,
            scm_affected_filter,
            ignore_deps,
            with_dependents,
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

    #[inline(always)]
    fn root_dir(&self) -> &std::path::Path {
        self.inner.root_dir()
    }

    #[inline(always)]
    fn get_cache_input_files(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> &[OmniPath] {
        self.inner
            .get_cache_info(project_name, task_name)
            .map(|c| &c.key_input_files[..])
            .unwrap_or(&[])
    }
}
