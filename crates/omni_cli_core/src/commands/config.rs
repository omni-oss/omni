use clap::ValueEnum;
use omni_api::{SchemaKind, handle_config_schema};

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
    Schema {
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

pub async fn run(config: &ConfigCommand) -> eyre::Result<()> {
    match config.subcommand {
        ConfigSubcommands::Schema {
            schema: ref subcommand,
            ref args,
        } => {
            let kind = match subcommand {
                Schema::Workspace => SchemaKind::Workspace,
                Schema::Project => SchemaKind::Project,
                Schema::Generator => SchemaKind::Generator,
            };

            let response = handle_config_schema(kind)?;

            if args.pretty {
                println!("{}", serde_json::to_string_pretty(&response.schema)?);
            } else {
                println!("{}", serde_json::to_string(&response.schema)?);
            }
        }
    }

    Ok(())
}
