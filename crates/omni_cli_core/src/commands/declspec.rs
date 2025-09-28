use std::io::Write as _;

use clap::{CommandFactory as _, ValueEnum};

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
    format: Option<DeclspecFormat>,
}

#[derive(ValueEnum, Clone, Debug, Copy)]
enum DeclspecFormat {
    Json,
    Yaml,
    Toml,
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
    let format = args.format.unwrap_or(DeclspecFormat::Json);

    match format {
        DeclspecFormat::Json => {
            serde_json::to_writer_pretty(std::io::stdout(), &cli_spec)?;
        }
        DeclspecFormat::Yaml => {
            serde_norway::to_writer(std::io::stdout(), &cli_spec)?;
        }
        DeclspecFormat::Toml => {
            let text = toml::ser::to_string(&cli_spec)?;
            std::io::stdout().write_all(text.as_bytes())?;
        }
    }

    Ok(())
}
