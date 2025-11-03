use omni_context::Context;
use omni_generator::prompt::{
    self, PromptingConfiguration,
    configuration::{
        BasePromptConfiguration, PromptConfiguration, TextPromptConfiguration,
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
    let configs = [PromptConfiguration::new_text(
        TextPromptConfiguration::new(ValidatedPromptConfiguration::new(
            BasePromptConfiguration::new(
                "test",
                "test text?",
                None,
                None,
                None,
            ),
            vec![],
        )),
    )];

    let prompting_config = PromptingConfiguration::default();
    let values = prompt::prompt(&configs, &prompting_config)?;

    trace::info!("values: {:?}", values);

    Ok(())
}
