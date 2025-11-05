use std::path::Path;

use derive_new::new;
use maps::{UnorderedMap, unordered_map};
use omni_generator_configurations::GeneratorConfiguration;
use omni_prompt::configuration::PromptingConfiguration;
use value_bag::OwnedValueBag;

use crate::error::Error;

#[derive(Debug, new)]
pub struct RunConfig<'a> {
    pub dry_run: bool,
    pub output_dir: &'a Path,
}

pub async fn run<'a>(
    r#gen: &GeneratorConfiguration,
    pre_exec_values: &UnorderedMap<String, OwnedValueBag>,
    _config: &RunConfig<'a>,
) -> Result<(), Error> {
    let prompting_config = PromptingConfiguration::default();

    let values = omni_prompt::prompt(
        &r#gen.prompts,
        &pre_exec_values,
        &unordered_map!(),
        &prompting_config,
    )?;

    trace::info!("values: {:?}", values);

    Ok(())
}
