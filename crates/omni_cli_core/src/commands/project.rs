use std::path::{Path, PathBuf};

use clap::{ArgAction, ArgGroup, Args, Subcommand};
use derive_new::new;
use omni_api::{
    OmniApi, ProjectListEntry, ProjectListRequest, ProjectSelector,
};
use omni_context::Context;
use omni_messages::NoopSubscriber;
use omni_tracing_subscriber::noop_subscriber;
use owo_colors::OwoColorize as _;
use serde::Serialize;
use tracing_futures::WithSubscriber as _;

use super::common_types::SerializationFormat;

#[derive(Args, Debug)]
pub struct ProjectCommand {
    #[command(flatten)]
    pub args: ProjectArgs,

    #[command(subcommand)]
    pub subcommand: ProjectSubcommand,
}

#[derive(Args, Debug)]
pub struct ProjectArgs {}

#[derive(Subcommand, Debug)]
pub enum ProjectSubcommand {
    PrintConfig(PrintConfigCommand),
    List(ListCommand),
}

pub async fn run(cmd: &ProjectCommand, ctx: &Context) -> eyre::Result<()> {
    match &cmd.subcommand {
        ProjectSubcommand::PrintConfig(command) => {
            run_print_config(command, ctx).await?;
        }
        ProjectSubcommand::List(command) => {
            run_list(command, ctx).await?;
        }
    }

    Ok(())
}

#[derive(Args, Debug)]
pub struct PrintConfigCommand {
    #[command(flatten)]
    args: PrintConfigArgs,
}

#[derive(Args, Debug)]
#[command(group(
    ArgGroup::new("selector")
        .required(true)
        .args(["project_name", "path"])
))]
pub struct PrintConfigArgs {
    #[arg(
        value_enum,
        long,
        short,
        default_value_t = SerializationFormat::Json,
        help = "Format to use when serializing"
    )]
    format: SerializationFormat,

    #[arg(help = "Name of the project whose configuration to print")]
    project_name: Option<String>,

    #[arg(
        long,
        help = "Path to the project configuration file to print. Relative paths are resolved against the current directory"
    )]
    path: Option<PathBuf>,

    #[arg(long, short, action = ArgAction::SetTrue, help = "Only print the raw configuration syntax, no logs will be added")]
    raw: bool,
}

impl PrintConfigArgs {
    fn selector(&self, current_dir: &Path) -> eyre::Result<ProjectSelector> {
        match (&self.project_name, &self.path) {
            (Some(name), None) => Ok(ProjectSelector::Name(name.clone())),
            (None, Some(path)) => {
                Ok(ProjectSelector::Path(current_dir.join(path)))
            }
            (Some(_), Some(_)) | (None, None) => Err(eyre::eyre!(
                "exactly one of a project name or --path is required"
            )),
        }
    }
}

async fn run_print_config(
    command: &PrintConfigCommand,
    ctx: &Context,
) -> eyre::Result<()> {
    let api = OmniApi::new_with_sys(ctx.clone(), NoopSubscriber);
    let selector = command.args.selector(&ctx.current_dir()?)?;

    let result = if command.args.raw {
        api.project_config(selector)
            .with_subscriber(noop_subscriber())
            .await
    } else {
        api.project_config(selector).await
    };

    match result {
        Ok(response) => {
            for warning in &response.warnings {
                eprintln!("{} {}", "warning:".yellow(), warning);
            }
            omni_file_data_serde::to_writer(
                &mut std::io::stdout(),
                &response.config,
                command.args.format.to_serde_format(),
            )?;
        }
        Err(e) => {
            log::error!("{}", e);
        }
    }

    Ok(())
}

#[derive(Args, Debug)]
pub struct ListCommand {
    #[command(flatten)]
    args: ListArgs,
}

#[derive(Args, Debug)]
pub struct ListArgs {
    #[arg(long, short, action = ArgAction::SetTrue, help = "Only print the raw list, no logs will be added")]
    raw: bool,

