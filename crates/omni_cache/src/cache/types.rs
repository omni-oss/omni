use std::path::{Path, PathBuf};

use bytes::Bytes;
use bytesize::ByteSize;
use derive_new::new;
use maps::Map;
use omni_command_config::CommandConfig;
use omni_hasher::impls::DefaultHash;
use omni_types::OmniPath;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use yoke::Yokeable;

#[allow(clippy::too_many_arguments)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, new, Yokeable, Serialize)]
pub struct TaskExecutionInfo<'a> {
    pub task_name: &'a str,
    #[serde(alias = "task_command")]
    pub task_exec: Option<&'a CommandConfig>,
    #[serde(alias = "task_retry_command")]
    pub task_retry_exec: Option<&'a CommandConfig>,
    pub project_name: &'a str,
    pub project_dir: &'a Path,
    pub output_files: &'a [OmniPath],
    pub output_files_exclude: &'a [OmniPath],
    pub input_files: &'a [OmniPath],
    pub input_files_exclude: &'a [OmniPath],
    pub input_env_keys: &'a [String],
    pub env_vars: &'a Map<String, String>,
    pub dependency_digests: &'a [DefaultHash],
    /// The merged args: configured overlaid with the invocation overrides.
    pub args: &'a Map<String, serde_json::Value>,
    /// The digest of the task's effective definition. Not optional: a new
    /// constructor must supply it, so no key can be built without it.
    pub effective_digest: DefaultHash,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, new)]
pub struct NewCacheInfo<'a> {
    pub task: TaskExecutionInfo<'a>,
    pub logs: Option<&'a Bytes>,
    pub execution_duration: std::time::Duration,
    pub exit_code: u32,
    pub tries: u16,
}

#[derive(
    Clone, PartialEq, Eq, Debug, new, Serialize, Deserialize, JsonSchema,
)]
pub struct PrunedCacheEntry {
    pub project_name: String,
    pub task_name: String,
    pub digest: DefaultHash,
    #[schemars(with = "String")]
    pub size: ByteSize,
    pub entry_dir: PathBuf,
    pub stale: StaleStatus,
}

#[derive(
    Clone, Copy, PartialEq, Eq, Debug, new, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum StaleStatus {
    Unknown,
    Stale,
    Fresh,
}
