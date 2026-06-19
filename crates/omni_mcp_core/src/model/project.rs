use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ProjectListResult {
    pub projects: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ProjectConfigParams {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ProjectConfigResult {
    pub name: String,
    pub dir: String,
    pub description: Option<String>,
    pub tasks: Vec<TaskSummary>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct TaskSummary {
    pub name: String,
    pub description: Option<String>,
}