    #[arg(
        value_enum,
        long,
        short,
        help = "If provided, the list will be serialized in the format specified"
    )]
    format: Option<SerializationFormat>,

    #[arg(long, action = ArgAction::SetTrue, help = "Also list base templates (configs with `base: true`)")]
    include_base: bool,

    #[arg(long, action = ArgAction::SetTrue, help = "Also list configs that are only reached through `extends`")]
    include_extended: bool,

    #[arg(long, action = ArgAction::SetTrue, help = "List every loaded project configuration")]
    all: bool,
}

impl ListArgs {
    fn request(&self) -> ProjectListRequest {
        ProjectListRequest {
            include_base: self.include_base,
            include_extended: self.include_extended,
            all: self.all,
        }
    }

    /// Any visibility flag switches the output to annotated entries, so
    /// the output shape depends only on the flags, not on the workspace.
    fn is_annotated(&self) -> bool {
        self.include_base || self.include_extended || self.all
    }
}

async fn run_list(command: &ListCommand, ctx: &Context) -> eyre::Result<()> {
    let api = OmniApi::new_with_sys(ctx.clone(), NoopSubscriber);

    let request = command.args.request();
    let entries = if command.args.raw {
        api.project_list(request)
            .with_subscriber(noop_subscriber())
            .await?
    } else {
        api.project_list(request).await?
    };

    if command.args.is_annotated() {
        print_entries(entries, command.args.format)
    } else {
        let names = entries.into_iter().map(|entry| entry.name).collect();
        print_names(names, command.args.format)
    }
}

fn print_names(
    names: Vec<String>,
    format: Option<SerializationFormat>,
) -> eyre::Result<()> {
    match format {
        Some(SerializationFormat::Toml) => omni_file_data_serde::to_writer(
            &mut std::io::stdout(),
            &ProjectNames::new(names),
            SerializationFormat::Toml.to_serde_format(),
        )?,
        Some(format) => omni_file_data_serde::to_writer(
            &mut std::io::stdout(),
            &names,
            format.to_serde_format(),
        )?,
        None => {
            for name in &names {
                println!("{name}");
            }
        }
    }

    Ok(())
}

fn print_entries(
    entries: Vec<ProjectListEntry>,
    format: Option<SerializationFormat>,
) -> eyre::Result<()> {
    match format {
        Some(SerializationFormat::Toml) => omni_file_data_serde::to_writer(
            &mut std::io::stdout(),
            &ProjectEntries::new(entries),
            SerializationFormat::Toml.to_serde_format(),
        )?,
        Some(format) => omni_file_data_serde::to_writer(
            &mut std::io::stdout(),
            &entries,
            format.to_serde_format(),
        )?,
        None => {
            for line in entry_lines(&entries) {
                println!("{line}");
            }
        }
    }

    Ok(())
}

fn entry_tags(entry: &ProjectListEntry) -> String {
    if !entry.base && entry.discovered {
        return "project".to_string();
    }

    let mut tags = vec![];
    if entry.base {
        tags.push("base");
    }
    if !entry.discovered {
        tags.push("extended");
    }
    tags.join(",")
}

fn entry_lines(entries: &[ProjectListEntry]) -> Vec<String> {
    let tags = entries.iter().map(entry_tags).collect::<Vec<_>>();
    let name_width = entries
        .iter()
        .map(|entry| entry.name.chars().count())
        .max()
        .unwrap_or(0);
    let tags_width = tags.iter().map(|t| t.chars().count()).max().unwrap_or(0);

    entries
        .iter()
        .zip(&tags)
        .map(|(entry, tags)| {
            format!(
                "{:<name_width$}  {:<tags_width$}  {}",
                entry.name, tags, entry.path
            )
        })
        .collect()
}

#[derive(Serialize, new)]
struct ProjectNames {
    projects: Vec<String>,
}

#[derive(Serialize, new)]
struct ProjectEntries {
    projects: Vec<ProjectListEntry>,
}

#[cfg(test)]
mod tests {
    use clap::{Parser, error::ErrorKind};

