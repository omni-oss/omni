use std::io::{BufWriter, Write as _};

use clap::{CommandFactory as _, ValueEnum};

use super::Cli;

#[derive(clap::Args)]
pub struct DeclspecCommand {
    #[command(flatten)]
    args: DeclspecArgs,
    #[command(subcommand)]
    subcommand: Option<DeclspecSubcommand>,
}

#[derive(clap::Args)]
pub struct DeclspecArgs {
    #[arg(
        long = "format",
        help = "The format to use for the output",
        value_enum,
        default_value = "json"
    )]
    format: Option<DeclspecFormat>,
}

#[derive(clap::Subcommand)]
enum DeclspecSubcommand {
    Schema {
        #[command(flatten)]
        args: SchemaArgs,
    },
}

#[derive(clap::Args)]
struct SchemaArgs {
    #[arg(long, short, help = "Pretty print the schema", default_value = "false", action = clap::ArgAction::SetTrue)]
    pretty: Option<bool>,
}

#[derive(ValueEnum, Clone, Debug, Copy)]
enum DeclspecFormat {
    Json,
    Yaml,
    Toml,
}

pub async fn run(cmd: &DeclspecCommand) -> eyre::Result<()> {
    let mut buf = BufWriter::new(std::io::stdout());

    if let Some(subcommand) = &cmd.subcommand {
        return run_subcommands(subcommand).await;
    }

    let format = cmd.args.format.unwrap_or(DeclspecFormat::Json);

    let cli_spec = clap_declspec::to_decspec(&Cli::command());
    match format {
        DeclspecFormat::Json => {
            serde_json::to_writer_pretty(&mut buf, &cli_spec)?
        }
        DeclspecFormat::Yaml => serde_yml::to_writer(&mut buf, &cli_spec)?,
        DeclspecFormat::Toml => {
            let text = toml::ser::to_string(&cli_spec)?;
            buf.write_all(text.as_bytes())?;
        }
    }

    Ok(())
}

async fn run_subcommands(sc: &DeclspecSubcommand) -> eyre::Result<()> {
    match sc {
        DeclspecSubcommand::Schema { args } => {
            run_schema(args).await?;
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
