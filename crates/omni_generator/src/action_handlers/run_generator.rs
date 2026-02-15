use std::borrow::Cow;

use maps::{UnorderedMap, unordered_map};
use omni_generator_configurations::{
    ForAllPromptValuesConfiguration, ForwardPromptValuesConfiguration,
    PromptValue, RunGeneratorActionConfiguration,
};
use value_bag::{OwnedValueBag, ValueBag};

use crate::{
    GeneratorSys, RunConfig,
    action_handlers::HandlerContext,
    error::{Error, ErrorInner},
    run_internal,
};

pub async fn run_generator<'a>(
    config: &RunGeneratorActionConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    let generator = ctx
        .available_generators
        .iter()
        .find(|g| g.name == config.generator)
        .ok_or_else(|| ErrorInner::GeneratorNotFound {
            name: config.generator.clone(),
        })?;

    let parent_prompts = ctx.context_values
        .get("prompts")
        .expect("should have prompt vaues, if you encountered this error, please report it to the maintainers");

    let prompt_values = resolve_prompt_values(parent_prompts, config, &ctx)?;

    trace::trace!("resolved prompt values: {prompt_values:#?}");

    let target_overrides = if config.targets.is_empty() {
        Cow::Borrowed(ctx.target_overrides)
    } else {
        let mut map = ctx.target_overrides.clone();

        for (key, value) in &config.targets {
            if !map.contains_key(key) {
                map.insert(key.clone(), value.clone());
            }
        }

        Cow::Owned(map)
    };

    trace::trace!("resolved target overrides: {target_overrides:#?}");

    let run_config = RunConfig {
        dry_run: ctx.dry_run,
        output_dir: ctx.output_path,
        workspace_dir: ctx.workspace_dir,
        overwrite: ctx.overwrite,
        context_values: ctx.context_values,
        prompt_values: prompt_values.as_ref(),
        target_overrides: target_overrides.as_ref(),
        current_dir: ctx.current_dir,
        env: ctx.env,
        args: Some(&config.args),
    };

    let prompted_values = Box::pin(run_internal(
        generator,
        ctx.available_generators,
        &run_config,
        sys,
    ))
    .await?;

    ctx.gen_session.merge(prompted_values);

    Ok(())
}

fn resolve_prompt_values<'a>(
    parent_prompts: &'a OwnedValueBag,
    config: &RunGeneratorActionConfiguration,
    ctx: &HandlerContext<'a>,
) -> Result<Cow<'a, UnorderedMap<String, OwnedValueBag>>, Error> {
    let parsed = serde_json::to_value(parent_prompts)?;

    if !parsed.is_object() {
        return Err(ErrorInner::Custom(eyre::eyre!(
            "prompts should be an object, but got: {parsed:?}"
        ))
        .into());
    }

    let prompts = parsed.as_object().expect("should be object at this point");

    let mut all_values = unordered_map!();

    match &config.prompt_values.forward {
        ForwardPromptValuesConfiguration::ForAll(
            ForAllPromptValuesConfiguration::All,
        ) => {
            for (key, value) in prompts {
                all_values.insert(
                    key.clone(),
                    ValueBag::capture_serde1(value).to_owned(),
                );
            }
        }
        ForwardPromptValuesConfiguration::ForAll(
            ForAllPromptValuesConfiguration::None,
        ) => {
            // do nothing
        }
        ForwardPromptValuesConfiguration::Selected(l) => {
            for key in l {
                if let Some(value) = prompts.get(key) {
                    all_values.insert(
                        key.clone(),
                        ValueBag::capture_serde1(value).to_owned(),
                    );
                }
            }
        }
    }

    for (key, value) in config.prompt_values.values.iter() {
        let converted =
            to_owned_value_bag(key, value, ctx.tera_context_values)?;

        all_values.insert(key.clone(), converted);
    }

    Ok(Cow::Owned(all_values))
}

fn to_owned_value_bag(
    key: &str,
    prompt_value: &PromptValue,
    tera_ctx: &tera::Context,
) -> Result<OwnedValueBag, Error> {
    Ok(match prompt_value {
        PromptValue::Integer(i) => ValueBag::capture_serde1(i).to_owned(),
        PromptValue::Float(f) => ValueBag::capture_serde1(f).to_owned(),
        PromptValue::Boolean(b) => ValueBag::capture_serde1(b).to_owned(),
        PromptValue::String(s) => ValueBag::capture_serde1(
            &omni_tera::one_off(&s, &format!("prompts.{key}"), tera_ctx)?,
        )
        .to_owned(),
        PromptValue::List(values) => {
            let mut list = Vec::with_capacity(values.len());

            for (idx, value) in values.iter().enumerate() {
                list.push(to_owned_value_bag(
                    &format!("{key}.{idx}"),
                    value,
                    tera_ctx,
                )?);
            }

            ValueBag::capture_serde1(&list).to_owned()
        }
    })
}
