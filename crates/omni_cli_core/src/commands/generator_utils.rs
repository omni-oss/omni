use std::borrow::Cow;

use maps::{UnorderedMap, unordered_map};
use omni_generator_configurations::GeneratorConfiguration;
use omni_input_provider::{CollectionConfig, builder, collect_one};
use omni_prompt::CliInputProvider;
use value_bag::{OwnedValueBag, ValueBag};

pub async fn prompt_generator_name(
    generators: &[Cow<'_, GeneratorConfiguration>],
) -> eyre::Result<String> {
    let context_values = unordered_map!();
    let prompting_config = CollectionConfig::default();

    let prompt = builder::select::<()>()
        .name("generator_name")
        .message("Select generator")
        .options(generators.iter().map(|generator| {
            builder::option()
                .name(
                    generator
                        .display_name
                        .clone()
                        .unwrap_or_else(|| generator.name.clone()),
                )
                .value(generator.name.clone())
                .maybe_description(generator.description.clone())
                .build()
        }))
        .build();

    let value = collect_one(
        &prompt,
        None,
        &context_values,
        &prompting_config,
        &CliInputProvider::default(),
    )
    .await?
    .expect("should have value at this point");

    let value = value
        .by_ref()
        .to_str()
        .ok_or_else(|| eyre::eyre!("value is not a string"))?;

    Ok(value.to_string())
}

pub fn get_input_values(
    values: &[(String, String)],
) -> UnorderedMap<String, OwnedValueBag> {
    UnorderedMap::from_iter(
        values.iter().map(|(k, v)| {
            (k.to_string(), ValueBag::capture_serde1(v).to_owned())
        }),
    )
}
