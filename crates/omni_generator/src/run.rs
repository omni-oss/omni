use std::{borrow::Cow, path::Path};

use maps::{Map, UnorderedMap, unordered_map};
use omni_generator_configurations::{
    GeneratorConfiguration, OmniPath, OverwriteConfiguration,
};
use omni_input_provider::{CollectionConfig, InputProvider, collect};
use omni_messages::{
    GeneratorCompletedEvent, GeneratorEventSubscriber, GeneratorStartEvent,
    NoopSubscriber,
};
use sets::UnorderedSet;
use value_bag::{OwnedValueBag, ValueBag};

use crate::{
    GeneratorSys, GeneratorSysFull, JsScriptRunner, LazyScriptRunner,
    error::{Error, ErrorInner},
    execute_actions::{ExecuteActionsArgs, execute_actions},
    gen_session::GenSession,
    sys_impl::TransactionSys,
    utils::{expand_json_value, get_tera_context},
};

#[derive(Debug)]
pub struct RunConfig<'a, S: GeneratorEventSubscriber = NoopSubscriber> {
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
    pub use_input_defaults: bool,
    pub available_generators: &'a [Cow<'a, GeneratorConfiguration>],
    pub input_provider: &'a dyn InputProvider,
    pub subscriber: &'a S,
}

pub struct GeneratorRunResult {
    pub session: GenSession,
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
    r#gen: &GeneratorConfiguration,
    config: &RunConfig<'a, S>,
    sys: &impl GeneratorSys,
) -> Result<GeneratorRunResult, Error> {
    let tx = TransactionSys::new(sys.clone());
    let runner = LazyScriptRunner::new(
        tx.clone(),
        config.workspace_dir.to_path_buf(),
        env!("CARGO_PKG_VERSION").to_string(),
    );
    config
        .subscriber
        .on_generator_start(GeneratorStartEvent {
            name: r#gen.name.clone(),
        })
        .await;

    let result = run_internal(r#gen, config, &tx, &runner).await;

    // Tear down the JS process (if one was started) regardless of outcome.
    runner.shutdown().await;

    let session = result?;

    if !config.dry_run {
        tx.commit().await?;
    }

    config
        .subscriber
        .on_generator_completed(GeneratorCompletedEvent {
            name: r#gen.name.clone(),
        })
        .await;

    Ok(GeneratorRunResult { session })
}

pub(crate) async fn run_internal<'a, S: GeneratorEventSubscriber>(
    r#gen: &GeneratorConfiguration,
    config: &RunConfig<'a, S>,
    sys: &impl GeneratorSysFull,
    runner: &dyn JsScriptRunner,
) -> Result<GenSession, Error> {
    let collection_config = CollectionConfig {
        use_defaults: config.use_input_defaults,
        ..CollectionConfig::default()
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
    };

    execute_actions(&args, &session, sys).await?;

    let skip = r#gen
        .inputs
        .iter()
        .filter_map(|p| {
            if p.extra().remember {
                None
            } else {
                Some(p.name())
            }
        })
        .collect::<UnorderedSet<_>>();

    session.set_inputs(
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
    )?;

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
