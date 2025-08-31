use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// A serializable representation of a single argument
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CliArg {
    pub id: String,
    pub short: Option<char>,
    pub long: Option<String>,
    pub aliases: Vec<String>,
    pub required: bool,
    pub help: Option<String>,
    pub default_values: Vec<String>,
    pub possible_values: Vec<String>,
    pub env: Option<String>,
}

// A serializable representation of a command
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CliCommand {
    pub name: String,
    pub bin_name: Option<String>,
    pub author: Option<String>,
    pub version: Option<String>,
    pub aliases: Vec<String>,
    pub about: Option<String>,
    pub positionals: Vec<CliArg>,
    pub opts: Vec<CliArg>,
    pub subcommands: Vec<CliCommand>,
}

pub type CliDeclspec = CliCommand;
