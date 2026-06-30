use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use maps::{Map, UnorderedMap, unordered_map};
use omni_generator_configurations::{
    Generator, GeneratorConfiguration, OmniPath, OverwriteConfiguration,
};
use omni_input_provider::{
    InputProfile, InputProvider, ValidationConfig, collect,
};
use omni_messages::{
    GeneratorCompletedEvent, GeneratorEventSubscriber, GeneratorStartEvent,
    NoopSubscriber,
};
use serde::{Deserialize, Serialize};
use sets::UnorderedSet;
use strum::EnumDiscriminants;
use value_bag::{OwnedValueBag, ValueBag};

use omni_utils::lock::LockGuard;

use crate::{
    GeneratorSys, GeneratorSysFull, JsScriptRunner, LazyScriptRunner,
    error::{Error, ErrorInner},
    execute_actions::{ExecuteActionsArgs, execute_actions},
    gen_session::GenSession,
    sys_impl::{self, PendingActionsVisitor, TransactionSys},
    utils::{expand_json_value, get_tera_context},
};

/// Default cap on `run-generator` nesting depth. The static `detect_recursion`
/// pass is the primary cycle guard; this bound is a defense-in-depth backstop
/// for runtime edges the static graph cannot model. Real nesting is only a few
/// levels deep, so the default is generous. Callers can override it via
/// [`RunConfig::max_depth`] when a config legitimately nests deeper.
pub const DEFAULT_MAX_GENERATOR_DEPTH: usize = 64;

#[derive(Debug, bon::Builder)]
pub struct RunConfig<'a, S: GeneratorEventSubscriber = NoopSubscriber> {
    #[builder(default)]
    pub dry_run: bool,
    pub output_dir: &'a Path,
    pub overwrite: Option<OverwriteConfiguration>,
    pub workspace_dir: &'a Path,
    pub current_dir: &'a Path,
    pub target_overrides: &'a UnorderedMap<String, OmniPath>,
    pub input_values: &'a UnorderedMap<String, OwnedValueBag>,
    pub context_values: &'a UnorderedMap<String, OwnedValueBag>,
    pub env: &'a Map<String, String>,
    pub args: Option<&'a UnorderedMap<String, serde_json::Value>>,
    #[builder(default)]
    pub use_input_defaults: bool,
    pub available_generators: &'a [Cow<'a, GeneratorConfiguration>],
    pub input_provider: &'a dyn InputProvider<Generator>,
    pub subscriber: &'a S,
    /// Maximum `run-generator` nesting depth before a run is aborted. Use
    /// [`DEFAULT_MAX_GENERATOR_DEPTH`] unless a config legitimately nests deeper.
    #[builder(default = DEFAULT_MAX_GENERATOR_DEPTH)]
    pub max_depth: usize,
}

pub struct GeneratorRunResult {
    pub session: GenSession,
    pub actions: Vec<Action>,
}

pub async fn run_named<'a, S: GeneratorEventSubscriber>(
    generator_name: &'a str,
    config: &RunConfig<'a, S>,
    sys: &impl GeneratorSys,
) -> Result<GeneratorRunResult, Error> {
    crate::validate(config.available_generators)?;

    let generator = config
        .available_generators
        .iter()
        .find(|g| g.name == generator_name)
        .ok_or_else(|| {
            ErrorInner::new_generator_not_found(generator_name.to_string())
        })?;

    run_in_transaction(&generator, config, sys).await
}

pub async fn run<'a, S: GeneratorEventSubscriber>(
    generator: &'a GeneratorConfiguration,
    config: &RunConfig<'a, S>,
    sys: &impl GeneratorSys,
) -> Result<GeneratorRunResult, Error> {
    crate::validate(config.available_generators)?;
    run_in_transaction(generator, config, sys).await
}

