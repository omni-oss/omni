use omni_diagnostic::catalog::{self, Format};

#[derive(clap::Args)]
pub struct DiagnosticsCommand {
    #[command(subcommand)]
    subcommand: DiagnosticsSubcommand,
}

#[derive(clap::Subcommand)]
enum DiagnosticsSubcommand {
    #[command(about = "List every diagnostic code")]
    List(ListArgs),
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum ListFormat {
    Text,
    Json,
    Md,
}

#[derive(clap::Args)]
struct ListArgs {
    #[arg(
        short = 'f',
        long = "format",
        help = "The format to use for the output",
        value_enum,
        default_value = "text"
    )]
    format: ListFormat,
}

pub async fn run(cmd: &DiagnosticsCommand) -> eyre::Result<()> {
    match &cmd.subcommand {
        DiagnosticsSubcommand::List(args) => {
            let format = match args.format {
                ListFormat::Text => Format::Text,
                ListFormat::Json => Format::Json,
                ListFormat::Md => Format::Markdown,
            };
            print!("{}", catalog::render(format));
            if format == Format::Json {
                println!();
            }
        }
    }
    Ok(())
}
