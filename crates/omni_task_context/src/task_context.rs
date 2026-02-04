use std::{borrow::Cow, sync::Arc};

use omni_core::TaskExecutionNode;
use omni_hasher::impls::DefaultHash;

use crate::{CacheInfo, aliases::EnvVars};

#[derive(Debug, Clone)]
pub struct TaskContext<'a> {
    pub node: &'a TaskExecutionNode,
    pub dependency_hashes: Vec<DefaultHash>,
    pub env_vars: Arc<EnvVars>,
    pub cache_info: Option<Cow<'a, CacheInfo>>,
    pub template_context: omni_tera::Context,
}
