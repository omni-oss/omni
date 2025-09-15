use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// A serializable representation of a single argument
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CliArg {
    pub id: String,
    pub short: Option<char>,
    pub long: Option<String>,
    pub long_help: Option<String>,
    pub aliases: Vec<String>,
    pub required: bool,
    pub help: Option<String>,
    pub default_values: Vec<String>,
    pub possible_values: Vec<String>,
    pub env: Option<String>,
    pub groups: Vec<String>,
    pub conflicts_with: Vec<CliArgLink>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CliArgLink {
    pub id: String,
    pub short: Option<char>,
    pub long: Option<String>,
}

// A serializable representation of a command
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CliCommand {
    pub name: String,
    pub bin_name: Option<String>,
    pub author: Option<String>,
    pub version: Option<String>,
    pub long_version: Option<String>,
    pub short_flag: Option<char>,
    pub long_flag: Option<String>,
    pub aliases: Vec<String>,
    pub about: Option<String>,
    pub long_about: Option<String>,
    pub positionals: Vec<CliArg>,
    pub opts: Vec<CliArg>,
    pub subcommands: Vec<CliCommand>,
    pub groups: Vec<CliGroup>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CliGroup {
    pub id: String,
    pub arg_ids: Vec<String>,
}

pub type CliDeclspec = CliCommand;
