use std::process::ExitCode;

use clap::Args;
use clap_utils::EnumValueAdapter;
use omni_execution_plan::TaskSelector;
use omni_task_executor::{ExecutionConfigBuilder, Force};

use crate::{
    commands::{
        common_args::RunArgs,
        utils::{exit_code, get_results_settings},
    },
    context::Context,
    executor::{Call, OnFailure, TaskExecutor},
};

use super::{OutputOptions, utils::resolve_subscriber};

#[derive(Args)]
pub struct RunCommand {
    #[arg(
        required_unless_present = "task_pattern",
        help = "Exact name of a task to run. Glob characters are literal; use --task-pattern to select by glob"
    )]
    pub task: Vec<String>,

    #[arg(
        long,
        value_name = "GLOB",
        help = "Run every task whose name matches this glob. Repeatable, and may be combined with exact task names"
    )]
    pub task_pattern: Vec<String>,

    #[arg(
        long,
        alias = "ignore-deps",
        short,
        help = "Run the command without dependencies. Not compatible with --with-dependents",
        conflicts_with = "with_dependents",
        default_value_t = false
    )]
    pub ignore_dependencies: bool,

    #[arg(
        long,
        short,
        help = "Run the task along with its dependents. Useful for combining with filters. Not compatible with --ignore-deps",
        default_value_t = false
    )]
    pub with_dependents: bool,

    #[arg(
        long,
        short,
        help = "How to handle failures",
        default_value_t = EnumValueAdapter::new(OnFailure::SkipDependents),
        value_enum
    )]
    pub on_failure: EnumValueAdapter<OnFailure>,

    #[arg(
        long,
        help = "Don't save the execution result to the cache",
        default_value_t = false
    )]
    pub no_cache: bool,

    #[arg(
        long,
        help = "Don't read from or write to the remote cache for any task",
        default_value_t = false
    )]
    pub no_remote_cache: bool,

    #[arg(
        long,
        short,
        num_args = 0..=1,
        help = "Force execution of tasks, even if it's already cached",
        default_value_t = EnumValueAdapter::new(Force::None),
        default_missing_value = "all",
        value_enum
    )]
    pub force: EnumValueAdapter<Force>,

    #[command(flatten)]
    pub run: RunArgs,
}

pub async fn run(
    command: &RunCommand,
    ctx: &Context,
    output: OutputOptions,
) -> eyre::Result<ExitCode> {
    let output_settings = get_results_settings(&command.run)?;

    let mut builder = ExecutionConfigBuilder::default();

    if output_settings.is_some() {
        builder.add_task_details(true);
    }

    builder
        .ignore_dependencies(command.ignore_dependencies)
        .with_dependents(command.with_dependents)
        .on_failure(command.on_failure.value())
        .no_cache(command.no_cache)
        .no_remote_cache(command.no_remote_cache)
        .force(command.force.value())
        .call(Call::Tasks(
            command
                .task
                .iter()
                .map(TaskSelector::exact)
                .chain(command.task_pattern.iter().map(TaskSelector::pattern))
                .collect(),
        ));

    command
        .run
        .apply_to(&mut builder, ctx.workspace_configuration());

    let config = builder.build()?;

    let ctx = ctx.clone().into_loaded().await?;
    let sub = resolve_subscriber(command.run.ui, ctx.scratch_dir(), output);
    let executor = TaskExecutor::new(config, &ctx, &sub);

    let results = match executor.run().await {
        Ok(results) => results,
        Err(err) => {
            sub.wait().await;
            return Err(err.into());
        }
    };

    sub.wait().await;

    // report_execution_results is now handled by CliSubscriber::on_execution_complete

    if let Some((fmt, results_file_path)) = output_settings {
        omni_file_data_serde::write_with_format_async(
            fmt.to_serde_format(),
            &results_file_path,
            &results,
            ctx.sys(),
        )
        .await?;
    }

    Ok(exit_code(&results))
}

#[cfg(test)]
mod tests {
    use clap::{Parser, error::ErrorKind};

    use super::*;

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        run: RunCommand,
    }

    fn parse(args: &[&str]) -> Result<RunCommand, clap::Error> {
        let mut argv = vec!["run"];
        argv.extend_from_slice(args);
        TestCli::try_parse_from(argv).map(|cli| cli.run)
    }

    #[test]
    fn positional_tasks_are_kept_verbatim() {
        let run = parse(&["build*", "a#b"]).unwrap();
        assert_eq!(run.task, ["build*", "a#b"]);
        assert!(run.task_pattern.is_empty());
    }

    #[test]
    fn task_patterns_repeat_and_may_stand_alone() {
        let run = parse(&["--task-pattern", "build*", "--task-pattern", "t*"])
            .unwrap();
        assert!(run.task.is_empty());
        assert_eq!(run.task_pattern, ["build*", "t*"]);
    }

    #[test]
    fn exact_and_pattern_selectors_mix() {
        let run = parse(&["lint", "--task-pattern", "test*"]).unwrap();
        assert_eq!(run.task, ["lint"]);
        assert_eq!(run.task_pattern, ["test*"]);
    }

    #[test]
    fn some_selector_is_required() {
        let error = parse(&[]).err().expect("no selector must fail");
        assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);
    }
}
