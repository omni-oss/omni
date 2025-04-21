use clap::{Args, Subcommand};

use crate::context::Context;

#[derive(Subcommand)]
#[command(rename_all = "kebab-case")]
enum EnvSubcommands {
    /// Retrieves the value of an environment variable
    Get {
        #[arg(required = true, name = "key")]
        /// The name of the environment variable to retrieve
        key: String,
    },
    /// Retrieves all environment variables
    All,
}

#[derive(Args)]
pub struct EnvCommand {
    #[command(subcommand)]
    subcommand: EnvSubcommands,
}

pub async fn run(env: &EnvCommand, ctx: &Context) -> eyre::Result<()> {
    match env.subcommand {
        EnvSubcommands::Get { ref key } => {
            let env = ctx.get_env_var(key);

            if let Some(env) = env {
                print!("{}", env);
            } else {
                tracing::warn!(
                    "environmental variable does not exists: {}",
                    key
                );
            }
        }
        EnvSubcommands::All => {
            for (key, value) in ctx.get_all_env_vars() {
                println!("{}={}", key, value);
            }
        }
    }
    Ok(())
}
