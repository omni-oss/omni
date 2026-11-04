use clap::{Args, Subcommand};
use omni_api::{EnvRequest, OmniApi};
use omni_context::Context;
use omni_messages::NoopSubscriber;

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

pub async fn run(
    env: &EnvCommand,
    ctx: &Context,
    quiet: bool,
) -> eyre::Result<()> {
    let api = OmniApi::new_with_sys(ctx.clone(), NoopSubscriber);

    match env.subcommand {
        EnvSubcommands::Get { ref key } => {
            let response = api
                .get_env(EnvRequest {
                    key: Some(key.clone()),
                })
                .await?;
            if let Some(val) = response.vars.get(key.as_str()) {
                print!("{val}");
            } else {
                crate::subscriber::print_notice(
                    &crate::notices::EnvVarMissing { name: key.clone() },
                    quiet,
                );
            }
        }
        EnvSubcommands::All => {
            let response = api.get_env(EnvRequest { key: None }).await?;
            for (k, v) in &response.vars {
                println!("{k}={v}");
            }
        }
    }

    Ok(())
}
