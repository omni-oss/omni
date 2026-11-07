//! Notices raised by generator commands.

use omni_notice::Notice;

omni_codes::prefixes!("generator");

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
