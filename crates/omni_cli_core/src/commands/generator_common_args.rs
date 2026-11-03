use omni_configurations::RedactOverrides;

use crate::commands::{common_args::RedactModeArg, parser::parse_key_value};

#[derive(Debug, clap::Args, Clone)]
pub struct GeneratorRunCommonArgs {
    #[arg(
        long,
        short,
        help = "Prefill values to inputs. Values are passed as plain strings (unlike `omni tool run --value`, which parses JSON when possible)",
        value_parser = parse_key_value::<String, String>
    )]
    pub value: Vec<(String, String)>,

    #[arg(
        long,
        help = "Use default values for inputs",
        default_value_t = false,
        action = clap::ArgAction::SetTrue
    )]
    pub use_defaults: bool,

    #[arg(
        long = "redact-input",
        value_name = "NAME",
        help = "Redact the value of this input (name or glob) in logs. Repeatable"
    )]
    pub redact_input: Vec<String>,

    #[arg(
        long = "redact-env",
        value_name = "NAME",
        help = "Redact the value of this environment variable (name or glob) in logs. Repeatable"
    )]
    pub redact_env: Vec<String>,

    #[arg(
        long = "redact",
        value_name = "MODE",
        value_enum,
        help = "Redaction mode: declared (apply configured policy), all, or none (prints a notice)"
    )]
    pub redact: Option<RedactModeArg>,
}

impl GeneratorRunCommonArgs {
    pub fn redact_overrides(&self) -> RedactOverrides {
        RedactOverrides {
            mode: self.redact.map(Into::into),
            args: vec![],
            inputs: self.redact_input.clone(),
            env: self.redact_env.clone(),
        }
    }
}
