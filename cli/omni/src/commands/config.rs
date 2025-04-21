use clap::ValueEnum;
use schemars::{schema::RootSchema, schema_for};

use crate::{
    configurations::{ProjectConfiguration, WorkspaceConfiguration},
    context::Context,
};

#[derive(clap::Args)]
pub struct ConfigCommand {
    #[command(flatten)]
    pub args: ConfigArgs,

    #[command(subcommand)]
    pub subcommand: ConfigSubcommands,
}

#[derive(clap::Args)]
pub struct ConfigArgs {}

#[derive(clap::Subcommand)]
pub enum ConfigSubcommands {
    PrintSchema {
        #[arg(value_enum, required = true)]
        schema: Schema,

        #[command(flatten)]
        args: PrintSchemaArgs,
    },
}

#[derive(ValueEnum, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug)]
#[value(rename_all = "kebab-case")]
pub enum Schema {
    Workspace,
    Project,
}

#[derive(clap::Args)]
pub struct PrintSchemaArgs {
    #[arg(long, short, help = "Pretty print the schema")]
    pretty: bool,
}

fn output_schema(
    schema: &RootSchema,
    args: &PrintSchemaArgs,
) -> eyre::Result<()> {
    if args.pretty {
        println!("{}", serde_json::to_string_pretty(schema)?);
    } else {
        println!("{}", serde_json::to_string(schema)?);
    }

    Ok(())
}

pub async fn run(config: &ConfigCommand, _ctx: &Context) -> eyre::Result<()> {
    match config.subcommand {
        ConfigSubcommands::PrintSchema {
            schema: ref subcommand,
            ref args,
        } => match subcommand {
            Schema::Workspace => {
                let sc = schema_for!(WorkspaceConfiguration);
                output_schema(&sc, args)?;
            }
            Schema::Project => {
                let sc = schema_for!(ProjectConfiguration);
                output_schema(&sc, args)?;
            }
        },
    }

    Ok(())
}
