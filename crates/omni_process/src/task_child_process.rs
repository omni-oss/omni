use std::path::PathBuf;

use bytes::Bytes;
use derive_new::new;
use maps::Map;
use omni_core::TaskExecutionNode;
use system_traits::auto_impl;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::{ChildProcess, ChildProcessError};

pub struct TaskChildProcess {
    task: TaskExecutionNode,
    child_process: ChildProcess<String, PathBuf>,
}

impl TaskChildProcess {
    pub fn new(
        task: TaskExecutionNode,
        override_command: Option<String>,
    ) -> Self {
        let command =
            override_command.unwrap_or_else(|| task.task_command().to_string());
        let current_dir = task.project_dir().to_owned();
        Self {
            task,
            child_process: ChildProcess::new(command, current_dir),
        }
    }
}

#[auto_impl]
pub trait TaskExecutorWriter: AsyncWrite + Send {}

#[auto_impl]
pub trait TaskExecutorReader: AsyncRead + Send {}

#[derive(Debug, Clone, PartialEq, Eq, new)]
pub struct TaskChildProcessResult {
    #[new(into)]
    pub node: TaskExecutionNode,
    #[new(into)]
    pub exit_code: u32,
    #[new(into)]
    pub elapsed: std::time::Duration,
    #[new(into)]
    pub logs: Option<Bytes>,
}

impl TaskChildProcessResult {
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }

    pub fn exit_code(&self) -> u32 {
        self.exit_code
    }
}

impl TaskChildProcess {
    pub fn output_writer(
        &mut self,
        writer: impl TaskExecutorWriter + 'static,
    ) -> &mut Self {
        self.child_process.output_writer(writer);

        self
    }

    pub fn env_vars(&mut self, vars: &Map<String, String>) -> &mut Self {
        self.child_process.env_vars(vars);

        self
    }

    pub fn keep_stdin_open(&mut self, keep_stdin_open: bool) -> &mut Self {
        self.child_process.keep_stdin_open(keep_stdin_open);

        self
    }

    pub fn input_reader(
        &mut self,
        reader: impl TaskExecutorReader + 'static,
    ) -> &mut Self {
        self.child_process.input_reader(reader);

        self
    }

    pub fn record_logs(&mut self, record_logs: bool) -> &mut Self {
        self.child_process.record_logs(record_logs);

        self
    }

    #[tracing::instrument(skip_all, fields(task = self.task.full_task_name()))]
    pub async fn exec(
        self,
    ) -> Result<TaskChildProcessResult, ChildProcessError> {
        let task = self.task;
        let result = self.child_process.exec().await?;

        Ok(TaskChildProcessResult {
            node: task,
            exit_code: result.exit_code,
            elapsed: result.elapsed,
            logs: result.logs,
        })
    }
}
