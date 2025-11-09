use std::path::Path;

use derive_new::new;
use maps::UnorderedMap;
use omni_generator_configurations::GeneratorConfiguration;
use omni_prompt::configuration::PromptingConfiguration;
use value_bag::{OwnedValueBag, ValueBag};

use crate::{
    GeneratorSys,
    error::Error,
    execute_actions::{ExecuteActionsArgs, execute_actions},
    sys_impl::DryRunSys,
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
    sys: &impl GeneratorSys,
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
        generator_dir: &r#gen
            .file
            .parent()
            .expect("generator should have a directory"),
        targets: &r#gen.targets,
    };

    if config.dry_run {
        execute_actions(&args, &DryRunSys::default()).await?;
    } else {
        execute_actions(&args, sys).await?;
    }

    Ok(())
}
