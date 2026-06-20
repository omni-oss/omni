pub mod cache;
pub mod generator;
pub mod hash;
pub mod project;
pub mod task;
pub mod workspace;

use crate::model::*;

pub fn tool_list() -> Vec<rmcp::model::Tool> {
    vec![
        tool_noargs(
            "workspace_info",
            "Return workspace root, cache dir and env vars",
            true,
        ),
        tool_noargs(
            "project_list",
            "List all project names in the workspace",
            true,
        ),
        tool_typed::<ProjectConfigParams>(
            "project_config",
            "Return full configuration for a named project including its tasks",
            true,
        ),
        tool_noargs(
            "generator_list",
            "List all available generators in the workspace",
            true,
        ),
        tool_typed::<GeneratorInspectParams>(
            "generator_inspect",
            "Inspect a generator's full input schema, options, validators and targets",
            true,
        ),
        tool_typed::<GeneratorRunParams>(
            "generator_run",
            "Scaffold files using a generator. Concurrent runs within the same workspace are automatically serialized to prevent race conditions on shared files. Run generators sequentially rather than in parallel.",
            false,
        ),
        tool_typed::<GeneratorValidateInputParams>(
            "generator_validate_input",
            "Validate input values against a generator's schema without running it",
            true,
        ),
        tool_noargs(
            "hash_workspace",
            "Compute a content hash for the entire workspace",
            true,
        ),
        tool_typed::<HashProjectParams>(
            "hash_project",
            "Compute a content hash for a single project (optionally scoped to tasks)",
            true,
        ),
        tool_typed::<CacheStatsParams>(
            "cache_stats",
            "Return cache hit/miss/size statistics per project and task",
            true,
        ),
        tool_typed::<CachePruneParams>(
            "cache_prune",
            "Prune stale cache entries. dry_run=true (default) shows what would be deleted without deleting",
            false,
        ),
        tool_typed::<RunTasksParams>(
            "run_tasks",
            "Execute named tasks with optional project/dir/dry_run filters",
            false,
        ),
        tool_typed::<ExecCommandParams>(
            "exec_command",
            "Run an arbitrary command across projects",
            false,
        ),
    ]
}

fn tool_noargs(
    name: &'static str,
    description: &'static str,
    read_only: bool,
) -> rmcp::model::Tool {
    use rmcp::model::{Tool, ToolAnnotations};
    use std::sync::Arc;
    let schema = Arc::new(
        serde_json::json!({"type": "object", "properties": {}})
            .as_object()
            .unwrap()
            .clone(),
    );
    let tool = Tool::new_with_raw(name, Some(description.into()), schema);
    tool.with_annotations(ToolAnnotations::new().read_only(read_only))
}

fn tool_typed<P: schemars::JsonSchema>(
    name: &'static str,
    description: &'static str,
    read_only: bool,
) -> rmcp::model::Tool {
    use rmcp::model::{Tool, ToolAnnotations};
    let schema_root = schemars::schema_for!(P);
    let schema_value = serde_json::to_value(&schema_root).unwrap();
    let schema_obj = schema_value.as_object().unwrap().clone();
    let schema = std::sync::Arc::new(schema_obj);
    let tool = Tool::new_with_raw(name, Some(description.into()), schema);
    tool.with_annotations(ToolAnnotations::new().read_only(read_only))
}
