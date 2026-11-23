use std::{borrow::Cow, sync::Arc};

use derive_new::new;
use maps::{Map, UnorderedMap};
use omni_configurations::MetaConfiguration;
use omni_core::TaskExecutionNode;
use omni_hasher::impls::DefaultHash;
use omni_task_output_logs::OutputLogsConfiguration;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{
    CacheInfo, Context, EnvVars, TaskContext, TaskContextProvider,
    TaskHashProvider, task_context::evaluate_enabled,
};

/// Task inputs that can be prepared before any dependency has run.
pub struct PreparedTaskContext<'a> {
    node: &'a TaskExecutionNode,
    env_vars: Arc<EnvVars>,
    cache_info: Option<Cow<'a, CacheInfo>>,
    output_logs: Option<OutputLogsConfiguration>,
    template_context: omni_tera::Context,
}

impl<'a> PreparedTaskContext<'a> {
    /// Evaluate the condition with the same inputs that execution will use.
    pub fn is_enabled(&self) -> omni_tera::Result<bool> {
        evaluate_enabled(self.node, &self.template_context)
    }

    /// Add runtime dependency hashes without reloading the prepared inputs.
    pub fn finish(
        self,
        hash_provider: &impl TaskHashProvider,
        ignore_dependencies: bool,
    ) -> TaskContext<'a> {
        let dependency_hashes =
            resolve_hashes(self.node, hash_provider, ignore_dependencies);
        self.into_context(dependency_hashes)
    }

    fn into_context(
        self,
        dependency_hashes: Vec<DefaultHash>,
    ) -> TaskContext<'a> {
        TaskContext {
            node: self.node,
            env_vars: self.env_vars,
            cache_info: self.cache_info,
            output_logs: self.output_logs,
            dependency_hashes,
            template_context: self.template_context,
        }
    }
}

fn resolve_hashes(
    node: &TaskExecutionNode,
    hash_provider: &impl TaskHashProvider,
    ignore_dependencies: bool,
) -> Vec<DefaultHash> {
    if ignore_dependencies {
        return vec![];
    }
    node.dependencies()
        .iter()
        .flat_map(|dep| hash_provider.get_dependency_hashes(dep))
        .collect()
}

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
        let (prepared, hashes) = prepare_with(&self.context, node, || {
            resolve_hashes(node, &self.hash_provider, ignore_dependencies)
        })?;
        Ok(prepared.into_context(hashes))
    }
}

/// Prepare the task's environment and template inputs without resolving hashes.
/// The context implementation may run command substitutions while loading env.
pub fn prepare_task_context<'a>(
    context: &'a impl Context,
    node: &'a TaskExecutionNode,
) -> Result<PreparedTaskContext<'a>, TaskContextProviderError> {
    let (prepared, ()) = prepare_with(context, node, || ())?;
    Ok(prepared)
}

fn prepare_with<'a, T>(
    context: &'a impl Context,
    node: &'a TaskExecutionNode,
    before_template: impl FnOnce() -> T,
) -> Result<(PreparedTaskContext<'a>, T), TaskContextProviderError> {
    let env_vars = context
        .get_task_env_vars(node)
        .map_err(|e| TaskContextProviderErrorInner::ContextError(e.into()))?
        .ok_or_else(|| TaskContextProviderErrorInner::NoEnvVarsForTask {
            full_task_name: node.full_task_name().to_string(),
        })?;

    let meta = context
        .get_task_meta_config(node.project_name(), node.task_name())
        .or_else(|| context.get_project_meta_config(node.project_name()));

    let cache_info = context
        .get_cache_info(node.project_name(), node.task_name())
        .map(Cow::Borrowed);

    let output_logs = context
        .get_output_logs(node.project_name(), node.task_name())
        .copied();

    // The default provider historically resolves hashes before asking the
    // context for override args. Public implementations can share state.
    let result = before_template();
    let template_context = create_template_context(
        &env_vars,
        meta,
        cache_info.as_ref().map(|c| &c.args),
        context.get_task_override_args(node.project_name(), node.task_name()),
    );

    Ok((
        PreparedTaskContext {
            node,
            env_vars,
            cache_info,
            output_logs,
            template_context,
        },
        result,
    ))
}

