use std::path::PathBuf;

use clap::Args;
use eyre::Ok;
use maps::{map, unordered_map};
use omni_generator::RunConfig;
use omni_prompt::CliInputProvider;
use system_traits::{EnvCurrentDirAsync, impls::RealSys};
use tempfile::tempdir;
use value_bag::ValueBag;

use crate::commands::{
    generator_common_args::GeneratorRunCommonArgs,
    generator_utils::get_input_values,
};

#[derive(Args, Debug)]
pub struct InitCommand {
    #[command(flatten)]
    pub args: InitArgs,
}

#[derive(Args, Debug)]
pub struct InitArgs {
    #[arg(
        group = "source",
        short,
        long,
        help = "The git repository URL to clone for initializing the workspace"
    )]
    git: Option<String>,

    #[arg(
        short,
        long,
        help = "The output directory for the initialized workspace"
    )]
    output: Option<PathBuf>,

    #[command(flatten)]
    common: GeneratorRunCommonArgs,
}

pub async fn run(command: &InitCommand) -> eyre::Result<()> {
    let dir = tempdir()?;
    let sys = RealSys;
    let current_dir = sys.env_current_dir_async().await?;
    let output_dir = command.args.output.as_deref().unwrap_or(&current_dir);
    if let Some(url) = &command.args.git {
        log::info!("Cloning repository from {}...", url);
        omni_git_utils::clone_repo(&sys, &url, None, dir.path()).await?;
        log::info!("Repository cloned successfully.");
    } else {
        log::error!(
            "No source provided for initialization. Please provide a git repository using the --git option."
        );

        return Ok(());
    }

    let primary_generator =
        omni_generator::discover_one_in_dir(dir.path(), &sys).await?;

    let generator = if let Some(generator) = primary_generator {
        generator
    } else {
        log::error!(
            "No primary generator is found, generators used as initializer must have one generator config in the root of the source folder",
        );
        return Ok(());
    };

    let all_generators =
        omni_generator::discover(dir.path(), &["**"], &sys).await?;

    log::info!("Found {} generator(s) in the source.", all_generators.len());

    let pre_exec_values = get_input_values(&command.args.common.value);

    let context_values = unordered_map! {
        "output_dir".to_string() => ValueBag::from_serde1(&output_dir.to_string_lossy()).to_owned(),
        "current_dir".to_string() => ValueBag::from_serde1(&current_dir).to_owned(),
    };

    let input_provider = CliInputProvider::default();
    let run_config = RunConfig {
        available_generators: &all_generators[..],
        output_dir: output_dir,
        workspace_dir: output_dir,
        current_dir: &current_dir,
        target_overrides: &unordered_map!(),
        context_values: &context_values,
        env: &map!(),
        dry_run: false,
        args: None,
        overwrite: None,
        use_inputs_defaults: command.args.common.use_defaults,
        input_values: &pre_exec_values,
        input_provider: &input_provider,
    };

    omni_generator::run(&generator, &run_config, &sys).await?;

    Ok(())
}
