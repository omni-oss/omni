use omni_configurations::Ui;
use omni_core::BatchedExecutionPlan;

pub(crate) fn should_use_tui(ui: Ui, plan: &BatchedExecutionPlan) -> bool {
    match ui {
        omni_configurations::Ui::Stream => false,
        omni_configurations::Ui::Tui => {
            if atty::is(atty::Stream::Stdout) {
                true
            } else {
                false
            }
        }
        omni_configurations::Ui::Auto => {
            if plan.iter().any(|b| b.iter().any(|t| t.interactive()))
                && atty::is(atty::Stream::Stdout)
            {
                true
            } else {
                false
            }
        }
    }
}