fn create_template_context<'a>(
    env_vars: &'a Map<String, String>,
    meta: Option<&MetaConfiguration>,
    args: Option<&'a Map<String, serde_json::Value>>,
    override_args: Option<&'a UnorderedMap<String, serde_json::Value>>,
) -> omni_tera::Context {
    let mut context = omni_tera::context::STANDARD.clone();

    context.insert("env", env_vars);
    if let Some(meta) = meta {
        context.insert("meta", meta);
    } else {
        context.insert("meta", &MetaConfiguration::default());
    }

    if let Some(args) = args {
        if let Some(override_args) = override_args
            && !override_args.is_empty()
        {
            let mut merged = args.clone();

            for (k, v) in override_args {
                merged.insert(k.to_owned(), v.to_owned());
            }

            context.insert("args", &merged);
        } else {
            context.insert("args", args);
        }
    } else {
        context.insert("args", override_args.unwrap_or(&Default::default()));
    }

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
mod preparation_tests {
    use std::{cell::Cell, convert::Infallible, rc::Rc};

    use super::*;
    use omni_config_types::TeraExprBoolean;

    #[derive(Default)]
    struct Inputs {
        env_reads: Cell<usize>,
        args: UnorderedMap<String, serde_json::Value>,
        hashes_before_args: Option<Rc<Cell<usize>>>,
    }

    impl Context for Inputs {
        type Error = Infallible;

        fn get_task_env_vars(
            &self,
            _: &TaskExecutionNode,
        ) -> Result<Option<Arc<EnvVars>>, Self::Error> {
            self.env_reads.set(self.env_reads.get() + 1);
            Ok(Some(Arc::new(maps::map![
                "READY".into() => "yes".into()
            ])))
        }

        fn get_task_meta_config(
            &self,
            _: &str,
            _: &str,
        ) -> Option<&MetaConfiguration> {
            None
        }

        fn get_project_meta_config(
            &self,
            _: &str,
        ) -> Option<&MetaConfiguration> {
            None
        }

        fn get_cache_info(&self, _: &str, _: &str) -> Option<&CacheInfo> {
            None
        }

        fn get_output_logs(
            &self,
            _: &str,
            _: &str,
        ) -> Option<&OutputLogsConfiguration> {
            None
        }

        fn get_task_override_args(
            &self,
            _: &str,
            _: &str,
        ) -> Option<&UnorderedMap<String, serde_json::Value>> {
            if let Some(hashes) = &self.hashes_before_args {
                assert_eq!(hashes.get(), 1);
            }
            Some(&self.args)
        }
    }

    #[derive(Default)]
    struct Hashes(Rc<Cell<usize>>);

    impl TaskHashProvider for Hashes {
        fn get_task_hash(&self, name: &str) -> Option<DefaultHash> {
            assert_eq!(name, "app#build");
            self.0.set(self.0.get() + 1);
            Some(DefaultHash::default())
        }
    }

    #[test]
    fn default_provider_requests_hashes_before_override_args() {
        let node = TaskExecutionNode::new(
            "check",
            None,
            None,
            "app",
            ".",
            vec!["app#build".into()],
            TeraExprBoolean::Boolean(true),
            false,
            false,
            Default::default(),
        );
        let hashes = Hashes::default();
        let inputs = Inputs {
            hashes_before_args: Some(Rc::clone(&hashes.0)),
            ..Default::default()
        };
        let provider = DefaultTaskContextProvider::new(hashes, inputs);
        let context = provider.get_task_context(&node, false).unwrap();
        assert_eq!(context.dependency_hashes.len(), 1);
    }

    #[test]
    fn prepare_reads_env_once_and_finish_adds_only_runtime_hashes() {
        let node = TaskExecutionNode::new(
            "check",
            None,
            None,
            "app",
            ".",
            vec!["app#build".into()],
            serde_json::from_value::<TeraExprBoolean>(serde_json::json!(
                "{{ env.READY == 'yes' and args.run }}"
            ))
            .unwrap(),
            false,
            false,
            Default::default(),
        );
        for ignore in [false, true] {
            let inputs = Inputs {
                args: maps::unordered_map!["run".into() => true.into()],
                ..Default::default()
            };
            let hashes = Hashes::default();
            let prepared = prepare_task_context(&inputs, &node).unwrap();
            assert!(prepared.is_enabled().unwrap());
            assert_eq!(inputs.env_reads.get(), 1);
            assert_eq!(hashes.0.get(), 0);

            let context = prepared.finish(&hashes, ignore);
            assert_eq!(context.env_vars["READY"], "yes");
            assert!(context.is_enabled().unwrap());
            assert_eq!(inputs.env_reads.get(), 1);
            assert_eq!(hashes.0.get(), usize::from(!ignore));
            assert_eq!(context.dependency_hashes.len(), usize::from(!ignore));
        }
    }
}
