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

#[derive(Debug, Clone, clap::Args)]
pub struct GeneratorCommand {
    #[command(subcommand)]
    pub subcommand: GeneratorSubcommand,
}

#[derive(Debug, Clone, clap::Subcommand)]
pub enum GeneratorSubcommand {
    Run(#[command(flatten)] GeneratorRunCommand),

    #[command(alias = "ls")]
    List(#[command(flatten)] GeneratorListCommand),
}

#[derive(Debug, Clone, clap::Args)]
pub struct GeneratorRunCommand {
    #[command(flatten)]
    pub args: GeneratorRunArgs,
}

#[derive(Debug, Clone, clap::Args)]
pub struct GeneratorRunArgs {
    #[arg(long = "name", short = 'n', help = "Generator name")]
    pub name: Option<String>,

    #[arg(
        long,
        short,
        help = "If provided, it will use the project's directory as output directory",
        conflicts_with = "out_dir"
    )]
    pub project: Option<String>,

    #[arg(long, short, help = "Output directory")]
    pub out_dir: Option<String>,

    #[arg(long, short, help = "Prefill answers to prompts")]
    pub answer: Vec<String>,
}

#[derive(Debug, Clone, clap::Args)]
pub struct GeneratorListCommand {
    #[command(flatten)]
    pub args: GeneratorListArgs,
}

#[derive(Debug, Clone, clap::Args)]
pub struct GeneratorListArgs {}

pub async fn run(
    _generate: &GeneratorCommand,
    _ctx: &Context,
) -> eyre::Result<()> {
    let configs =
        [PromptConfiguration::new_text(TextPromptConfiguration::new(
            ValidatedPromptConfiguration::new(
                BasePromptConfiguration::new("test", "test text?", None),
                vec![ValidateConfiguration {
                    condition: "{{ prompts.test == 'test' }}".to_string(),
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
