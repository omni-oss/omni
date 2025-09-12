use strum::{Display, EnumIs, VariantArray};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    EnumIs,
    Display,
    VariantArray,
)]
pub enum OnFailure {
    #[strum(serialize = "continue")]
    Continue,
    #[strum(serialize = "skip-next-batches")]
    SkipNextBatches,
    #[strum(serialize = "skip-dependents")]
    SkipDependents,
}
