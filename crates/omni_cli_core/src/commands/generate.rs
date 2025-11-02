use omni_context::Context;

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
    Ok(())
}
