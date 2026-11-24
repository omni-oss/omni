use std::sync::Arc;

use maps::UnorderedMap;
use omni_context::{ContextSys, EnvVarsMap, LoadedContext, LoadedContextError};
use omni_core::TaskExecutionNode;
use omni_hasher::impls::DefaultHash;
use omni_task_context::{
    CacheInfo, Context as ContextTrait,
    DefaultTaskContextProvider as Implementation, DependencyHash, TaskContext,
    TaskContextProvider, TaskContextProviderError, TaskHashProvider,
    collect_dependency_hashes,
};

use crate::{SkipReason, TaskExecutionResult};

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
        override_args: Option<&'a UnorderedMap<String, serde_json::Value>>,
    ) -> Self {
        Self {
            inner: Implementation::new(
                OverallResultsTashHashProvider { overall_results },
                ContextWrapper {
                    context,
                    override_args,
                },
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
    override_args: Option<&'a UnorderedMap<String, serde_json::Value>>,
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

    fn get_output_logs(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&omni_task_output_logs::OutputLogsConfiguration> {
        self.context.get_output_logs(project_name, task_name)
    }

    fn get_task_meta_config(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&omni_configurations::MetaConfiguration> {
        self.context.get_task_meta_config(project_name, task_name)
    }

    fn get_project_meta_config(
        &self,
        project_name: &str,
    ) -> Option<&omni_configurations::MetaConfiguration> {
        self.context.get_project_meta_config(project_name)
    }

    fn get_task_override_args(
        &self,
        _project_name: &str,
        _task_name: &str,
    ) -> Option<&UnorderedMap<String, serde_json::Value>> {
        self.override_args
    }
}

struct OverallResultsTashHashProvider<'a> {
    overall_results: &'a UnorderedMap<String, TaskExecutionResult>,
}

impl<'a> TaskHashProvider for OverallResultsTashHashProvider<'a> {
    fn get_task_hash(&self, task_full_name: &str) -> Option<DefaultHash> {
        let result = self.overall_results.get(task_full_name)?;
        // An uncached task's hash is the zero sentinel, which proves nothing
        // about freshness, so it never becomes a dependency digest.
        if result
            .cache_status()
            .is_some_and(|s| !s.provides_reusable_hash())
        {
            return None;
        }
        result.hash()
    }

    fn get_dependency_hashes(&self, task_full_name: &str) -> Vec<DefaultHash> {
        collect_dependency_hashes(task_full_name, |name| {
            match self.overall_results.get(name) {
                Some(TaskExecutionResult::Skipped {
                    skip_reason: SkipReason::NoCommand | SkipReason::Disabled,
                    task,
                    ..
                }) => DependencyHash::Forward(task.dependencies()),
                _ => DependencyHash::Direct(self.get_task_hash(name)),
            }
        })
    }
}

#[cfg(all(test, unix))]
mod timing_tests {
    use std::fs;

    use omni_context::Context;
    use omni_execution_plan::Call;
    use omni_messages::NoopSubscriber;
    use omni_task_context::prepare_task_context;
    use system_traits::impls::RealSys;

    use super::ContextWrapper;
    use crate::{ExecutionConfigBuilder, TaskExecutor};

    #[tokio::test]
    async fn eager_environment_preparation_can_change_a_task_condition() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let project = root.join("app");
        fs::create_dir(&project).unwrap();
        fs::write(root.join("workspace.omni.yaml"), "projects: ['app/**']\n")
            .unwrap();
        fs::write(
            project.join("project.omni.yaml"),
            r#"
name: app
tasks:
  produce:
    exec: [sh, -c, 'printf yes > ready']
    cache: { enabled: false }
  consume:
    dependencies: [produce]
    exec: [sh, -c, 'printf ran > consumed']
    enabled: "{% if env.READY == 'yes' %}true{% else %}false{% endif %}"
    cache: { enabled: true }
    env:
      vars:
        READY: "$(sh probe.sh)"
"#,
        )
        .unwrap();
        fs::write(
            project.join("probe.sh"),
            "printf 'called\\n' >> calls\nif test -f ready; then cat ready; else printf no; fi\n",
        )
        .unwrap();

        let loaded = Context::new(
            RealSys,
            "test",
            root,
            false,
            "workspace.omni.yaml",
            Some(vec![]),
            &Default::default(),
        )
        .unwrap()
        .into_loaded()
        .await
        .unwrap();
        let graph = loaded
            .get_project_graph()
            .unwrap()
            .get_task_execution_graph()
            .unwrap();
        let consumer = graph.get_task_by_name("app", "consume").unwrap();
        let wrapper = ContextWrapper {
            context: &loaded,
            override_args: None,
        };
        assert!(!project.join("calls").exists());
        let early = prepare_task_context(&wrapper, consumer).unwrap();
        assert!(!early.is_enabled().unwrap());
        assert_eq!(
            fs::read_to_string(project.join("calls")).unwrap(),
            "called\n"
        );

        let config = ExecutionConfigBuilder::default()
            .call(Call::pattern_tasks(["consume"]))
            .build()
            .unwrap();
        let results = TaskExecutor::new(config, &loaded, NoopSubscriber)
            .run()
            .await
            .unwrap();
        assert!(
            project.join("consumed").is_file(),
            "ready: {:?}; calls: {:?}; task results: {results:?}",
            fs::read_to_string(project.join("ready")),
            fs::read_to_string(project.join("calls")),
        );
        assert_eq!(
            fs::read_to_string(project.join("consumed")).unwrap(),
            "ran"
        );
        assert_eq!(
            fs::read_to_string(project.join("calls")).unwrap(),
            "called\ncalled\n"
        );
    }
}
