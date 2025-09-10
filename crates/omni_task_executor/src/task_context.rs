use std::sync::Arc;

use omni_cache::TaskExecutionInfo;
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

impl<'a> TaskContext<'a> {
    pub fn execution_info(&'a self) -> Option<TaskExecutionInfo<'a>> {
        let ci = self.cache_info?;
        Some(TaskExecutionInfo {
            dependency_hashes: &self.dependency_hashes,
            env_vars: &self.env_vars,
            input_env_keys: &ci.key_env_keys,
            input_files: &ci.key_input_files,
            output_files: &ci.cache_output_files,
            project_dir: self.node.project_dir(),
            project_name: self.node.project_name(),
            task_command: self.node.task_command(),
            task_name: self.node.task_name(),
        })
    }
}
