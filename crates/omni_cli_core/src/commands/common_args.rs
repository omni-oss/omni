use clap::Args;

use crate::executor::TaskExecutorBuilder;

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
}

impl RunArgs {
    pub fn apply_to(&self, builder: &mut TaskExecutorBuilder) {
        if let Some(meta) = &self.meta {
            builder.meta_filter(meta);
        }

        if let Some(project) = &self.project {
            builder.project_filter(project);
        }

        if let Some(max_concurrency) = self.max_concurrency {
            builder.max_concurrency(max_concurrency);
        }
    }
}
