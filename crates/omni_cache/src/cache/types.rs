use std::path::Path;

use bytes::Bytes;
use derive_new::new;
use maps::Map;
use omni_hasher::impls::DefaultHash;
use omni_types::OmniPath;
use yoke::Yokeable;

#[allow(clippy::too_many_arguments)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, new, Yokeable)]
pub struct TaskExecutionInfo<'a> {
    pub task_name: &'a str,
    pub task_command: &'a str,
    pub project_name: &'a str,
    pub project_dir: &'a Path,
    pub output_files: &'a [OmniPath],
    pub input_files: &'a [OmniPath],
    pub input_env_keys: &'a [String],
    pub env_vars: &'a Map<String, String>,
    pub dependency_hashes: &'a [DefaultHash],
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, new, Yokeable)]
pub struct NewCacheInfo<'a> {
    pub task: TaskExecutionInfo<'a>,
    pub logs: Option<&'a Bytes>,
    pub execution_time: std::time::Duration,
    pub exit_code: i32,
}
