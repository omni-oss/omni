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
use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL};
use itertools::Itertools;
use maps::{UnorderedMap, unordered_map};
use omni_api::{GeneratorRunRequest, OmniApi};
use omni_configurations::{GeneratorSourceConfiguration, types::SingleOrMany};
use omni_context::Context;
use omni_core::Project;
use omni_generator::GeneratorSys;
use omni_generator_configurations::{
    AllowedValueExtras, GenBase, Generator, GeneratorConfiguration, OmniPath,
    OverwriteConfiguration, allowed_extras, gen_base,
};
use omni_input_provider::configuration::builder::string;
use omni_input_provider::{AllowedValue, ValidationConfig, collect_one};
use omni_messages::NoopSubscriber;
use omni_prompt::{CliInputProvider, builder::allowed};
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

    #[arg(
        long,
        help = "Maximum run-generator nesting depth before the run is aborted. Omit to use the default. Raise it if a generator legitimately nests deeper than the default."
    )]
    pub max_depth: Option<usize>,

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
        max_depth: command.args.max_depth,
    };

    let api = OmniApi::new_with_loaded_sys(
        loaded_context,
        crate::subscriber::CliSubscriber::new_stream(),
    );
    let response = api.generator_run(req).await?;

    report_generator_output(ctx.root_dir(), response);

    Ok(())
}

fn report_generator_output(
    root_dir: &Path,
    response: omni_api::GeneratorRunResponse,
) {
    use omni_utils::path;

    let root_dir = path::clean(root_dir);

    let mut files_created = vec![];
    let mut files_modified = vec![];
    let mut files_removed = vec![];
    let mut dirs_created = vec![];
    let mut dirs_removed = vec![];
    let mut renamed = vec![];
    let mut copied = vec![];

    fn clean_diff_path(path: PathBuf, root_dir: &Path) -> PathBuf {
        path::diff(&path::clean(&path), root_dir).unwrap_or(path)
    }

    for action in response.actions {
        match action {
            omni_generator::Action::CreateFile { path } => {
                files_created.push(clean_diff_path(path, &root_dir));
            }
            omni_generator::Action::ModifyFile { path } => {
                files_modified.push(clean_diff_path(path, &root_dir));
            }
            omni_generator::Action::RemoveFile { path } => {
                files_removed.push(clean_diff_path(path, &root_dir));
            }
            omni_generator::Action::CreateDir { path } => {
                dirs_created.push(clean_diff_path(path, &root_dir));
            }
            omni_generator::Action::RemoveDir { path } => {
                dirs_removed.push(clean_diff_path(path, &root_dir));
            }
            omni_generator::Action::RemoveDirAll { path } => {
                dirs_removed.push(clean_diff_path(path, &root_dir));
            }
            omni_generator::Action::Rename { from, to } => {
                renamed.push((
                    clean_diff_path(from, &root_dir),
                    clean_diff_path(to, &root_dir),
                ));
            }
            omni_generator::Action::Copy { from, to } => {
                copied.push((
                    clean_diff_path(from, &root_dir),
                    clean_diff_path(to, &root_dir),
                ));
            }
        }
    }

    if !files_created.is_empty()
        || !files_modified.is_empty()
        || !files_removed.is_empty()
        || !dirs_created.is_empty()
        || !dirs_removed.is_empty()
        || !renamed.is_empty()
        || !copied.is_empty()
    {
        let mut table = comfy_table::Table::new();
        table
            .load_preset(UTF8_FULL)
            .apply_modifier(UTF8_ROUND_CORNERS)
            .set_header(vec!["Action", "Paths", "Count"]);

        if !dirs_created.is_empty() || !files_created.is_empty() {
            table.add_row(vec![
                "Created".to_string(),
                dirs_created
                    .iter()
                    .map(|p| p.display())
                    .chain(files_created.iter().map(|p| p.display()))
                    .join("\n"),
                format!("{}", dirs_created.len() + files_created.len()),
            ]);
        }

        if !files_modified.is_empty() {
            table.add_row(vec![
                "Modified".to_string(),
                files_modified.iter().map(|p| p.display()).join("\n"),
                format!("{}", files_modified.len()),
            ]);
        }

        if !renamed.is_empty() {
            table.add_row(vec![
                "Renamed".to_string(),
                renamed
                    .iter()
                    .map(|(from, to)| {
                        format!("{}\n  ⮡ {}", from.display(), to.display())
                    })
                    .join("\n"),
                format!("{}", renamed.len()),
            ]);
        }

        if !copied.is_empty() {
            table.add_row(vec![
                "Copied".to_string(),
                copied
                    .iter()
                    .map(|(from, to)| {
                        format!("{}\n  ⮡ {}", from.display(), to.display())
                    })
                    .join("\n"),
                format!("{}", copied.len()),
            ]);
        }

        if !dirs_removed.is_empty() || !files_removed.is_empty() {
            table.add_row(vec![
                "Removed".to_string(),
                dirs_removed.iter().map(|path| path.display()).join("\n"),
                format!("{}", dirs_removed.len() + files_removed.len()),
            ]);
        }

        println!("{table}");
    }

    if response.session_saved {
        println!("Session saved to disk.");
    }
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
    let prompting_config = ValidationConfig::default();

    let prompt = string::<Generator>()
        .name("output_dir_or_project")
        .base_extra(
            gen_base()
                .message("Where should the generator output be written?")
                .build(),
        )
        .allowed([
            allowed()
                .value("output_dir")
                .base_extra(allowed_extras().name("Output directory").build())
                .build(),
            allowed()
                .value("project")
                .base_extra(allowed_extras().name("Project directory").build())
                .build(),
        ])
        .default("output_dir")
        .build();

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
        let prompt = string::<Generator>()
            .name("output_dir")
            .base_extra(gen_base().message("Output directory path").build())
            .build();

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
            .map(|p| (p.name.clone(), p.dir.to_string_lossy().to_string()))
            .collect::<Vec<_>>();

        let prompt = string::<Generator>()
            .name("project")
            .base_extra(GenBase::new("Select project"))
            .allowed(options.iter().map(|(name, value)| AllowedValue {
                value: value.clone(),
                description: None,
                base_extra: AllowedValueExtras {
                    name: Some(name.clone()),
                    separator: false,
                },
            }))
            .build();

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
