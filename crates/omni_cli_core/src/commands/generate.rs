use maps::unordered_map;
use omni_context::Context;
use omni_generator::prompt::{
    self,
    configuration::{
        BasePromptConfiguration, PromptConfiguration, PromptingConfiguration,
        TextPromptConfiguration, ValidateConfiguration,
        ValidatedPromptConfiguration,
    },
};

#[derive(clap::Args)]
pub struct GenerateCommand {
    #[command(flatten)]
    pub args: GenerateArgs,
}

#[derive(clap::Args)]
pub struct GenerateArgs {}

pub async fn run(
    _generate: &GenerateCommand,
    _ctx: &Context,
) -> eyre::Result<()> {
    let configs =
        [PromptConfiguration::new_text(TextPromptConfiguration::new(
            ValidatedPromptConfiguration::new(
                BasePromptConfiguration::new("test", "test text?", None),
                vec![ValidateConfiguration {
                    condition: "{{ value == 'test' }}".to_string(),
                    error_message: Some("value should be 'test'".to_string()),
                }],
            ),
            None,
        ))];

    let prompting_config = PromptingConfiguration::default();
    let pre_exec = unordered_map!();
    let values = prompt::prompt(&configs, &pre_exec, &prompting_config)?;

    trace::info!("values: {:?}", values);

    Ok(())
}
