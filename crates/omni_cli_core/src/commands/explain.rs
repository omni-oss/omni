use omni_mcp_core::codes::UnknownCode;

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum ExplainFormat {
    Text,
    Json,
}

/// Explain an error or notice code.
#[derive(clap::Args)]
pub struct ExplainCommand {
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

pub async fn run(cmd: &ExplainCommand) -> eyre::Result<()> {
    let info =
        omni_codes::explain(&cmd.code).map_err(UnknownCode::from_registry)?;
    match cmd.format {
        ExplainFormat::Text => print!("{}", info.render_text()),
        ExplainFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&info.to_json())?)
        }
    }
    Ok(())
}
