use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::commands::{
    generator_common_args::GeneratorRunCommonArgs,
    generator_utils::{get_input_values, prompt_generator_name},
};

use super::parser::parse_key_value;
use clap_utils::EnumValueAdapter;
use maps::{UnorderedMap, unordered_map};
use omni_api::{GeneratorRunRequest, OmniApi};
use omni_configurations::{GeneratorSourceConfiguration, types::SingleOrMany};
use omni_context::Context;
use omni_core::Project;
use omni_generator::GeneratorSys;
use omni_generator_configurations::{
    GeneratorConfiguration, OmniPath, OverwriteConfiguration,
};
use omni_input_provider::{
    CollectionConfig, collect_one,
    configuration::{
        BaseInputConfiguration, InputConfiguration, OptionConfiguration,
        SelectInputConfiguration, TextInputConfiguration,
        ValidatedInputConfiguration,
    },
};
use omni_messages::NoopSubscriber;
use omni_prompt::CliInputProvider;
use omni_remote_sources::manager::{
    RemoteSourceManager, config::RemoteSourceConfig,
};
use owo_colors::OwoColorize;
use tokio::task::JoinSet;

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
        help = "Save inputs and targets to the output directory so they can be reused future invocations",
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

    let (output_dir, _project) =
        match (command.args.output.clone(), &command.args.project) {
            (None, None) => {
                (prompt_output_dir(projects, &current_dir).await?, None)
            }
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

    let sys = loaded_context.sys();
    let generators = get_generators(ctx, sys).await?;

    let generator_name = if let Some(name) = &command.args.name {
        Cow::Borrowed(name.as_str())
    } else {
        Cow::Owned(prompt_generator_name(&generators).await?)
    };

    let req = GeneratorRunRequest {
        name: Some(generator_name.to_string()),
        output_dir,
        project: None,
        target: get_target_overrides(&command.args.target)
            .into_iter()
            .collect(),
        dry_run: command.args.dry_run,
        overwrite: command.args.overwrite.map(|o| o.value()),
        save_session: command.args.save_session,
        ignore_session: command.args.ignore_session,
        input_values: get_input_values(&command.args.common.value),
        use_defaults: command.args.common.use_defaults,
        input_provider: Arc::new(CliInputProvider::default()),
    };

    OmniApi::new_with_sys(
        ctx.clone(),
        crate::subscriber::CliSubscriber::new_stream(),
    )
    .generator_run(req)
    .await?;

    Ok(())
}

fn get_target_overrides(
    target: &[(String, OmniPath)],
) -> UnorderedMap<String, OmniPath> {
    UnorderedMap::from_iter(
        target.iter().map(|(k, v)| (k.to_string(), v.clone())),
    )
}

async fn prompt_output_dir(
    projects: &[Project],
    current_dir: &Path,
) -> eyre::Result<PathBuf> {
    let context_values = unordered_map!();
    let prompting_config = CollectionConfig::default();

    let prompt =
        InputConfiguration::<()>::new_select(SelectInputConfiguration::new(
            BaseInputConfiguration::new(
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

    let value = collect_one(
        &prompt,
        None,
        &context_values,
        &prompting_config,
        &CliInputProvider::default(),
    )
    .await?
    .expect("should have value at this point");

    let value = value
        .by_ref()
        .to_str()
        .ok_or_else(|| eyre::eyre!("value is not a string"))?;

    if value == "output_dir" {
        let text_prompt = TextInputConfiguration::new(
            ValidatedInputConfiguration::new(
                BaseInputConfiguration::new(
                    "output_dir",
                    "Output directory",
                    None,
                ),
                [],
            ),
            None,
        );
        let prompt = &InputConfiguration::<()>::new_text(text_prompt);

        loop {
            let output_dir = collect_one(
                &prompt,
                None,
                &context_values,
                &prompting_config,
                &CliInputProvider::default(),
            )
            .await?
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

        let prompt = InputConfiguration::<()>::new_select(
            SelectInputConfiguration::new(
                BaseInputConfiguration::new("project", "Select project", None),
                options,
                Some("project".to_string()),
            ),
        );

        let value = collect_one(
            &prompt,
            None,
            &context_values,
            &prompting_config,
            &CliInputProvider::default(),
        )
        .await?
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

async fn run_generator_list(
    _command: &GeneratorListCommand,
    ctx: &Context,
) -> eyre::Result<()> {
    let response = OmniApi::new_with_sys(ctx.clone(), NoopSubscriber)
        .generator_list()
        .await?;

    println!("{}", "Available Generators:".bold());
    for generator in response.generators {
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
