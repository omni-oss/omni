use clap::ValueEnum;
use schemars::{Schema as SchemarsSchema, schema_for};

use crate::configurations::{
    GeneratorConfiguration, ProjectConfiguration, WorkspaceConfiguration,
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
    Generator,
}

#[derive(clap::Args)]
pub struct PrintSchemaArgs {
    #[arg(long, short, help = "Pretty print the schema")]
    pretty: bool,
}

fn output_schema(
    schema: &SchemarsSchema,
    args: &PrintSchemaArgs,
) -> eyre::Result<()> {
    if args.pretty {
        println!("{}", serde_json::to_string_pretty(schema)?);
    } else {
        println!("{}", serde_json::to_string(schema)?);
    }

    Ok(())
}

pub async fn run(config: &ConfigCommand) -> eyre::Result<()> {
    match config.subcommand {
        ConfigSubcommands::PrintSchema {
            schema: ref subcommand,
            ref args,
        } => {
            let sc = match subcommand {
                Schema::Workspace => {
                    schema_for!(WorkspaceConfiguration)
                }
                Schema::Project => {
                    schema_for!(ProjectConfiguration)
                }
                Schema::Generator => {
                    schema_for!(GeneratorConfiguration)
                }
            };

            output_schema(&sc, args)?;
        }
    }

    Ok(())
}
