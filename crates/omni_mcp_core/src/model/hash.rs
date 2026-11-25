use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct HashProjectParams {
    pub name: String,
    /// Exact task names (a task or an alias; `project#task` must name this
    /// project); glob characters are literal. With no `tasks` and no
    /// `task_patterns`, every task of the project is hashed.
    #[serde(default)]
    pub tasks: Vec<String>,
    /// Task name globs, in addition to `tasks`.
    #[serde(default)]
    pub task_patterns: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct HashResult {
    pub hash: String,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn tasks_are_exact_and_patterns_are_separate() {
        let params: HashProjectParams = serde_json::from_value(json!({
            "name": "app",
            "tasks": ["build*"],
            "task_patterns": ["test*"],
        }))
        .unwrap();
        assert_eq!(params.tasks, ["build*"]);
        assert_eq!(params.task_patterns, ["test*"]);
    }

    #[test]
    fn selectors_are_optional() {
        let params: HashProjectParams =
            serde_json::from_value(json!({ "name": "app" })).unwrap();
        assert!(params.tasks.is_empty() && params.task_patterns.is_empty());
    }
}
