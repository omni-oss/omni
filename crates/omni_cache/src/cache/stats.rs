use std::{path::PathBuf, time::Duration};

use bytesize::ByteSize;
use omni_hasher::impls::DefaultHash;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct CacheStats {
    pub projects: Vec<ProjectCacheStats>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct ProjectCacheStats {
    pub project_name: String,
    pub tasks: Vec<TaskCacheStats>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct TaskCacheStats {
    pub task_name: String,
    pub entry_dir: PathBuf,

    pub log_file: Option<FileCacheStats>,
    pub cached_files: Vec<FileCacheStats>,
    pub meta_file: FileCacheStats,
    pub execution_duration: Duration,

    pub total_size: ByteSize,
    pub cached_files_total_size: ByteSize,

    pub digest: DefaultHash,

    #[serde(with = "time::serde::rfc3339")]
    pub created_timestamp: OffsetDateTime,

    #[serde(with = "time::serde::rfc3339::option")]
    pub last_used_timestamp: Option<OffsetDateTime>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct FileCacheStats {
    pub path: String,
    pub size: ByteSize,
}
