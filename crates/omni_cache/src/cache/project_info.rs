use std::path::Path;

use derive_new::new;
use maps::Map;
use omni_types::OmniPath;
use yoke::Yokeable;

#[derive(Clone, Copy, PartialEq, Eq, Debug, new, Yokeable)]
pub struct ProjectInfo<'a> {
    pub name: &'a str,

    pub dir: &'a Path,

    pub output_files: &'a [OmniPath],

    pub input_files: &'a [OmniPath],

    pub input_env_cache_keys: &'a [String],

    pub env_vars: &'a Map<String, String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, new, Yokeable)]
pub struct CacheInfo<'a> {
    pub project: ProjectInfo<'a>,
    pub logs: Option<&'a str>,
}
