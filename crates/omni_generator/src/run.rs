use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use derive_new::new;
use maps::{UnorderedMap, unordered_map};
use omni_generator_configurations::{
    GeneratorConfiguration, OverwriteConfiguration,
};
use omni_prompt::configuration::{
    BasePromptConfiguration, OptionConfiguration, PromptConfiguration,
    PromptingConfiguration, SelectPromptConfiguration,
};
use value_bag::{OwnedValueBag, ValueBag};

use crate::{
    GeneratorSys,
    error::{Error, ErrorInner},
    execute_actions::{ExecuteActionsArgs, execute_actions},
    sys_impl::DryRunSys,
    utils::get_tera_context,
};

#[derive(Debug, new)]
pub struct RunConfig<'a> {
    pub dry_run: bool,
    pub output_dir: &'a Path,
    pub overwrite: Option<OverwriteConfiguration>,
}

pub async fn run<'a>(
    generator_name: Option<&'a str>,
    root_dir: &'a Path,
    generator_patterns: &'a [String],
    target_overrides: &UnorderedMap<String, PathBuf>,
    prompt_values: &UnorderedMap<String, OwnedValueBag>,
    context_values: &UnorderedMap<String, OwnedValueBag>,
    config: &RunConfig<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    let generators = crate::discover(root_dir, generator_patterns, sys).await?;

    crate::validate(&generators)?;

    let generator_name = if let Some(name) = generator_name.clone() {
        Cow::Borrowed(name)
    } else {
        Cow::Owned(prompt_generator_name(&generators)?)
    };

    let generator = generators
        .iter()
        .find(|g| g.name == generator_name)
        .ok_or_else(|| {
            ErrorInner::new_generator_not_found(generator_name.to_string())
        })?;

    if config.dry_run {
        let sys = DryRunSys::default();
        run_internal(
            &generator,
            &generators,
            target_overrides,
            prompt_values,
            context_values,
            config,
            &sys,
        )
        .await?;
    } else {
        run_internal(
            &generator,
            &generators,
            target_overrides,
            prompt_values,
            context_values,
            config,
            sys,
        )
        .await?;
    }

    Ok(())
}

pub(crate) async fn run_internal<'a>(
    r#gen: &GeneratorConfiguration,
    available_generators: &[GeneratorConfiguration],
    target_overrides: &UnorderedMap<String, PathBuf>,
    prompt_values: &UnorderedMap<String, OwnedValueBag>,
    context_values: &UnorderedMap<String, OwnedValueBag>,
    config: &RunConfig<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    let prompting_config = PromptingConfiguration::default();

    let mut values = omni_prompt::prompt(
        &r#gen.prompts,
        &prompt_values,
        context_values,
        &prompting_config,
    )?;

    // propagate prompt values to the context values
    for (key, value) in prompt_values.iter() {
        if !values.contains_key(key) {
            values.insert(key.to_string(), value.clone());
        }
    }

    trace::trace!("prompt values: {:#?}", values);

    let mut context_values = context_values.clone();

    context_values.insert(
        "prompts".to_string(),
        ValueBag::capture_serde1(&values).to_owned(),
    );

    let vars = expand_vars(&r#gen.vars, &context_values)?;

    context_values.insert(
        "vars".to_string(),
        ValueBag::capture_serde1(&vars).to_owned(),
    );

    let args = ExecuteActionsArgs {
        actions: &r#gen.actions,
        context_values: &context_values,
        dry_run: config.dry_run,
        output_dir: config.output_dir,
        generator_dir: &r#gen
            .file
            .parent()
            .expect("generator should have a directory"),
        targets: &r#gen.targets,
        overwrite: config.overwrite,
        available_generators,
        target_overrides,
    };

    execute_actions(&args, sys).await?;

    Ok(())
}

fn expand_vars(
    values: &UnorderedMap<String, serde_json::Value>,
    context_values: &UnorderedMap<String, OwnedValueBag>,
) -> Result<UnorderedMap<String, OwnedValueBag>, Error> {
    let tera_ctx = get_tera_context(context_values);
    let mut result = unordered_map!();

    for (key, value) in values.iter() {
        let value = expand_json_value(&tera_ctx, key, value)?;
        result.insert(key.to_string(), value);
    }

    Ok(result)
}

fn expand_json_value(
    tera_ctx: &tera::Context,
    key: &String,
    value: &tera::Value,
) -> Result<OwnedValueBag, Error> {
    Ok(match value {
        tera::Value::Null => {
            ValueBag::capture_serde1(&serde_json::Value::Null).to_owned()
        }
        tera::Value::Bool(b) => ValueBag::capture_serde1(b).to_owned(),
        tera::Value::Number(n) => ValueBag::capture_serde1(n).to_owned(),
        tera::Value::String(s) => {
            let expanded = omni_tera::one_off(
                &s,
                &format!("value for var {}", key),
                tera_ctx,
            )?;
            ValueBag::capture_serde1(&expanded).to_owned()
        }
        tera::Value::Array(values) => {
            let mut result = Vec::new();
            for value in values {
                let value = expand_json_value(tera_ctx, key, value)?;
                result.push(value);
            }
            ValueBag::capture_serde1(&result).to_owned()
        }
        tera::Value::Object(map) => {
            let mut result = unordered_map!();
            for (key, value) in map {
                let value = expand_json_value(tera_ctx, key, value)?;
                result.insert(key.to_string(), value);
            }
            ValueBag::capture_serde1(&result).to_owned()
        }
    })
}

fn prompt_generator_name(
    generators: &[GeneratorConfiguration],
) -> Result<String, Error> {
    let context_values = unordered_map!();
    let prompting_config = PromptingConfiguration::default();

    let prompt =
        PromptConfiguration::new_select(SelectPromptConfiguration::new(
            BasePromptConfiguration::new(
                "generator_name",
                "Select generator",
                None,
            ),
            generators
                .iter()
                .map(|g| {
                    OptionConfiguration::new(
                        g.display_name.as_deref().unwrap_or(&g.name.as_str()),
                        g.description.clone(),
                        g.name.clone(),
                        false,
                    )
                })
                .collect::<Vec<_>>(),
            Some("generator_name".to_string()),
        ));

    let value = omni_prompt::prompt_one(
        &prompt,
        None,
        &context_values,
        &prompting_config,
    )?
    .expect("should have value at this point");

    let value = value
        .by_ref()
        .to_str()
        .ok_or_else(|| eyre::eyre!("value is not a string"))?;

    Ok(value.to_string())
}
