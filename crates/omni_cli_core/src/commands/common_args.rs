use std::path::PathBuf;

use clap::{Args, ValueEnum};
use clap_utils::EnumValueAdapter;
use omni_configurations::{Ui, WorkspaceConfiguration};
use omni_task_executor::ExecutionConfigBuilder;

#[derive(Args, Debug)]
pub struct RunArgs {
    #[arg(
        short,
        long,
        help = "Filter the task/projects based on the meta configuration. Use the syntax of the CEL expression language"
    )]
    pub meta: Option<String>,

    #[arg(
        long,
        short,
        help = "Run the command based on the project name matching the filter"
    )]
    pub project: Vec<String>,

    #[arg(long, short = 'c', help = "How many concurrent tasks to run")]
    pub max_concurrency: Option<usize>,

    #[arg(
        long,
        short,
        action = clap::ArgAction::SetTrue,
        default_value_t = false,
        help = "Don't execute the task, just print the command to be executed"
    )]
    pub dry_run: bool,

    #[arg(
        long,
        help = "Output the results of the execution to the specified file"
    )]
    pub result: Option<PathBuf>,

    #[arg(
        long,
        help = "Output the results of the execution in the specified format, if not specified, the format will be inferred from the file extension",
        value_enum
    )]
    pub result_format: Option<ResultFormat>,

    #[arg(
        long,
        short,
        help = "Ui mode to use while running the command",
        value_enum
    )]
    pub ui: Option<EnumValueAdapter<Ui>>,
}

#[derive(ValueEnum, Debug, Clone, Copy)]
pub enum ResultFormat {
    Json,
    Yaml,
    Toml,
}

impl RunArgs {
    pub fn apply_to(
        &self,
        builder: &mut ExecutionConfigBuilder,
        ws_config: &WorkspaceConfiguration,
    ) {
        if let Some(meta) = &self.meta {
            builder.meta_filter(meta);
        }

        builder.project_filters(self.project.clone());

        if let Some(max_concurrency) = self.max_concurrency {
            builder.max_concurrency(max_concurrency);
        }

        if let Some(ui) = self.ui {
            builder.ui(ui.value());
        } else {
            builder.ui(ws_config.ui);
        }

        // additional check if tty is available, since `Ui::Tui` is only available if tty is available
        if !atty::is(atty::Stream::Stdout) {
            builder.ui(Ui::Stream);
        }

        builder.dry_run(self.dry_run);
    }
}
