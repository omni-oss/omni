use std::path::{Path, PathBuf};

use super::parser::parse_key_value;
use clap_utils::EnumValueAdapter;
use maps::{UnorderedMap, unordered_map};
use omni_context::Context;
use omni_core::Project;
use omni_generator::RunConfig;
use omni_generator_configurations::{
    GeneratorConfiguration, OverwriteConfiguration,
};
use omni_prompt::configuration::{
    BasePromptConfiguration, OptionConfiguration, PromptConfiguration,
    PromptingConfiguration, SelectPromptConfiguration, TextPromptConfiguration,
    ValidatedPromptConfiguration,
};
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
        help = "If provided, it will use the project's directory as output directory",
        conflicts_with = "out_dir"
    )]
    pub project: Option<String>,

    #[arg(long, short, help = "Output directory")]
    pub out_dir: Option<PathBuf>,

    #[arg(
        long,
        short,
        help = "Prefill answers to prompts",
        value_parser = parse_key_value::<String, String>
    )]
    pub answer: Vec<(String, String)>,

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
    let generators = omni_generator::discover(
        ctx.root_dir(),
        &ctx.workspace_configuration().generators,
        ctx.sys(),
    )
    .await?;

    omni_generator::validate(&generators)?;

    let generator_name = if let Some(name) = command.args.name.clone() {
        name
    } else {
        prompt_generator_name(&generators)?
    };

    let generator = generators
        .iter()
        .find(|g| g.name == generator_name)
        .ok_or_else(|| {
            eyre::eyre!("generator '{}' not found", generator_name)
        })?;

    trace::trace!("Generator: {:#?}", generator);

    let loaded_context = ctx.clone().into_loaded().await?;
    let projects = loaded_context.projects();
    let current_dir = loaded_context.current_dir()?;

    let (output_dir, project) =
        match (command.args.out_dir.clone(), &command.args.project) {
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
                    "Both --out-dir and --project are provided, using --out-dir"
                );
                (out, None)
            }
        };

    trace::trace!("Generator output directory: {}", output_dir.display());

    let run = RunConfig {
        dry_run: command.args.dry_run,
        output_dir: output_dir.as_path(),
        overwrite: command.args.overwrite.map(|o| o.value()),
    };
    let pre_exec_values = get_pre_exec_values(&command.args.answer);
    let env = loaded_context.get_cached_env_vars(output_dir.as_path());

    let mut context_values = unordered_map!();

    if let Some(env) = env {
        context_values.insert(
            "env".to_string(),
            ValueBag::capture_serde1(&env).to_owned(),
        );
    }

    if let Some(project) = project {
        context_values.insert(
            "project".to_string(),
            ValueBag::capture_serde1(project).to_owned(),
        );
    }

    omni_generator::run(
        &generator,
        &pre_exec_values,
        &context_values,
        &run,
        loaded_context.sys(),
    )
    .await?;

    Ok(())
}

fn get_pre_exec_values(
    values: &[(String, String)],
) -> UnorderedMap<String, OwnedValueBag> {
    UnorderedMap::from_iter(
        values.iter().map(|(k, v)| {
            (k.to_string(), ValueBag::capture_serde1(v).to_owned())
        }),
    )
}

fn prompt_output_dir(
    projects: &[Project],
    current_dir: &Path,
) -> eyre::Result<PathBuf> {
    let context_values = unordered_map!();
    let prompting_config = PromptingConfiguration::default();

    let prompt =
        PromptConfiguration::new_select(SelectPromptConfiguration::new(
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
        let prompt = &PromptConfiguration::new_text(text_prompt);

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

        let prompt =
            PromptConfiguration::new_select(SelectPromptConfiguration::new(
                BasePromptConfiguration::new("project", "Select project", None),
                options,
                Some("project".to_string()),
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
        PromptConfiguration::new_select(SelectPromptConfiguration::new(
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

async fn run_generator_list(
    _command: &GeneratorListCommand,
    _ctx: &Context,
) -> eyre::Result<()> {
    Ok(())
}
