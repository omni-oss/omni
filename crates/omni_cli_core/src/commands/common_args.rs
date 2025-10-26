use std::path::PathBuf;

use clap::{Args, ValueEnum};
use clap_utils::EnumValueAdapter;
use omni_configurations::{Ui, WorkspaceConfiguration};
use omni_execution_plan::ScmAffectedFilter;
use omni_scm::SelectScm;
use omni_task_executor::ExecutionConfigBuilder;

#[derive(Args, Debug)]
pub struct RunArgs {
    #[arg(
        short,
        long,
        help = "Filter the task/projects based on the meta configuration, accepts CEL syntax"
    )]
    pub meta: Option<String>,

    #[arg(
        long,
        short,
        help = "Filter based on the project name matching the passed argument, accepts glob patterns"
    )]
    pub project: Vec<String>,

    #[arg(
        long,
        help = "Filter based on projects residing in the specified directories, accepts glob patterns"
    )]
    pub dir: Vec<String>,

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

    #[arg(
        long,
        alias = "base",
        short = 'b',
        help = "The base commit to compare against. This will implicitly enable --scm-affected"
    )]
    pub scm_base: Option<String>,

    #[arg(
        long,
        alias = "target",
        short = 't',
        help = "The target commit to compare against. This will implicitly enable --scm-affected"
    )]
    pub scm_target: Option<String>,

    #[arg(
        long,
        alias = "affected",
        short = 'a',
        default_value_t = EnumValueAdapter::new(SelectScm::None),
        default_missing_value = "auto",
        value_enum,
        num_args = 0..=1,
        help = "Enable scm-based filtering of tasks. Optionally specify the scm to use for detecting affected tasks",
    )]
    pub scm_affected: EnumValueAdapter<SelectScm>,
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
        builder.dir_filters(self.dir.clone());

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

        let scm = self.scm_affected.value();
        if !scm.is_none() {
            let base = self.scm_base.as_ref().map(|s| s.to_string());
            let target = self.scm_target.as_ref().map(|s| s.to_string());
            let filter = ScmAffectedFilter { base, scm, target };

            builder.scm_affected_filter(filter);
        }
    }
}
