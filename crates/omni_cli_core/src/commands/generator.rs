use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use super::parser::parse_key_value;
use clap_utils::EnumValueAdapter;
use either::Left;
use maps::{Map, UnorderedMap, unordered_map};
use omni_context::Context;
use omni_core::Project;
use omni_generator::{GenSession, RunConfig};
use omni_generator_configurations::{
    GeneratorConfiguration, OmniPath, OverwriteConfiguration,
};
use omni_prompt::configuration::{
    BasePromptConfiguration, ConfirmPromptConfiguration, OptionConfiguration,
    PromptConfiguration, PromptingConfiguration, SelectPromptConfiguration,
    TextPromptConfiguration, ValidatedPromptConfiguration,
};
use owo_colors::OwoColorize;
use system_traits::{FsCreateDirAllAsync, FsMetadataAsync};
use value_bag::{OwnedValueBag, ValueBag};

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
        help = "Prefill values to prompts",
        value_parser = parse_key_value::<String, String>
    )]
    pub value: Vec<(String, String)>,

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
        num_args(0..1),
        help = "Save prompts and targets to the output directory so they can be reused future invocations",
        default_missing_value = "true"
    )]
    pub save_session: Option<bool>,

    #[arg(
        long,
        help = "Don't load the session from the output directory",
        default_missing_value = "true"
    )]
    pub ignore_session: Option<bool>,
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
    let current_dir = loaded_context.current_dir()?.clone();
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
                trace::warn!(
                    "Both --output and --project are provided, using --output"
                );
                (out, None)
            }
        };

    trace::trace!("Generator output directory: {}", output_dir.display());

    let mut pre_exec_values = get_prompt_values(&command.args.value);
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
    let generators = omni_generator::discover(
        loaded_context.root_dir(),
        &ctx.workspace_configuration().generators,
        sys,
    )
    .await?;

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
    };

    let session = omni_generator::run(
        &generator_name,
        &ctx.workspace_configuration().generators,
        &run,
        loaded_context.sys(),
    )
    .await?;

    if !command.args.dry_run {
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

fn get_prompt_values(
    values: &[(String, String)],
) -> UnorderedMap<String, OwnedValueBag> {
    UnorderedMap::from_iter(
        values.iter().map(|(k, v)| {
            (k.to_string(), ValueBag::capture_serde1(v).to_owned())
        }),
    )
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

fn prompt_generator_name(
    generators: &[GeneratorConfiguration],
) -> eyre::Result<String> {
    let context_values = unordered_map!();
    let prompting_config = PromptingConfiguration::default();

    let prompt =
        PromptConfiguration::<()>::new_select(SelectPromptConfiguration::new(
            BasePromptConfiguration::new(
                "generator_name",
                "Select generator",
                None,
            ),
            generators
                .iter()
                .map(|g| {
                    OptionConfiguration::new(
                        g.display_name.as_deref().unwrap_or(&g.name.as_str()),
                        g.description.clone(),
                        g.name.clone(),
                        false,
                    )
                })
                .collect::<Vec<_>>(),
            Some("generator_name".to_string()),
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

    Ok(value.to_string())
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
    let generators = omni_generator::discover(
        ctx.root_dir(),
        &ctx.workspace_configuration().generators,
        ctx.sys(),
    )
    .await?;

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
