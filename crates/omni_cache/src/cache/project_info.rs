use std::path::Path;

use derive_new::new;
use maps::Map;
use yoke::Yokeable;

#[derive(Clone, Copy, PartialEq, Eq, Debug, new, Yokeable)]
pub struct ProjectInfo<'a> {
    pub name: &'a str,

    pub dir: &'a Path,

    pub output_files: &'a [&'a Path],

    pub files: &'a [&'a Path],

    pub env_vars: &'a Map<String, String>,

    pub env_cache_keys: &'a [String],
}
