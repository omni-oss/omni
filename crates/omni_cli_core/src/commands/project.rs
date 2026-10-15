use clap::{ArgAction, Args, Subcommand};
use derive_new::new;
use omni_api::{OmniApi, ProjectListRequest, ProjectSelector};
use omni_context::Context;
use omni_messages::NoopSubscriber;
use omni_tracing_subscriber::noop_subscriber;
use owo_colors::OwoColorize as _;
use serde::Serialize;
use tracing_futures::WithSubscriber as _;

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
    let api = OmniApi::new_with_sys(ctx.clone(), NoopSubscriber);
    let selector = ProjectSelector::Name(command.args.project_name.clone());

    let result = if command.args.raw {
        api.project_config(selector)
            .with_subscriber(noop_subscriber())
            .await
    } else {
        api.project_config(selector).await
    };

    match result {
        Ok(response) => {
            for warning in &response.warnings {
                eprintln!("{} {}", "warning:".yellow(), warning);
            }
            omni_file_data_serde::to_writer(
                &mut std::io::stdout(),
                &response.config,
                command.args.format.to_serde_format(),
            )?;
        }
        Err(e) => {
            log::error!("{}", e);
        }
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
    let api = OmniApi::new_with_sys(ctx.clone(), NoopSubscriber);

    let request = ProjectListRequest::default();
    let entries = if command.args.raw {
        api.project_list(request)
            .with_subscriber(noop_subscriber())
            .await?
    } else {
        api.project_list(request).await?
    };
    let names = entries
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();

    if let Some(format) = command.args.format {
        if format == SerializationFormat::Toml {
            omni_file_data_serde::to_writer(
                &mut std::io::stdout(),
                &ProjectNames::new(names),
                format.to_serde_format(),
            )?;
        } else {
            omni_file_data_serde::to_writer(
                &mut std::io::stdout(),
                &names,
                format.to_serde_format(),
            )?;
        }
    } else {
        for name in &names {
            println!("{name}");
        }
    }

    Ok(())
}

#[derive(Serialize, new)]
struct ProjectNames {
    projects: Vec<String>,
}
