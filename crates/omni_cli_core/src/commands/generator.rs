use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::commands::{
    generator_common_args::GeneratorRunCommonArgs,
    generator_utils::{get_prompt_values, prompt_generator_name},
};

use super::parser::parse_key_value;
use clap_utils::EnumValueAdapter;
use either::Left;
use maps::{Map, UnorderedMap, unordered_map};
use omni_configurations::{GeneratorSourceConfiguration, types::SingleOrMany};
use omni_context::Context;
use omni_core::Project;
use omni_generator::{GenSession, GeneratorSys, RunConfig};
use omni_generator_configurations::{
    GeneratorConfiguration, OmniPath, OverwriteConfiguration,
};
use omni_prompt::configuration::{
    BasePromptConfiguration, ConfirmPromptConfiguration, OptionConfiguration,
    PromptConfiguration, PromptingConfiguration, SelectPromptConfiguration,
    TextPromptConfiguration, ValidatedPromptConfiguration,
};
use omni_remote_sources::manager::{
    RemoteSourceManager, config::RemoteSourceConfig,
};
use owo_colors::OwoColorize;
use system_traits::{FsCreateDirAllAsync, FsMetadataAsync};
use tokio::task::JoinSet;
use value_bag::ValueBag;

#[derive(Debug, Clone, clap::Args)]
pub struct GeneratorCommand {
    #[command(subcommand)]
    pub subcommand: GeneratorSubcommand,
}

#[derive(Debug, Clone, clap::Subcommand)]
pub enum GeneratorSubcommand {
    Run(#[command(flatten)] GeneratorRunCommand),

    #[command(alias = "ls")]
    List(#[command(flatten)] GeneratorListCommand),
}

#[derive(Debug, Clone, clap::Args)]
pub struct GeneratorRunCommand {
    #[command(flatten)]
    pub args: GeneratorRunArgs,
}

#[derive(Debug, Clone, clap::Args)]
pub struct GeneratorRunArgs {
    #[arg(long = "name", short = 'n', help = "Generator name")]
    pub name: Option<String>,

    #[arg(
        long,
        short,
        help = "If provided, it will use the project's directory as output path",
        conflicts_with = "output"
    )]
    pub project: Option<String>,

    #[arg(long, short, help = "Output path")]
    pub output: Option<PathBuf>,

    #[arg(
        long,
        short,
        help = "Override target paths",
        value_parser = parse_key_value::<String, OmniPath>
    )]
    pub target: Vec<(String, OmniPath)>,

    #[arg(
        long,
        short,
        help = "Dry run",
        default_value_t = false,
        action = clap::ArgAction::SetTrue
    )]
    pub dry_run: bool,

    #[arg(
        long,
        help = "How to handle overwriting existing files, takes precedence over the generator's configuration",
        value_enum
    )]
    pub overwrite: Option<EnumValueAdapter<OverwriteConfiguration>>,

    #[arg(
        long,
        num_args(0..=1),
        require_equals(true),
        help = "Save prompts and targets to the output directory so they can be reused future invocations",
        default_missing_value = "true"
    )]
    pub save_session: Option<bool>,

    #[arg(
        long,
        num_args(0..=1),
        require_equals(true),
        help = "Don't load the session from the output directory",
        default_missing_value = "true"
    )]
    pub ignore_session: Option<bool>,

    #[command(flatten)]
    pub common: GeneratorRunCommonArgs,
}

#[derive(Debug, Clone, clap::Args)]
pub struct GeneratorListCommand {
    #[command(flatten)]
    pub args: GeneratorListArgs,
}

#[derive(Debug, Clone, clap::Args)]
pub struct GeneratorListArgs {}

pub async fn run(
    generate: &GeneratorCommand,
    ctx: &Context,
) -> eyre::Result<()> {
    match &generate.subcommand {
        GeneratorSubcommand::Run(generator_run_command) => {
            run_generator_run(generator_run_command, ctx).await?
        }
        GeneratorSubcommand::List(generator_list_command) => {
            run_generator_list(generator_list_command, ctx).await?
        }
    }

    Ok(())
}

