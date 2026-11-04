use omni_codes::{
    CodeKind,
    catalog::{self, Format},
};
use omni_mcp_core::codes::UnknownCode;

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum ExplainFormat {
    Text,
    Json,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum ListFormat {
    Text,
    Json,
    Md,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum KindFilter {
    Error,
    Notice,
}

impl From<KindFilter> for CodeKind {
    fn from(k: KindFilter) -> Self {
        match k {
            KindFilter::Error => CodeKind::Error,
            KindFilter::Notice => CodeKind::Notice,
        }
    }
}

/// Look up error and notice codes.
#[derive(clap::Args)]
pub struct CodeCommand {
    #[command(subcommand)]
    subcommand: CodeSubcommand,
}

#[derive(clap::Subcommand)]
enum CodeSubcommand {
    #[command(about = "List every error and notice code")]
    List(ListArgs),

    #[command(about = "Explain an error or notice code")]
    Explain(ExplainArgs),
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

    #[arg(
        short = 'k',
        long = "kind",
        help = "Only list codes of this kind",
        value_enum
    )]
    kind: Option<KindFilter>,
}

#[derive(clap::Args)]
struct ExplainArgs {
    #[arg(help = "The code to explain, for example `meta.filter_failed`")]
    code: String,

    #[arg(
        short = 'f',
        long = "format",
        help = "The format to use for the output",
        value_enum,
        default_value = "text"
    )]
    format: ExplainFormat,
}

pub async fn run(cmd: &CodeCommand) -> eyre::Result<()> {
    match &cmd.subcommand {
        CodeSubcommand::List(args) => {
            let format = match args.format {
                ListFormat::Text => Format::Text,
                ListFormat::Json => Format::Json,
                ListFormat::Md => Format::Markdown,
            };
            let kind = args.kind.map(CodeKind::from);
            let entries: Vec<_> = catalog::entries()
                .into_iter()
                .filter(|c| kind.is_none_or(|k| c.kind() == k))
                .collect();
            print!("{}", catalog::render_entries(&entries, format));
            if format == Format::Json {
                println!();
            }
        }
        CodeSubcommand::Explain(args) => {
            let info = omni_codes::lookup(&args.code)
                .map_err(UnknownCode::from_registry)?;
            match args.format {
                ExplainFormat::Text => print!("{}", info.render_text()),
                ExplainFormat::Json => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&info.to_json())?
                    )
                }
            }
        }
    }
    Ok(())
}
