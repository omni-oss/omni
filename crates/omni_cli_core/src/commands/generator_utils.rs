use std::borrow::Cow;

use maps::{UnorderedMap, unordered_map};
use omni_generator_configurations::{
    AllowedBuilderAllowedExtrasExt, Generator, GeneratorConfiguration,
    StringBuilderGenBaseExt,
};
use omni_input_provider::{ValidationConfig, collect_one};
use omni_prompt::builder::allowed;
use omni_prompt::{CliInputProvider, builder::string};
use value_bag::{OwnedValueBag, ValueBag};

pub async fn prompt_generator_name(
    generators: &[Cow<'_, GeneratorConfiguration>],
) -> eyre::Result<String> {
    let context_values = unordered_map!();
    let prompting_config = ValidationConfig::default();

    let prompt = string::<Generator>()
        .name("generator_name")
        .gen_base(|b| b.message("Select generator"))
        .allowed(generators.iter().map(|generator| {
            allowed()
                .value(generator.name.clone())
                .maybe_description(generator.description.clone())
                .allowed_extras(|b| {
                    b.name(
                        generator
                            .display_name
                            .clone()
                            .unwrap_or_else(|| generator.name.clone()),
                    )
                    .separator(false)
                })
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
