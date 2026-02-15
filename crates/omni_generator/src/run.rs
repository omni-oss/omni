use std::path::Path;

use derive_new::new;
use maps::{Map, UnorderedMap, unordered_map};
use omni_generator_configurations::{
    GeneratorConfiguration, OmniPath, OverwriteConfiguration,
};
use omni_prompt::configuration::PromptingConfiguration;
use sets::UnorderedSet;
use value_bag::{OwnedValueBag, ValueBag};

use crate::{
    GeneratorSys,
    error::{Error, ErrorInner},
    execute_actions::{ExecuteActionsArgs, execute_actions},
    gen_session::GenSession,
    sys_impl::DryRunSys,
    utils::{expand_json_value, get_tera_context},
};

#[derive(Debug, new)]
pub struct RunConfig<'a> {
    pub dry_run: bool,
    pub output_dir: &'a Path,
    pub overwrite: Option<OverwriteConfiguration>,
    pub workspace_dir: &'a Path,
    pub current_dir: &'a Path,
    pub target_overrides: &'a UnorderedMap<String, OmniPath>,
    pub prompt_values: &'a UnorderedMap<String, OwnedValueBag>,
    pub context_values: &'a UnorderedMap<String, OwnedValueBag>,
    pub env: &'a Map<String, String>,
    pub args: Option<&'a UnorderedMap<String, serde_json::Value>>,
}

pub async fn run<'a>(
    generator_name: &'a str,
    generators: &'a [GeneratorConfiguration],
    config: &RunConfig<'a>,
    sys: &impl GeneratorSys,
) -> Result<GenSession, Error> {
    crate::validate(generators)?;

    let generator = generators
        .iter()
        .find(|g| g.name == generator_name)
        .ok_or_else(|| {
            ErrorInner::new_generator_not_found(generator_name.to_string())
        })?;

    if config.dry_run {
        let sys = DryRunSys::default();
        run_internal(&generator, generators, config, &sys).await
    } else {
        run_internal(&generator, generators, config, sys).await
    }
}

pub(crate) async fn run_internal<'a>(
    r#gen: &GeneratorConfiguration,
    available_generators: &[GeneratorConfiguration],
    config: &RunConfig<'a>,
    sys: &impl GeneratorSys,
) -> Result<GenSession, Error> {
    let prompting_config = PromptingConfiguration::default();

    let session = GenSession::with_restored(
        r#gen.name.as_str(),
        config.target_overrides.clone(),
        Default::default(),
    );

    let mut values = omni_prompt::prompt(
        &r#gen.prompts,
        &config.prompt_values,
        &config.context_values,
        &prompting_config,
    )?;

    // propagate prompt values to the context values
    for (key, value) in config.prompt_values.iter() {
        if !values.contains_key(key) {
            values.insert(key.to_string(), value.clone());
        }
    }

    trace::trace!(?values, "prompt_values");

    let mut context_values = config.context_values.clone();

    context_values.insert(
        "prompts".to_string(),
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
            .file
            .parent()
            .expect("generator should have a directory"),
        generator_name: &r#gen.name,
        targets: &r#gen.targets,
        overwrite: config.overwrite,
        workspace_dir: config.workspace_dir,
        available_generators,
        target_overrides: config.target_overrides,
        current_dir: config.current_dir,
        env: config.env,
    };

    execute_actions(&args, &session, sys).await?;

    let skip = r#gen
        .prompts
        .iter()
        .filter_map(|p| {
            if p.extra().remember {
                None
            } else {
                Some(p.name())
            }
        })
        .collect::<UnorderedSet<_>>();

    session.set_prompts(
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
    );

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
