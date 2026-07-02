use std::{path::PathBuf, process::ExitCode};

use clap_utils::EnumValueAdapter;
use eyre::OptionExt;
use omni_configurations::Ui;

use crate::{
    commands::{common_args::RunArgs, common_types::SerializationFormat},
    executor::TaskExecutionResult,
    subscriber::CliSubscriber,
};

pub fn get_serialization_format(
    results: PathBuf,
    format: Option<SerializationFormat>,
) -> eyre::Result<SerializationFormat> {
    let fmt = match format {
        Some(r) => r,
        None => {
            let ext = results
                .extension()
                .ok_or_eyre("results file has no extension")?;

            match ext.to_string_lossy().as_ref() {
                "json" => SerializationFormat::Json,
                "yaml" | "yml" => SerializationFormat::Yaml,
                "toml" => SerializationFormat::Toml,
                _ => {
                    eyre::bail!(
                        "results file has an unsupported extension '{ext:?}'"
                    )
                }
            }
        }
    };

    Ok(fmt)
}

pub fn get_results_settings(
    args: &RunArgs,
) -> eyre::Result<Option<(SerializationFormat, PathBuf)>> {
    if let Some(results) = &args.result {
        let fmt =
            get_serialization_format(results.clone(), args.result_format)?;
        Ok(Some((fmt, results.clone())))
    } else {
        Ok(None)
    }
}

pub fn exit_code(results: &[TaskExecutionResult]) -> ExitCode {
    let has_error = results.iter().any(|r| r.is_failure());

    if has_error {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

pub fn resolve_subscriber(ui: Option<EnumValueAdapter<Ui>>) -> CliSubscriber {
    use omni_configurations::Ui::*;
    match ui.as_ref().map(|u| u.value()) {
        Some(Tui) => {
            if atty::is(atty::Stream::Stdout) {
                CliSubscriber::new_tui()
            } else {
                CliSubscriber::new_stream()
            }
        }
        Some(Stream) => CliSubscriber::new_stream(),
        Some(Auto) | None => CliSubscriber::new_auto(),
    }
}
