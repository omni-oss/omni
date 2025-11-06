use std::path::Path;

use derive_new::new;
use maps::UnorderedMap;
use omni_generator_configurations::GeneratorConfiguration;
use omni_prompt::configuration::PromptingConfiguration;
use value_bag::{OwnedValueBag, ValueBag};

use crate::{
    error::Error,
    execute_actions::{ExecuteActionsArgs, execute_actions},
};

#[derive(Debug, new)]
pub struct RunConfig<'a> {
    pub dry_run: bool,
    pub output_dir: &'a Path,
}

pub async fn run<'a>(
    r#gen: &GeneratorConfiguration,
    pre_exec_values: &UnorderedMap<String, OwnedValueBag>,
    context_values: &UnorderedMap<String, OwnedValueBag>,
    config: &RunConfig<'a>,
) -> Result<(), Error> {
    let prompting_config = PromptingConfiguration::default();

    let values = omni_prompt::prompt(
        &r#gen.prompts,
        &pre_exec_values,
        context_values,
        &prompting_config,
    )?;

    trace::trace!("prompt values: {:#?}", values);

    let mut context_values = context_values.clone();

    context_values.insert(
        "prompts".to_string(),
        ValueBag::capture_serde1(&values).to_owned(),
    );

    let args = ExecuteActionsArgs {
        actions: &r#gen.actions,
        context_values: &context_values,
        dry_run: config.dry_run,
        output_dir: config.output_dir,
    };

    execute_actions(&args).await?;

    Ok(())
}
