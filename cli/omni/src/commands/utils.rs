pub fn report_execution_results(
    results: &[crate::executor::TaskExecutionResult],
) {
    let mut skipped = 0;
    let mut errored = 0;
    let mut completed = 0;

    for res in results {
        if res.is_skipped() {
            skipped += 1;
        } else if res.is_error_before_complete() || !res.success() {
            errored += 1;
        } else if res.is_completed() && res.success() {
            completed += 1;
        }
    }

    if completed > 0 {
        trace::info!("Completed: {completed}");
    }
    if skipped > 0 {
        trace::info!("Skipped: {skipped}");
    }
    if errored > 0 {
        trace::info!("Errored: {errored}");
    }
}
