pub fn report_execution_results(
    results: &[crate::executor::TaskExecutionResult],
) {
    let skipped = results.iter().filter(|r| r.is_skipped()).count();
    let completed = results.iter().filter(|r| r.is_completed()).count();
    let errored = results
        .iter()
        .filter(|r| r.is_error_before_complete())
        .count();

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
