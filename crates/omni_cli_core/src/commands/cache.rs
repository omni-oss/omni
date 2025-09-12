use clap::Subcommand;

use crate::context::Context;

#[derive(clap::Args)]
#[command(author, version, about = "Get Cachees for the workspace or projects")]
pub struct CacheCommand {
    #[command(subcommand)]
    subcommand: CacheSubcommands,
}

#[derive(Subcommand)]
pub enum CacheSubcommands {
    #[command(about = "Print the cache directory")]
    Dir,
}

pub async fn run(command: &CacheCommand, ctx: &Context) -> eyre::Result<()> {
    match &command.subcommand {
        CacheSubcommands::Dir => {
            println!("{}", ctx.cache_dir().display());
        }
    }

    Ok(())
}
