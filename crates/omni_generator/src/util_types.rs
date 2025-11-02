use std::path::{Path, PathBuf};

use strum::EnumDiscriminants;

#[derive(Debug, Clone, Copy, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(TemplateType))]
pub enum Template<'a> {
    Inline(&'a str),
    File(&'a Path),
    Files(&'a [PathBuf]),
}
