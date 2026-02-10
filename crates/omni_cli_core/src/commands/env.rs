use clap::{Args, Subcommand};
use omni_context::GetVarsArgs;

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

pub async fn run(env: &EnvCommand, ctx: &mut Context) -> eyre::Result<()> {
    let mut env_loader = ctx.create_env_loader();
    let env_vars = env_loader.get(&GetVarsArgs {
        inherit_env_vars: ctx.inherit_env_vars(),
        ..Default::default()
    })?;

    match env.subcommand {
        EnvSubcommands::Get { ref key } => {
            if let Some(env) = env_vars.get(key) {
                print!("{env}");
            } else {
                trace::warn!("environmental variable does not exists: {}", key);
            }
        }
        EnvSubcommands::All => {
            for (key, value) in env_vars.iter() {
                println!("{key}={value}");
            }
        }
    }
    Ok(())
}
