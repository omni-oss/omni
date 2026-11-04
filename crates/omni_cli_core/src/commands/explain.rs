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

/// Explain an error or notice code, or list them all.
#[derive(clap::Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct ExplainCommand {
    #[command(subcommand)]
    subcommand: Option<ExplainSubcommand>,

    #[arg(help = "The code to explain, for example `meta.filter_failed`")]
    code: Option<String>,

    #[arg(
        short = 'f',
        long = "format",
        help = "The format to use for the output",
        value_enum,
        default_value = "text"
    )]
    format: ExplainFormat,
}

#[derive(clap::Subcommand)]
enum ExplainSubcommand {
    #[command(about = "List every error and notice code")]
    List(ListArgs),
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

pub async fn run(cmd: &ExplainCommand) -> eyre::Result<()> {
    if let Some(ExplainSubcommand::List(args)) = &cmd.subcommand {
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
        return Ok(());
    }

    let Some(code) = &cmd.code else {
        eyre::bail!(
            "pass a code to explain, or use `omni explain list` to see every code"
        );
    };
    let info = omni_codes::explain(code).map_err(UnknownCode::from_registry)?;
    match cmd.format {
        ExplainFormat::Text => print!("{}", info.render_text()),
        ExplainFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&info.to_json())?)
        }
    }
    Ok(())
}
