use std::path::PathBuf;

use derive_new::new;
use omni_hasher::impls::DefaultHash;
use omni_types::OmniPath;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

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
#[allow(clippy::too_many_arguments)]
pub struct CachedTaskExecution {
    #[new(into)]
    pub project_name: String,

    #[new(into)]
    pub task_name: String,

    #[new(into)]
    pub task_command: String,

    #[new(into)]
    #[serde(alias = "execution_hash")]
    pub digest: DefaultHash,

    #[new(into)]
    #[serde(alias = "dependency_hashes")]
    pub dependency_digests: Vec<DefaultHash>,

    #[new(into)]
    /// Location of the cached output
    pub logs_path: Option<PathBuf>,

    #[new(into)]
    pub files: Vec<CachedFileOutput>,

    #[new(into)]
    pub exit_code: u32,

    #[new(into)]
    pub execution_duration: std::time::Duration,

    #[new(into)]
    pub execution_time: OffsetDateTime,
}

#[derive(
    Debug,
    Copy,
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
pub struct CachedTaskExecutionHash<'a> {
    #[new(into)]
    pub project_name: &'a str,

    #[new(into)]
    pub task_name: &'a str,

    #[new(into)]
    pub digest: DefaultHash,
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
