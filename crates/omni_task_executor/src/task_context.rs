use std::sync::Arc;

use omni_context::{CacheInfo, EnvVarsMap};
use omni_core::TaskExecutionNode;
use omni_hasher::impls::DefaultHash;

#[derive(Debug, Clone)]
pub struct TaskContext<'a> {
    pub node: &'a TaskExecutionNode,
    pub dependencies: &'a [String],
    pub dependency_hashes: Vec<DefaultHash>,
    pub env_vars: Arc<EnvVarsMap>,
    pub cache_info: Option<&'a CacheInfo>,
}