    use super::*;

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: ProjectSubcommand,
    }

    fn try_parse(args: &[&str]) -> Result<ProjectSubcommand, clap::Error> {
        TestCli::try_parse_from(args).map(|cli| cli.cmd)
    }

    fn print_config_args(args: &[&str]) -> PrintConfigArgs {
        match try_parse(args).expect("should parse") {
            ProjectSubcommand::PrintConfig(command) => command.args,
            ProjectSubcommand::List(_) => panic!("expected print-config"),
        }
    }

    fn list_args(args: &[&str]) -> ListArgs {
        match try_parse(args).expect("should parse") {
            ProjectSubcommand::List(command) => command.args,
            ProjectSubcommand::PrintConfig(_) => panic!("expected list"),
        }
    }

    fn entry(
        name: &str,
        path: &str,
        base: bool,
        discovered: bool,
    ) -> ProjectListEntry {
        ProjectListEntry {
            name: name.to_string(),
            path: path.to_string(),
            base,
            discovered,
        }
    }

    #[test]
    fn print_config_accepts_a_name() {
        let args = print_config_args(&["omni", "print-config", "app"]);
        assert_eq!(
            args.selector(Path::new("/ws")).unwrap(),
            ProjectSelector::Name("app".to_string())
        );
    }

    #[test]
    fn print_config_resolves_a_relative_path_against_the_current_dir() {
        let args = print_config_args(&[
            "omni",
            "print-config",
            "--path",
            "tpl/project.omni.yaml",
        ]);
        assert_eq!(
            args.selector(Path::new("/ws/apps")).unwrap(),
            ProjectSelector::Path(PathBuf::from(
                "/ws/apps/tpl/project.omni.yaml"
            ))
        );
    }

    #[test]
    fn print_config_keeps_an_absolute_path() {
        let absolute = std::env::temp_dir().join("project.omni.yaml");
        let absolute = absolute.to_str().unwrap();
        let args =
            print_config_args(&["omni", "print-config", "--path", absolute]);
        assert_eq!(
            args.selector(Path::new("/elsewhere")).unwrap(),
            ProjectSelector::Path(PathBuf::from(absolute))
        );
    }

    #[test]
    fn print_config_rejects_both_a_name_and_a_path() {
        let error = try_parse(&[
            "omni",
            "print-config",
            "app",
            "--path",
            "project.omni.yaml",
        ])
        .expect_err("should be rejected");
        assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
    }

    #[test]
    fn print_config_rejects_neither_a_name_nor_a_path() {
        let error = try_parse(&["omni", "print-config"])
            .expect_err("should be rejected");
        assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn list_without_flags_is_not_annotated() {
        let args = list_args(&["omni", "list"]);
        assert!(!args.is_annotated());
        let request = args.request();
        assert!(
            !request.include_base && !request.include_extended && !request.all
        );
    }

    #[test]
    fn list_flags_are_passed_through_and_annotate() {
        for (flag, expected) in [
            ("--include-base", (true, false, false)),
            ("--include-extended", (false, true, false)),
            ("--all", (false, false, true)),
        ] {
            let args = list_args(&["omni", "list", flag]);
            assert!(args.is_annotated(), "{flag}");
            let request = args.request();
            assert_eq!(
                (request.include_base, request.include_extended, request.all),
                expected,
                "{flag}"
            );
        }
    }

    #[test]
    fn entry_tags_render_the_two_booleans() {
        assert_eq!(entry_tags(&entry("a", "a", false, true)), "project");
        assert_eq!(entry_tags(&entry("a", "a", true, true)), "base");
        assert_eq!(entry_tags(&entry("a", "a", false, false)), "extended");
        assert_eq!(entry_tags(&entry("a", "a", true, false)), "base,extended");
    }

    #[test]
    fn entry_lines_align_columns() {
        let lines = entry_lines(&[
            entry("app", "apps/app/project.omni.yaml", false, true),
            entry("base", "base.omni.yaml", true, false),
            entry("shared-base", "libs/shared/project.omni.yaml", true, true),
        ]);

        assert_eq!(
            lines,
            [
                "app          project        apps/app/project.omni.yaml",
                "base         base,extended  base.omni.yaml",
                "shared-base  base           libs/shared/project.omni.yaml",
            ]
        );
    }
}
