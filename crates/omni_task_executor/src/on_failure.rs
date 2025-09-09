use strum::{Display, EnumIs};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, EnumIs, Display,
)]
pub enum OnFailure {
    #[strum(to_string = "continue")]
    Continue,
    #[strum(to_string = "skip-next-batches")]
    SkipNextBatches,
    #[strum(to_string = "skip-dependents")]
    SkipDependents,
}
