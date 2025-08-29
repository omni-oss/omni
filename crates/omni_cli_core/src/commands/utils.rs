use std::{fs::OpenOptions, path::PathBuf, process::ExitCode, time::Duration};

use eyre::OptionExt;
use owo_colors::OwoColorize as _;
use tiny_gradient::{Gradient, GradientStr};

use crate::{
    commands::common_args::{ResultFormat, RunArgs},
    executor::TaskExecutionResult,
};

pub fn report_execution_results(results: &[TaskExecutionResult]) {
    let mut skipped = 0;
    let mut errored = 0;
    let mut success = 0;
    let mut cached_success = 0;
    let mut cached_error = 0;
    let mut total_saved_time = Duration::ZERO;

    for res in results {
        if res.is_skipped() {
            skipped += 1;
        } else if !res.success() {
            errored += 1;
        } else if res.success() {
            success += 1;
        }

        if let TaskExecutionResult::Completed {
            exit_code,
            elapsed,
            cache_hit,
            ..
        } = res
            && *cache_hit
        {
            if *exit_code == 0 {
                cached_success += 1;
            } else {
                cached_error += 1;
            }
            total_saved_time += *elapsed;
        }
    }

    if success > 0 {
        trace::info!(
            "{}",
            format!(
                "Successfully executed {} tasks ({} results from cache)",
                success, cached_success
            )
            .green()
            .bold()
        );
    }
    if errored > 0 {
        trace::info!(
            "{}",
            format!(
                "Failed to execute {} tasks ({} results from cache)",
                errored, cached_error
            )
            .red()
            .bold()
        );
    }
    if skipped > 0 {
        trace::info!(
            "{}",
            format!("Skipped {} tasks", skipped).yellow().bold()
        );
    }
    if (cached_error + cached_success) > 0 {
        trace::info!(
            "{}",
            format!(
                "Saved {:?} in total from cached results",
                total_saved_time
            )
            .gradient(Gradient::Instagram)
            .bold()
        )
    }
}

pub fn get_result_format(
    results: PathBuf,
    format: Option<ResultFormat>,
) -> eyre::Result<ResultFormat> {
    let fmt = match format {
        Some(r) => r,
        None => {
            let ext = results
                .extension()
                .ok_or_eyre("results file has no extension")?;

            match ext.to_string_lossy().as_ref() {
                "json" => ResultFormat::Json,
                "yaml" | "yml" => ResultFormat::Yaml,
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
) -> eyre::Result<Option<(ResultFormat, PathBuf)>> {
    if let Some(results) = &args.result {
        let fmt = get_result_format(results.clone(), args.result_format)?;
        Ok(Some((fmt, results.clone())))
    } else {
        Ok(None)
    }
}

pub fn write_results(
    results: &Vec<TaskExecutionResult>,
    fmt: ResultFormat,
    results_file: PathBuf,
) -> eyre::Result<()> {
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&results_file)
        .map_err(|e| {
            eyre::eyre!("failed to open results file '{results_file:?}': {e}")
        })?;

    match fmt {
        ResultFormat::Json => {
            serde_json::to_writer_pretty(&mut f, results)?;
        }
        ResultFormat::Yaml => {
            serde_yml::to_writer(&mut f, results)?;
        }
    }

    Ok(())
}

pub fn exit_code(results: &[TaskExecutionResult]) -> ExitCode {
    let has_error = results.iter().any(|r| r.is_failure());

    if has_error {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
