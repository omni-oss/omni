use crate::commands::parser::parse_key_value;

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
}
