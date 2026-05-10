use std::path::PathBuf;

use clap::Args;
use maps::{map, unordered_map};
use omni_generator::RunConfig;
use system_traits::{EnvCurrentDirAsync, impls::RealSys};
use tempfile::tempdir;
use value_bag::ValueBag;

use crate::commands::{
    generator_common_args::GeneratorRunCommonArgs,
    generator_utils::{get_prompt_values, prompt_generator_name},
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

    let generators =
        omni_generator::discover(dir.path(), &["**"], &sys).await?;

    if generators.is_empty() {
        log::error!(
            "No generators found in the provided source. Please ensure the repository contains valid generators."
        );

        return Ok(());
    }

    log::info!("Found {} generator(s) in the source.", generators.len());

    let generator = if generators.len() == 1 {
        &generators[0]
    } else {
        let gen_name = prompt_generator_name(&generators)?;

        let generator = generators.iter().find(|g| g.name == gen_name);

        if let Some(generator) = generator {
            generator
        } else {
            log::error!(
                "Selected generator '{}' not found. Please select a valid generator.",
                gen_name
            );

            return Ok(());
        }
    };

    let pre_exec_values = get_prompt_values(&command.args.common.value);

    let context_values = unordered_map! {
        "output_dir".to_string() => ValueBag::from_serde1(&output_dir.to_string_lossy()).to_owned(),
        "current_dir".to_string() => ValueBag::from_serde1(&current_dir).to_owned(),
    };

    let run_config = RunConfig {
        available_generators: &generators,
        output_dir: output_dir,
        workspace_dir: output_dir,
        current_dir: &current_dir,
        target_overrides: &unordered_map!(),
        context_values: &context_values,
        env: &map!(),
        dry_run: false,
        args: None,
        overwrite: None,
        use_prompt_defaults: command.args.common.use_defaults,
        prompt_values: &pre_exec_values,
    };

    omni_generator::run(generator, &run_config, &sys).await?;

    Ok(())
}
