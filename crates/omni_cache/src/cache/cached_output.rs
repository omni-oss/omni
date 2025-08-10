use std::path::PathBuf;

use derive_new::new;
use omni_hasher::impls::DefaultHash;
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
    Default,
)]
#[allow(clippy::too_many_arguments)]
pub struct CachedTaskExecution {
    #[new(into)]
    pub project_name: String,

    #[new(into)]
    pub task_name: String,

    #[new(into)]
    pub task_command: String,

    #[new(into)]
    pub execution_hash: DefaultHash,

    #[new(into)]
    /// Location of the cached output
    pub logs_path: Option<PathBuf>,

    #[new(into)]
    pub files: Vec<CachedFileOutput>,

    #[new(into)]
    pub exit_code: i32,

    #[new(into)]
    pub execution_time: std::time::Duration,
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
