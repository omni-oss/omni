use std::borrow::Cow;

use maps::{UnorderedMap, unordered_map};
use omni_generator_configurations::GeneratorConfiguration;
use omni_input_provider::{
    CollectionConfig, collect_one,
    configuration::{
        BaseInputConfiguration, InputConfiguration, OptionConfiguration,
        SelectInputConfiguration,
    },
};
use omni_prompt::CliInputProvider;
use value_bag::{OwnedValueBag, ValueBag};

pub async fn prompt_generator_name(
    generators: &[Cow<'_, GeneratorConfiguration>],
) -> eyre::Result<String> {
    let context_values = unordered_map!();
    let prompting_config = CollectionConfig::default();

    let prompt =
        InputConfiguration::<()>::new_select(SelectInputConfiguration::new(
            BaseInputConfiguration::new(
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
            None,
        ));

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
