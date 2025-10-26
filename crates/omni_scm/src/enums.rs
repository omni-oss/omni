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
pub enum SelectScm {
    /// Use the auto-detected scm
    #[strum(serialize = "auto")]
    Auto,
    /// Use git as the scm
    #[strum(serialize = "git")]
    Git,
    /// Don't use any scm
    #[strum(serialize = "none")]
    None,
}
