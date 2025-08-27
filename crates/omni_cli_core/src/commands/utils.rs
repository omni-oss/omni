use std::time::Duration;

use owo_colors::OwoColorize as _;
use tiny_gradient::{Gradient, GradientStr};

use crate::executor::TaskExecutionResult;

pub fn report_execution_results(
    results: &[crate::executor::TaskExecutionResult],
) {
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

        if let TaskExecutionResult::CacheHit {
            result: execution, ..
        } = res
        {
            if execution.success() {
                cached_success += 1;
            } else {
                cached_error += 1;
            }
            total_saved_time += execution.elapsed;
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
