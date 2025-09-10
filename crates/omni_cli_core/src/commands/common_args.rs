use std::path::PathBuf;

use clap::{Args, ValueEnum};
use omni_task_executor::ExecutionConfigBuilder;

#[derive(Args, Debug)]
pub struct RunArgs {
    #[arg(
        short,
        long,
        help = "Filter the task/projects based on the meta configuration. Use the syntax of the CEL expression language"
    )]
    meta: Option<String>,
    #[arg(
        long,
        short,
        help = "Run the command based on the project name matching the filter"
    )]
    project: Option<String>,

    #[arg(long, short = 'c', help = "How many concurrent tasks to run")]
    max_concurrency: Option<usize>,

    #[arg(
        long,
        short,
        action = clap::ArgAction::SetTrue,
        default_value_t = false,
        help = "Don't execute the task, just print the command to be executed"
    )]
    dry_run: bool,

    #[arg(
        long,
        help = "Output the results of the execution to the specified file"
    )]
    pub(crate) result: Option<PathBuf>,

    #[arg(
        long,
        help = "Output the results of the execution in the specified format, if not specified, the format will be inferred from the file extension",
        value_enum
    )]
    pub(crate) result_format: Option<ResultFormat>,
}

#[derive(ValueEnum, Debug, Clone, Copy)]
pub enum ResultFormat {
    Json,
    Yaml,
    Toml,
}

impl RunArgs {
    pub fn apply_to(&self, builder: &mut ExecutionConfigBuilder) {
        if let Some(meta) = &self.meta {
            builder.meta_filter(meta);
        }

        if let Some(project) = &self.project {
            builder.project_filter(project);
        }

        if let Some(max_concurrency) = self.max_concurrency {
            builder.max_concurrency(max_concurrency);
        }

        builder.dry_run(self.dry_run);
    }
}
