use omni_task_context::TaskContext;

use crate::TaskExecutionInfo;

pub trait TaskExecutionInfoExt<'a>: 'a {
    fn execution_info(&'a self) -> Option<TaskExecutionInfo<'a>>;
}

impl<'a> TaskExecutionInfoExt<'a> for TaskContext<'a> {
    fn execution_info(&'a self) -> Option<TaskExecutionInfo<'a>> {
        let ci = self.cache_info.as_ref()?;

        Some(TaskExecutionInfo {
            dependency_digests: &self.dependency_hashes,
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
