//! Notices raised by CLI commands.

use omni_notice::Notice;

omni_codes::prefixes!("env", "generator");

/// An environment variable that was asked for is not set.
///
/// `omni env get <key>` looked the variable up in the resolved environment
/// (workspace env files and, if enabled, the inherited process environment)
/// and found nothing.
#[derive(Notice)]
#[notice(
    code = "env.var_missing",
    level = warn,
    message = "environmental variable does not exists: {name}"
)]
pub struct EnvVarMissing {
    pub name: String,
}

/// Both `--output` and `--project` were given.
///
/// The output directory comes from `--output`; the project's directory is not
/// used.
#[derive(Notice)]
#[notice(
    code = "generator.output_overrides_project",
    level = warn,
    message = "Both --output and --project are provided, using --output"
)]
pub struct OutputOverridesProject;
