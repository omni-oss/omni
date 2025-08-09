use std::path::PathBuf;

use derive_new::new;
use omni_types::OmniPath;
use serde::{Deserialize, Serialize};

#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    new,
)]
pub struct CachedOutput {
    #[new(into)]
    /// Location of the cached output, can be a local file or a remote url
    pub logs_path: Option<PathBuf>,

    #[new(into)]
    pub files: Vec<CachedFileOutput>,
}

#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    new,
)]
pub struct CachedFileOutput {
    /// Canonical path to the cached file
    #[new(into)]
    pub cached_path: PathBuf,

    /// Canonical path to the original file
    #[new(into)]
    pub original_path: OmniPath,
}
