use std::{collections::HashMap, path::Path};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{DependencyConfiguration, TaskConfiguration};

fn default_true() -> bool {
    true
}

#[derive(Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub struct ProjectConfiguration {
    pub name: String,
    #[serde(default = "default_true")]
    pub implicit_tasks: bool,
    pub tasks: Option<HashMap<String, TaskConfiguration>>,
    #[serde(default)]
    pub dependencies: Vec<DependencyConfiguration>,
    #[serde(default = "default_true")]
    pub implicit_dependencies: bool,
}

impl ProjectConfiguration {
    pub fn load<'a>(path: impl Into<&'a Path>) -> eyre::Result<Self> {
        let f = std::fs::File::open(path.into())?;
        let p = serde_yml::from_reader(f)?;
        Ok(p)
    }
}