async fn run_generator_run(
    command: &GeneratorRunCommand,
    ctx: &Context,
) -> eyre::Result<()> {
    let loaded_context = ctx.clone().into_loaded().await?;
    let projects = loaded_context.projects();
    let current_dir = loaded_context.current_dir()?;
    let workspace_dir = loaded_context.root_dir().to_path_buf();

    let (output_dir, project) =
        match (command.args.output.clone(), &command.args.project) {
            (None, None) => (prompt_output_dir(projects, &current_dir)?, None),
            (None, Some(project)) => {
                let p = projects.iter().find(|p| p.name == *project);
                if let Some(p) = p {
                    (p.dir.clone(), Some(p))
                } else {
                    return Err(eyre::eyre!("Project {} not found", project));
                }
            }
            (Some(out), None) => {
                (path_clean::clean(current_dir.join(out)), None)
            }
            (Some(out), Some(_)) => {
                log::warn!(
                    "Both --output and --project are provided, using --output"
                );
                (out, None)
            }
        };

    log::trace!("Generator output directory: {}", output_dir.display());

    let mut pre_exec_values = get_prompt_values(&command.args.common.value);
    let env = loaded_context.get_cached_env_vars(output_dir.as_path());

    let mut context_values = unordered_map!();

    context_values.insert(
        "output_dir".to_string(),
        ValueBag::capture_serde1(&output_dir).to_owned(),
    );

    context_values.insert(
        "workspace_dir".to_string(),
        ValueBag::capture_serde1(&workspace_dir).to_owned(),
    );

    context_values.insert(
        "current_dir".to_string(),
        ValueBag::capture_serde1(&current_dir).to_owned(),
    );

    let default_map = Map::default();

    if let Some(env) = &env {
        context_values.insert(
            "env".to_string(),
            ValueBag::capture_serde1(env).to_owned(),
        );
    } else {
        context_values.insert(
            "env".to_string(),
            ValueBag::capture_serde1(&default_map).to_owned(),
        );
    }

    if let Some(project) = project {
        context_values.insert(
            "project".to_string(),
            ValueBag::capture_serde1(project).to_owned(),
        );
    }

    let mut target_overrides = get_target_overrides(&command.args.target);

    let sys = loaded_context.sys();

    const GENERATOR_OUTPUT_DIR: &str = ".omni";
    const GENERATOR_OUTPUT_FILE: &str = ".omni/generator.json";
    let gen_output_dir = output_dir.join(GENERATOR_OUTPUT_DIR);
    let file = output_dir.join(GENERATOR_OUTPUT_FILE);
    let generators = get_generators(ctx, sys).await?;

    let generator_name = if let Some(name) = &command.args.name {
        Cow::Borrowed(name.as_str())
    } else {
        Cow::Owned(prompt_generator_name(&generators)?)
    };

    let mut has_exiting_session = false;

    if !command.args.ignore_session.unwrap_or(false)
        && sys.fs_exists_no_err_async(&file).await
    {
        let session = GenSession::from_disk(file.as_path(), sys).await?;
        has_exiting_session = true;
        session.restore_targets(&generator_name, &mut target_overrides, false);
        session.restore_prompts(&generator_name, &mut pre_exec_values, false);
    }

    let run = RunConfig {
        dry_run: command.args.dry_run,
        output_dir: output_dir.as_path(),
        overwrite: command.args.overwrite.map(|o| o.value()),
        workspace_dir: &workspace_dir,
        target_overrides: &target_overrides,
        context_values: &context_values,
        prompt_values: &pre_exec_values,
        current_dir: &current_dir,
        env: &env.as_deref().unwrap_or(&default_map),
        args: None,
        use_prompt_defaults: command.args.common.use_defaults,
        available_generators: &generators,
    };

    let session =
        omni_generator::run_named(&generator_name, &run, loaded_context.sys())
            .await?;

    if !command.args.dry_run && !session.is_empty() {
        let save = if let Some(save) = command.args.save_session {
            save
        } else if has_exiting_session {
            true
        } else {
            prompt_save_prompts()?
        };

        if save {
            if !sys.fs_exists_no_err_async(&gen_output_dir).await {
                sys.fs_create_dir_all_async(&gen_output_dir).await?;
            }

            session.write_to_disk(file.as_path(), sys).await?;
        }
    }

    Ok(())
}

fn get_target_overrides(
    target: &[(String, OmniPath)],
) -> UnorderedMap<String, OmniPath> {
    UnorderedMap::from_iter(
        target.iter().map(|(k, v)| (k.to_string(), v.clone())),
    )
}

fn prompt_output_dir(
    projects: &[Project],
    current_dir: &Path,
) -> eyre::Result<PathBuf> {
    let context_values = unordered_map!();
    let prompting_config = PromptingConfiguration::default();

    let prompt =
        PromptConfiguration::<()>::new_select(SelectPromptConfiguration::new(
            BasePromptConfiguration::new(
                "output_dir_or_project",
                "Where should the generator output be written?",
                None,
            ),
            [
                OptionConfiguration::new(
                    "Output directory",
                    None,
                    "output_dir",
                    false,
                ),
                OptionConfiguration::new(
                    "Project directory",
                    None,
                    "project",
                    false,
                ),
            ],
            Some("output_dir".to_string()),
        ));

    let value = omni_prompt::prompt_one(
        &prompt,
        None,
        &context_values,
        &prompting_config,
    )?
    .expect("should have value at this point");

    let value = value
        .by_ref()
        .to_str()
        .ok_or_else(|| eyre::eyre!("value is not a string"))?;

    if value == "output_dir" {
        let text_prompt = TextPromptConfiguration::new(
            ValidatedPromptConfiguration::new(
                BasePromptConfiguration::new(
                    "output_dir",
                    "Output directory",
                    None,
                ),
                [],
            ),
            None,
        );
        let prompt = &PromptConfiguration::<()>::new_text(text_prompt);

        loop {
            let output_dir = omni_prompt::prompt_one(
                &prompt,
                None,
                &context_values,
                &prompting_config,
            )?
            .expect("should have value at this point");
            let output_dir =
                output_dir.by_ref().to_str().expect("value is not a string");

            break Ok(path_clean::clean(current_dir.join(output_dir.as_ref())));
        }
    } else if value == "project" {
        let options = projects
            .iter()
            .map(|p| {
                OptionConfiguration::new(
                    p.name.as_str(),
                    None,
                    p.dir.to_string_lossy(),
                    false,
                )
            })
            .collect::<Vec<_>>();

        let prompt = PromptConfiguration::<()>::new_select(
            SelectPromptConfiguration::new(
                BasePromptConfiguration::new("project", "Select project", None),
                options,
                Some("project".to_string()),
            ),
        );

        let value = omni_prompt::prompt_one(
            &prompt,
            None,
            &context_values,
            &prompting_config,
        )?
        .expect("should have value at this point");

        let value = value
            .by_ref()
            .to_str()
            .ok_or_else(|| eyre::eyre!("value is not a string"))?;

        Ok(Path::new(value.as_ref()).to_path_buf())
    } else {
        Err(eyre::eyre!(
            "invalid value for output_dir_or_project: {value}"
        ))
    }
}

