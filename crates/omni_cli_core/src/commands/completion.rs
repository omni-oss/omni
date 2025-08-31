use std::io::BufWriter;

use clap::CommandFactory as _;
use clap_complete::Shell;

use crate::build;

use super::Cli;

#[derive(clap::Args)]
pub struct CompletionCommand {
    #[command(flatten)]
    pub args: CompletionArgs,
}

#[derive(clap::Args)]
pub struct CompletionArgs {
    #[arg(
        short = 's',
        long,
        help = "Which shell to generate completion for",
        default_value = "bash",
        value_enum
    )]
    pub shell: Option<Shell>,
}

pub async fn run(completion: &CompletionCommand) -> eyre::Result<()> {
    let shell = completion.args.shell.unwrap_or(Shell::Bash);
    let mut buf = BufWriter::new(std::io::stdout());

    clap_complete::generate(
        shell,
        &mut Cli::command(),
        build::PROJECT_NAME,
        &mut buf,
    );

    Ok(())
}