/// Runs a generator against a transactional overlay of `sys`.
///
/// Every file-system mutation performed while executing the generator's
/// actions is buffered in the overlay; nothing touches `sys` until the
/// transaction is committed. A normal run commits the transaction once all
/// actions have succeeded (making generation atomic), while a dry run simply
/// drops the buffered actions without committing.
async fn run_in_transaction<'a, S: GeneratorEventSubscriber>(
    generator: &GeneratorConfiguration,
    config: &RunConfig<'a, S>,
    sys: &impl GeneratorSys,
) -> Result<GeneratorRunResult, Error> {
    if !generator.user_invocable {
        return Err(ErrorInner::new_generator_not_invocable(
            generator.name.to_string(),
        )
        .into());
    }

    crate::detect_recursion(generator, config.available_generators)?;

    // Serialize all generator runs within the same workspace. Generators can
    // write to arbitrary workspace-level paths (not just output_dir), so
    // per-directory locking is insufficient — a single workspace-scoped lock
    // is the only safe granularity.
    let lock_path = config
        .workspace_dir
        .join(".omni")
        .join("locks")
        .join("generator.lock");
    let _lock = LockGuard::acquire_exclusive(lock_path).await?;

    let tx = TransactionSys::new(sys.clone());
    let runner = LazyScriptRunner::new(
        tx.clone(),
        config.workspace_dir.to_path_buf(),
        env!("CARGO_PKG_VERSION").to_string(),
    );
    config
        .subscriber
        .on_generator_start(GeneratorStartEvent {
            name: generator.name.clone(),
        })
        .await;

    let result = run_internal(generator, config, &tx, &runner, 0).await;

    // Tear down the JS process (if one was started) regardless of outcome.
    runner.shutdown().await;

    let session = result?;

    let actions = {
        struct InferActionsVisitor {
            actions: Vec<Action>,
        }
        impl<TSys: GeneratorSys> PendingActionsVisitor<TSys> for InferActionsVisitor {
            type Error = eyre::Report;

            async fn visit_action(
                &mut self,
                action: &sys_impl::Action,
                sys: &TSys,
            ) -> Result<(), Self::Error> {
                if let Some(a) = Action::infer_from(action, sys).await? {
                    self.actions.push(a);
                }
                Ok(())
            }
        }

        let mut visitor = InferActionsVisitor {
            actions: Vec::new(),
        };
        tx.visit_pending_actions(&mut visitor).await?;
        visitor.actions
    };

    if !config.dry_run {
        tx.commit().await?;
    }

    config
        .subscriber
        .on_generator_completed(GeneratorCompletedEvent {
            name: generator.name.clone(),
        })
        .await;

    Ok(GeneratorRunResult { session, actions })
}

