pub mod cache;
pub mod generator;
pub mod hash;
pub mod project;
pub mod projection;
pub mod task;
pub mod tool;
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
        tool_typed::<TaskRunParams>(
            "task_run",
            "Execute named tasks with optional project/dir/dry_run filters",
            false,
        ),
        tool_typed::<ExecCommandParams>(
            "exec_command",
            "Run an arbitrary command across projects",
            false,
        ),
        tool_noargs(
            "tool_list",
            "List all available tools in the workspace",
            true,
        ),
        tool_typed::<ToolInspectParams>(
            "tool_inspect",
            "Inspect a tool's input schema derived from its own inputs",
            true,
        ),
        tool_typed::<ToolRunParams>(
            "tool_run",
            "Run a workspace tool by name with JSON arguments and return its captured value",
            false,
        ),
        tool_typed::<ProjectionSyncParams>(
            "projection_sync",
            "Materialize configured projections into the workspace via links, persisting the ledger. Idempotent; not read-only.",
            false,
        ),
        tool_typed::<ProjectionStatusParams>(
            "projection_status",
            "Report the state of every recorded projection link (ok/missing/broken/drifted)",
            true,
        ),
        tool_typed::<ProjectionUnlinkParams>(
            "projection_unlink",
            "Tear down the links recorded for a single projection source id",
            false,
        ),
        tool_typed::<ProjectionPruneParams>(
            "projection_prune",
            "Remove ledger-recorded links whose destinations have become dangling",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_exactly_three_fixed_tool_entries() {
        let names: Vec<String> =
            tool_list().iter().map(|t| t.name.to_string()).collect();
        for expected in ["tool_list", "tool_inspect", "tool_run"] {
            assert!(
                names.iter().any(|n| n == expected),
                "missing MCP tool entry `{expected}`; got {names:?}"
            );
        }
        // Exactly three tool-subsystem entries, never one per workspace tool.
        let tool_entries =
            names.iter().filter(|n| n.starts_with("tool_")).count();
        assert_eq!(tool_entries, 3, "got {names:?}");
    }

    #[test]
    fn tool_run_is_not_read_only_but_list_and_inspect_are() {
        let by_name = |name: &str| {
            tool_list()
                .into_iter()
                .find(|t| t.name == name)
                .unwrap_or_else(|| panic!("missing {name}"))
        };
        let read_only = |t: &rmcp::model::Tool| {
            t.annotations.as_ref().and_then(|a| a.read_only_hint)
        };
        assert_eq!(read_only(&by_name("tool_list")), Some(true));
        assert_eq!(read_only(&by_name("tool_inspect")), Some(true));
        assert_eq!(read_only(&by_name("tool_run")), Some(false));
    }

    #[test]
    fn exposes_projection_tools_with_correct_read_only_flags() {
        let by_name = |name: &str| {
            tool_list()
                .into_iter()
                .find(|t| t.name == name)
                .unwrap_or_else(|| panic!("missing MCP tool entry `{name}`"))
        };
        let read_only = |t: &rmcp::model::Tool| {
            t.annotations.as_ref().and_then(|a| a.read_only_hint)
        };
        // Only status is read-only; sync/unlink/prune mutate the workspace.
        assert_eq!(read_only(&by_name("projection_status")), Some(true));
        assert_eq!(read_only(&by_name("projection_sync")), Some(false));
        assert_eq!(read_only(&by_name("projection_unlink")), Some(false));
        assert_eq!(read_only(&by_name("projection_prune")), Some(false));
    }
}
