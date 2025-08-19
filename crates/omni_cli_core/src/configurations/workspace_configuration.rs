use std::path::Path;

use garde::Validate;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_traits::FsReadAsync;

use crate::{
    configurations::ScriptingConfiguration, constants::WORKSPACE_NAME_REGEX,
    utils,
};

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Eq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct WorkspaceConfiguration {
    #[garde(pattern(*WORKSPACE_NAME_REGEX))]
    pub name: Option<String>,

    pub projects: Vec<String>,

    #[serde(default)]
    pub scripting: ScriptingConfiguration,

    #[serde(default)]
    pub generators: Vec<String>,
}

impl WorkspaceConfiguration {
    pub async fn load<'a>(
        path: impl Into<&'a Path>,
        sys: impl FsReadAsync + Send + Sync,
    ) -> eyre::Result<Self> {
        utils::fs::load_config(path, sys).await
    }
}
