use clap::CommandFactory as _;

use crate::commands::common_types::SerializationFormat;

use super::Cli;

#[derive(clap::Args)]
pub struct DeclspecCommand {
    #[command(subcommand)]
    subcommand: DeclspecSubcommand,
}

#[derive(clap::Subcommand)]
enum DeclspecSubcommand {
    Schema {
        #[command(flatten)]
        args: SchemaArgs,
    },
    Dump {
        #[command(flatten)]
        args: DumpArgs,
    },
}

#[derive(clap::Args)]
struct SchemaArgs {
    #[arg(long, short, help = "Pretty print the schema", default_value = "false", action = clap::ArgAction::SetTrue)]
    pretty: Option<bool>,
}

#[derive(clap::Args)]
pub struct DumpArgs {
    #[arg(
        short = 'f',
        long = "format",
        help = "The format to use for the output",
        value_enum,
        default_value = "json"
    )]
    format: Option<SerializationFormat>,
}

pub async fn run(cmd: &DeclspecCommand) -> eyre::Result<()> {
    match &cmd.subcommand {
        DeclspecSubcommand::Schema { args } => {
            run_schema(args).await?;
        }
        DeclspecSubcommand::Dump { args } => {
            run_dump(args).await?;
        }
    }

    Ok(())
}

async fn run_schema(args: &SchemaArgs) -> eyre::Result<()> {
    let sc = schemars::schema_for!(clap_declspec::CliDeclspec);
    let pretty = args.pretty.unwrap_or(false);
    let json = if pretty {
        serde_json::to_string_pretty(&sc)?
    } else {
        serde_json::to_string(&sc)?
    };

    println!("{json}");

    Ok(())
}

async fn run_dump(args: &DumpArgs) -> eyre::Result<()> {
    let cli_spec = clap_declspec::to_decspec(&Cli::command());
    let format = args.format.unwrap_or(SerializationFormat::Json);

    omni_file_data_serde::to_writer(
        &mut std::io::stdout(),
        &cli_spec,
        format.to_serde_format(),
    )?;

    Ok(())
}