fn prompt_save_prompts() -> eyre::Result<bool> {
    let context_values = unordered_map!();
    let prompting_config = PromptingConfiguration::default();

    let prompt = PromptConfiguration::<()>::new_confirm(
        ConfirmPromptConfiguration::new(
            BasePromptConfiguration::new(
                "save_prompts",
                "Would you like to save prompts and targets to the output directory?",
                Some(Left(true)),
            ),
            Some(Left(true)),
        ),
    );

    let value = omni_prompt::prompt_one(
        &prompt,
        None,
        &context_values,
        &prompting_config,
    )?
    .expect("should have value at this point");

    let value = value
        .by_ref()
        .to_bool()
        .ok_or_else(|| eyre::eyre!("value is not boolean"))?;

    Ok(value)
}

async fn run_generator_list(
    _command: &GeneratorListCommand,
    ctx: &Context,
) -> eyre::Result<()> {
    let generators = get_generators(ctx, ctx.sys()).await?;

    println!("{}", "Available Generators:".bold());
    for generator in generators {
        println!(
            "- {}{}{} (id: {})",
            generator
                .display_name
                .as_deref()
                .unwrap_or(generator.name.as_str())
                .bold(),
            if generator.description.is_some() {
                ": "
            } else {
                ":"
            },
            generator.description.as_deref().unwrap_or(""),
            generator.name.italic()
        );
    }

    Ok(())
}

async fn get_generators(
    ctx: &Context,
    sys: &impl GeneratorSys,
) -> eyre::Result<Vec<Cow<'static, GeneratorConfiguration>>> {
    let omni_path = ctx.omni_dir();
    let generator_sources_path = omni_path.join("./sources/generator");
    let lockfile_path = generator_sources_path.join("lock.json");

    let remote_sources = Arc::new(
        RemoteSourceManager::new(
            RemoteSourceConfig::builder()
                .lockfile_path(lockfile_path)
                .soure_dir_path(generator_sources_path)
                .build(),
            sys.clone(),
        )
        .await?,
    );

    let mut retrieval_tasks = JoinSet::new();

    let mut git_sources = vec![];

    for (idx, config) in
        ctx.workspace_configuration().generators.iter().enumerate()
    {
        // remote generator scopes should start at 100, reserve 0-99 for future uses
        let scope_id = idx + 100;

        match config {
            GeneratorSourceConfiguration::Local(local) => {
                let local = local.clone();
                let root_dir = ctx.root_dir().to_path_buf();
                let sys = sys.clone();
                retrieval_tasks.spawn(async move {
                    let configurations = match local.path {
                        SingleOrMany::Single(item) => {
                            omni_generator::discover(&root_dir, &[item], &sys)
                                .await?
                        }
                        SingleOrMany::Many(items) => {
                            omni_generator::discover(&root_dir, &items, &sys)
                                .await?
                        }
                    };
                    Ok::<_, eyre::Report>(omni_generator::assign_scope_id(
                        scope_id,
                        configurations,
                    ))
                });
            }
            GeneratorSourceConfiguration::Git(git) => {
                let remote_sources = remote_sources.clone();

                git_sources.push((&git.uri, git.rev.as_str()));

                let sys = sys.clone();
                let git = git.clone();

                retrieval_tasks.spawn(async move {
                    let dir = remote_sources
                        .pull_git_repo(&git.uri, &git.rev)
                        .await?;
                    let configurations =
                        omni_generator::discover(&dir, &["**"], &sys).await?;

                    Ok::<_, eyre::Report>(omni_generator::assign_scope_id(
                        scope_id,
                        configurations,
                    ))
                });
            }
        }
    }

    let mut configurations = vec![];

    for configs in retrieval_tasks.join_all().await {
        configurations.extend(configs?);
    }

    remote_sources.retain_git_sources(&git_sources).await?;
    remote_sources.lock().await?;

    Ok(configurations)
}
