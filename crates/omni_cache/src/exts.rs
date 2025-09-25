use omni_task_context::TaskContext;

use crate::TaskExecutionInfo;

pub trait TaskExecutionInfoExt {
    fn execution_info(&self) -> Option<TaskExecutionInfo<'_>>;
}

impl<'a> TaskExecutionInfoExt for TaskContext<'a> {
    fn execution_info(&self) -> Option<TaskExecutionInfo<'_>> {
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
