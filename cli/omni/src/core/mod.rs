use std::{collections::HashMap, path::PathBuf};

use derive_more::Constructor;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Constructor, Deserialize, Serialize)]
pub struct Project {
    pub name: String,
    pub dir: PathBuf,
    pub dependencies: Vec<String>,
    pub tasks: HashMap<String, Task>,
}

#[derive(
    Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema, Constructor,
)]
pub struct Task {
    pub command: String,
    pub dependencies: Vec<String>,
}
