use clap::{ArgAction, Args, Subcommand};
use derive_new::new;
use omni_context::Context;
use omni_tracing_subscriber::noop_subscriber;
use serde::Serialize;
use tracing_futures::WithSubscriber;

use crate::commands::utils::write_serialized_to;

use super::common_types::SerializationFormat;

#[derive(Args, Debug)]
pub struct ProjectCommand {
    #[command(flatten)]
    pub args: ProjectArgs,

    #[command(subcommand)]
    pub subcommand: ProjectSubcommand,
}

#[derive(Args, Debug)]
pub struct ProjectArgs {}

#[derive(Subcommand, Debug)]
pub enum ProjectSubcommand {
    PrintConfig(PrintConfigCommand),
    List(ListCommand),
}

pub async fn run(cmd: &ProjectCommand, ctx: &Context) -> eyre::Result<()> {
    match &cmd.subcommand {
        ProjectSubcommand::PrintConfig(command) => {
            run_print_config(command, ctx).await?;
        }
        ProjectSubcommand::List(command) => {
            run_list(command, ctx).await?;
        }
    }

    Ok(())
}

#[derive(Args, Debug)]
pub struct PrintConfigCommand {
    #[command(flatten)]
    args: PrintConfigArgs,
}

#[derive(Args, Debug)]
pub struct PrintConfigArgs {
    #[arg(
        value_enum,
        long,
        short,
        default_value_t = SerializationFormat::Json,
        help = "Format to use when serializing"
    )]
    format: SerializationFormat,

    #[arg(required = true)]
    project_name: String,

    #[arg(long, short, action = ArgAction::SetTrue, help = "Only print the raw configuration syntax, no logs will be added")]
    raw: bool,
}

async fn run_print_config(
    command: &PrintConfigCommand,
    ctx: &Context,
) -> eyre::Result<()> {
    let loaded = if command.args.raw {
        ctx.clone()
            .load_project_configurations()
            .with_subscriber(noop_subscriber())
            .await?
    } else {
        ctx.load_project_configurations().await?
    };

    let result = loaded.iter().find(|x| x.name == command.args.project_name);

    if let Some(result) = result {
        write_serialized_to(result, command.args.format, std::io::stdout())?;
    } else {
        log::error!("No project named '{}' found", command.args.project_name);
    }

    Ok(())
}

#[derive(Args, Debug)]
pub struct ListCommand {
    #[command(flatten)]
    args: ListArgs,
}

#[derive(Args, Debug)]
pub struct ListArgs {
    #[arg(long, short, action = ArgAction::SetTrue, help = "Only print the raw list, no logs will be added")]
    raw: bool,

    #[arg(
        value_enum,
        long,
        short,
        help = "If provided, the list will be serialized in the format specified"
    )]
    format: Option<SerializationFormat>,
}

async fn run_list(command: &ListCommand, ctx: &Context) -> eyre::Result<()> {
    let loaded = if command.args.raw {
        ctx.clone()
            .load_project_configurations()
            .with_subscriber(noop_subscriber())
            .await?
    } else {
        ctx.load_project_configurations().await?
    };

    if let Some(format) = command.args.format {
        let names = loaded.iter().map(|e| e.name.as_str()).collect::<Vec<_>>();
        if format == SerializationFormat::Toml {
            write_serialized_to(
                ProjectNames::new(&names),
                format,
                std::io::stdout(),
            )?;
        } else {
            write_serialized_to(names, format, std::io::stdout())?;
        }
    } else {
        for project in &loaded {
            println!("{}", project.name);
        }
    }

    Ok(())
}

#[derive(Serialize, new)]
struct ProjectNames<'a> {
    projects: &'a Vec<&'a str>,
}