pub(crate) async fn run_internal<'a, S: GeneratorEventSubscriber>(
    r#gen: &GeneratorConfiguration,
    config: &RunConfig<'a, S>,
    sys: &impl GeneratorSysFull,
    runner: &dyn JsScriptRunner,
    depth: usize,
) -> Result<GenSession, Error> {
    if depth > config.max_depth {
        return Err(ErrorInner::new_max_generator_depth_exceeded(
            r#gen.name.clone(),
            config.max_depth,
        )
        .into());
    }

    let collection_config = ValidationConfig {
        use_defaults: config.use_input_defaults,
        ..ValidationConfig::default()
    };

    let session = GenSession::with_restored(
        r#gen.name.as_str(),
        config.target_overrides.clone(),
        Default::default(),
    );

    let mut values = collect(
        &r#gen.inputs,
        &config.input_values,
        &config.context_values,
        &collection_config,
        config.input_provider,
    )
    .await?;

    // propagate prompt values to the context values
    for (key, value) in config.input_values.iter() {
        if !values.contains_key(key) {
            values.insert(key.to_string(), value.clone());
        }
    }

    trace::trace!(?values, "input_values");

    let mut context_values = config.context_values.clone();

    context_values.insert(
        "inputs".to_string(),
        ValueBag::capture_serde1(&values).to_owned(),
    );

    let vars = expand_vars("vars", &r#gen.vars, &context_values)?;

    context_values.insert(
        "vars".to_string(),
        ValueBag::capture_serde1(&vars).to_owned(),
    );

    if let Some(args) = config.args {
        let args = expand_vars("args", args, &context_values)?;
        context_values.insert(
            "args".to_string(),
            ValueBag::capture_serde1(&args).to_owned(),
        );
    } else {
        let map = UnorderedMap::<&str, ()>::default();
        context_values.insert(
            "args".to_string(),
            ValueBag::capture_serde1(&map).to_owned(),
        );
    }

    let args = ExecuteActionsArgs {
        actions: &r#gen.actions,
        context_values: &context_values,
        dry_run: config.dry_run,
        output_dir: config.output_dir,
        generator_dir: &r#gen
            .config_path
            .parent()
            .expect("generator should have a directory"),
        generator_name: &r#gen.name,
        scope_id: r#gen.scope_id.as_deref(),
        targets: &r#gen.targets,
        overwrite: config.overwrite,
        workspace_dir: config.workspace_dir,
        available_generators: config.available_generators,
        target_overrides: config.target_overrides,
        current_dir: config.current_dir,
        env: config.env,
        js_script_runner: runner,
        input_provider: config.input_provider,
        subscriber: config.subscriber,
        use_input_defaults: config.use_input_defaults,
        depth,
        max_depth: config.max_depth,
    };

    execute_actions(&args, &session, sys).await?;

    let skip = r#gen
        .inputs
        .iter()
        .filter_map(|p| {
            if Generator::is_remember(p) {
                None
            } else {
                Some(p.base().name.as_str())
            }
        })
        .collect::<UnorderedSet<_>>();

    session
        .set_inputs(
            r#gen.name.as_str(),
            values
                .into_iter()
                .filter_map(|(k, v)| {
                    if skip.contains(k.as_str()) {
                        None
                    } else {
                        Some((k, v))
                    }
                })
                .collect(),
        )
        .await?;

    Ok(session)
}

fn expand_vars(
    key: &str,
    values: &UnorderedMap<String, serde_json::Value>,
    context_values: &UnorderedMap<String, OwnedValueBag>,
) -> Result<UnorderedMap<String, OwnedValueBag>, Error> {
    let tera_ctx = get_tera_context(context_values);
    let mut result = unordered_map!();
    let parent_key = Some(key);

    for (key, value) in values.iter() {
        let value = expand_json_value(&tera_ctx, parent_key, key, value)?;
        result.insert(
            key.to_string(),
            ValueBag::capture_serde1(value.as_ref()).to_owned(),
        );
    }

    Ok(result)
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
    EnumDiscriminants,
)]
#[serde(tag = "action", rename_all = "kebab-case")]
#[strum_discriminants(name(ActionType))]
pub enum Action {
    CreateFile { path: PathBuf },
    ModifyFile { path: PathBuf },
    RemoveFile { path: PathBuf },
    CreateDir { path: PathBuf },
    RemoveDir { path: PathBuf },
    RemoveDirAll { path: PathBuf },
    Rename { from: PathBuf, to: PathBuf },
    Copy { from: PathBuf, to: PathBuf },
}

impl Action {
    pub async fn infer_from<'a, TSys: GeneratorSys>(
        action: &'a sys_impl::Action,
        sys: &'a TSys,
    ) -> eyre::Result<Option<Self>> {
        Ok(match action {
            sys_impl::Action::Write { path, .. } => {
                let exists = sys.fs_exists_no_err_async(path).await;
                if exists {
                    Some(Self::ModifyFile { path: path.clone() })
                } else {
                    Some(Self::CreateFile { path: path.clone() })
                }
            }
            sys_impl::Action::Append { path, .. } => {
                let exists = sys.fs_exists_no_err_async(path).await;
                if exists {
                    Some(Self::ModifyFile { path: path.clone() })
                } else {
                    Some(Self::CreateFile { path: path.clone() })
                }
            }
            sys_impl::Action::CreateDir { path, .. } => {
                Some(Self::CreateDir { path: path.clone() })
            }
            sys_impl::Action::RemoveFile { path } => {
                Some(Self::RemoveFile { path: path.clone() })
            }
            sys_impl::Action::RemoveDir { path } => {
                Some(Self::RemoveDir { path: path.clone() })
            }
            sys_impl::Action::RemoveDirAll { path } => {
                Some(Self::RemoveDirAll { path: path.clone() })
            }
            sys_impl::Action::Rename { from, to } => Some(Self::Rename {
                from: from.clone(),
                to: to.clone(),
            }),
            sys_impl::Action::Copy { from, to } => Some(Self::Copy {
                from: from.clone(),
                to: to.clone(),
            }),
            sys_impl::Action::SetCurrentDir { .. } => None,
        })
    }
}
