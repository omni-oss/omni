use std::path::Path;

use derive_new::new;
use maps::{Map, UnorderedMap, unordered_map};
use omni_core::Project;
use omni_generator_configurations::GeneratorConfiguration;
use omni_prompt::configuration::PromptingConfiguration;
use value_bag::{OwnedValueBag, ValueBag};

use crate::error::Error;

#[derive(Debug, new)]
pub struct RunConfig<'a> {
    pub dry_run: bool,
    pub output_dir: &'a Path,
}

pub async fn run<'a>(
    r#gen: &GeneratorConfiguration,
    pre_exec_values: &UnorderedMap<String, OwnedValueBag>,
    project: Option<&'a Project>,
    env: &Map<String, String>,
    _config: &RunConfig<'a>,
) -> Result<(), Error> {
    let prompting_config = PromptingConfiguration::default();

    let project = project.map(|p| ValueBag::from_serde1(p).to_owned());
    let env = ValueBag::capture_serde1(env).to_owned();

    let mut context_values = unordered_map!(
        "env".to_string() => env,
    );

    if let Some(project) = project {
        context_values.insert("project".to_string(), project);
    }

    let values = omni_prompt::prompt(
        &r#gen.prompts,
        &pre_exec_values,
        &context_values,
        &prompting_config,
    )?;

    trace::info!("values: {:?}", values);

    Ok(())
}
