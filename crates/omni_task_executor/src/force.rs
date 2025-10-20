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
pub enum Force {
    #[strum(serialize = "all")]
    All,
    #[strum(serialize = "failed")]
    Failed,
    #[strum(serialize = "none")]
    None,
}
