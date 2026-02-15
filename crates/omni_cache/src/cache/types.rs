use std::path::{Path, PathBuf};

use bytes::Bytes;
use bytesize::ByteSize;
use derive_new::new;
use maps::Map;
use omni_hasher::impls::DefaultHash;
use omni_types::OmniPath;
use serde::{Deserialize, Serialize};
use yoke::Yokeable;

#[allow(clippy::too_many_arguments)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, new, Yokeable, Serialize)]
pub struct TaskExecutionInfo<'a> {
    pub task_name: &'a str,
    pub task_command: &'a str,
    pub project_name: &'a str,
    pub project_dir: &'a Path,
    pub output_files: &'a [OmniPath],
    pub input_files: &'a [OmniPath],
    pub input_env_keys: &'a [String],
    pub env_vars: &'a Map<String, String>,
    pub dependency_digests: &'a [DefaultHash],
    pub args: &'a Map<String, serde_json::Value>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, new)]
pub struct NewCacheInfo<'a> {
    pub task: TaskExecutionInfo<'a>,
    pub logs: Option<&'a Bytes>,
    pub execution_duration: std::time::Duration,
    pub exit_code: u32,
    pub tries: u8,
}

#[derive(Clone, PartialEq, Eq, Debug, new, Serialize, Deserialize)]
pub struct PrunedCacheEntry {
    pub project_name: String,
    pub task_name: String,
    pub digest: DefaultHash,
    pub size: ByteSize,
    pub entry_dir: PathBuf,
    pub stale: StaleStatus,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, new, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StaleStatus {
    Unknown,
    Stale,
    Fresh,
}
