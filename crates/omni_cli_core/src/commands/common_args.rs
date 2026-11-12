use std::path::PathBuf;

use clap::Args;
use clap_utils::EnumValueAdapter;
use config_utils::Replace;
use maps::UnorderedMap;
use omni_configurations::{
    RedactMode, RedactOverrides, Ui, WorkspaceConfiguration,
};
use omni_execution_plan::{MetaOnError, ScmAffectedFilter};
use omni_retry_config::RetryConfiguration;
use omni_scm::SelectScm;
use omni_task_executor::ExecutionConfigBuilder;
use omni_task_output_logs::LogsDisplay;

use crate::commands::{
    common_types::SerializationFormat,
    parser::{parse_key_value, parse_retry_spec},
};

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
        help = "What to do when the --meta filter fails to evaluate for a task: fail with a diagnostic, or exclude the task and warn",
        value_enum,
        default_value_t = EnumValueAdapter::new(MetaOnError::Fail),
    )]
    pub meta_on_error: EnumValueAdapter<MetaOnError>,

    #[arg(
        long,
        help = "How many failing tasks to list per distinct --meta error cause",
        default_value_t = 3
    )]
    pub meta_error_samples: usize,

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
    pub result_format: Option<SerializationFormat>,

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
        short = 'r',
        value_name = "SPEC",
        value_parser = parse_retry_spec,
        help = "Retry policy: COUNT[,strategy][,key=value...], e.g. 3,exponential,base=500ms,max_delay=30s,jitter=full. Merged per field over the `retry` task configuration"
    )]
    pub retry: Option<RetryConfiguration>,

    #[arg(long, conflicts_with = "retry", help = "Disable retries")]
    pub no_retry: bool,

    #[arg(
        long,
        alias = "affected",
        default_value_t = EnumValueAdapter::new(SelectScm::None),
        default_missing_value = "auto",
        value_enum,
        num_args = 0..=1,
        help = "Enable scm-based filtering of tasks. Optionally specify the scm to use for detecting affected tasks",
    )]
    pub scm_affected: EnumValueAdapter<SelectScm>,

    #[arg(
        short = 'a',
        long = "arg",
        help = "Pass arguments to the commands invoked",
        value_parser = parse_key_value::<String, String>,
    )]
    pub args: Vec<(String, String)>,

    #[arg(
        long = "redact-arg",
        value_name = "NAME",
        help = "Redact the value of this arg (name or glob) in logs and dry-run output. Repeatable"
    )]
    pub redact_arg: Vec<String>,

    #[arg(
        long = "redact-env",
        value_name = "NAME",
        help = "Redact the value of this environment variable (name or glob) in logs and dry-run output. Repeatable"
    )]
    pub redact_env: Vec<String>,

    #[arg(
        long = "redact",
        value_name = "MODE",
        value_enum,
        help = "Redaction mode: declared (apply configured policy), all, or none (prints a notice)"
    )]
    pub redact: Option<RedactModeArg>,

    #[arg(
        long,
        short,
        help = "Ui mode to use while running the command",
        value_enum
    )]
    pub ui: Option<EnumValueAdapter<Ui>>,

    #[arg(
        long,
        help = "Which task output logs to display: all, failed, or never. Applies to both fresh and cached output",
        value_enum
    )]
    pub output_logs: Option<EnumValueAdapter<LogsDisplay>>,

    #[arg(
        long,
        help = "Which cached task output logs to replay: all, failed, or never. Overrides --output-logs for the cached facet",
        value_enum
    )]
    pub output_cached_logs: Option<EnumValueAdapter<LogsDisplay>>,
}

/// `--redact` values; see [`RedactMode`].
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum RedactModeArg {
    Declared,
    All,
    None,
}

impl From<RedactModeArg> for RedactMode {
    fn from(value: RedactModeArg) -> Self {
        match value {
            RedactModeArg::Declared => RedactMode::Declared,
            RedactModeArg::All => RedactMode::All,
            RedactModeArg::None => RedactMode::None,
        }
    }
}

impl RunArgs {
    pub fn apply_to(
        &self,
        builder: &mut ExecutionConfigBuilder,
        _ws_config: &WorkspaceConfiguration,
    ) {
        if let Some(meta) = &self.meta {
            builder.meta_filter(meta);
        }

        builder.meta_on_error(self.meta_on_error.value());
        builder.meta_error_samples(self.meta_error_samples);

        builder.project_filters(self.project.clone());
        builder.dir_filters(self.dir.clone());

        if let Some(max_concurrency) = self.max_concurrency {
            builder.max_concurrency(max_concurrency);
        }

        builder.dry_run(self.dry_run);

        if let Some(retry) = &self.retry {
            builder.retry(retry.clone());
        } else if self.no_retry {
            builder.retry(RetryConfiguration {
                max_retries: Some(Replace::new(0)),
                ..Default::default()
            });
        }

        let mut scm = self.scm_affected.value();
        // `--scm-base`/`--scm-target` implicitly enable scm filtering (using the
        // auto-detected scm) even when `--scm-affected` was not passed.
        if scm.is_none()
            && (self.scm_base.is_some() || self.scm_target.is_some())
        {
            scm = SelectScm::Auto;
        }
        if !scm.is_none() {
            let base = self.scm_base.as_ref().map(|s| s.to_string());
            let target = self.scm_target.as_ref().map(|s| s.to_string());
            let filter = ScmAffectedFilter { base, scm, target };

            builder.scm_affected_filter(filter);
        }

        if !self.args.is_empty() {
            let hm = self
                .args
                .iter()
                .cloned()
                .map(|(k, v)| (k, serde_json::Value::String(v)))
                .collect::<UnorderedMap<_, _>>();

            builder.args(hm);
        }

        builder.redact(RedactOverrides {
            mode: self.redact.map(Into::into),
            args: self.redact_arg.clone(),
            inputs: vec![],
            env: self.redact_env.clone(),
        });

        if let Some(output_logs) = &self.output_logs {
            builder.output_logs(output_logs.value());
        }

        if let Some(output_cached_logs) = &self.output_cached_logs {
            builder.output_cached_logs(output_cached_logs.value());
        }
    }
}
