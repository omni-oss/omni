use std::borrow::Cow;

use maps::{UnorderedMap, unordered_map};
use omni_generator_configurations::GeneratorConfiguration;
use omni_prompt::configuration::{
    BasePromptConfiguration, OptionConfiguration, PromptConfiguration,
    PromptingConfiguration, SelectPromptConfiguration,
};
use value_bag::{OwnedValueBag, ValueBag};

pub fn prompt_generator_name(
    generators: &[Cow<GeneratorConfiguration>],
) -> eyre::Result<String> {
    let context_values = unordered_map!();
    let prompting_config = PromptingConfiguration::default();

    let prompt =
        PromptConfiguration::<()>::new_select(SelectPromptConfiguration::new(
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
            None,
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

pub fn get_prompt_values(
    values: &[(String, String)],
) -> UnorderedMap<String, OwnedValueBag> {
    UnorderedMap::from_iter(
        values.iter().map(|(k, v)| {
            (k.to_string(), ValueBag::capture_serde1(v).to_owned())
        }),
    )
}
