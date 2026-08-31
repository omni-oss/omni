//! Integration tests for `omni_api`.
//!
//! Each test creates a minimal but valid workspace in a temporary directory,
//! constructs an `OmniApi` backed by that workspace, and asserts the expected
//! behaviour of each operation.

use std::path::Path;

use omni_api::{EnvRequest, OmniApi, SchemaKind, handle_config_schema};
use omni_messages::NoopSubscriber;
use omni_tracing_subscriber::TracingConfig;
use system_traits::impls::RealSys;

// ── Test-workspace helpers ────────────────────────────────────────────────────

/// Write a minimal workspace with one project to `dir`.
fn write_workspace(dir: &Path) {
    std::fs::write(
        dir.join("workspace.omni.yaml"),
        "projects:\n  - \"projects/**\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("projects/alpha")).unwrap();
    std::fs::write(
        dir.join("projects/alpha/project.omni.yaml"),
        "name: alpha\ntasks:\n  build:\n    exec: echo \"alpha\"\n",
    )
    .unwrap();
}

/// Build an `OmniApi` from the given workspace directory (no setup / keyring).
fn make_api(dir: &Path) -> OmniApi<RealSys, NoopSubscriber> {
    let ctx = omni_context::Context::new(
        RealSys,
        "development",
        dir,
        false,
        "workspace.omni.yaml",
        None,
        &TracingConfig::disabled(),
    )
    .expect("context creation failed");
    OmniApi::new_with_sys(ctx, NoopSubscriber)
}

// ── config_schema (no workspace needed) ──────────────────────────────────────

#[test]
fn config_schema_workspace_is_json_object() {
    let resp = handle_config_schema(SchemaKind::Workspace).expect("schema");
    assert!(resp.schema.is_object());
}

#[test]
fn config_schema_project_is_json_object() {
    let resp = handle_config_schema(SchemaKind::Project).expect("schema");
    assert!(resp.schema.is_object());
}

#[test]
fn config_schema_generator_is_json_object() {
    let resp = handle_config_schema(SchemaKind::Generator).expect("schema");
    assert!(resp.schema.is_object());
}

// ── OmniApiBuilder ────────────────────────────────────────────────────────────

#[test]
fn builder_with_real_workspace_succeeds() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_workspace(tmp.path());

    let result = OmniApi::builder()
        .root_dir(tmp.path())
        .with_setup(false)
        .build();

    assert!(
        result.is_ok(),
        "builder should succeed with a valid workspace"
    );
}

#[test]
fn builder_fails_without_workspace_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let result = OmniApi::builder()
        .root_dir(tmp.path())
        .with_setup(false)
        .build();
    assert!(
        result.is_err(),
        "builder should fail without a workspace file"
    );
}

// ── project ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn project_list_contains_alpha() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_workspace(tmp.path());
    let api = make_api(tmp.path());

    let names = api.project_list().await.expect("project list");
    assert!(
        names.contains(&"alpha".to_string()),
        "expected 'alpha' in project list, got {names:?}"
    );
}

#[tokio::test]
async fn project_config_alpha() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_workspace(tmp.path());
    let api = make_api(tmp.path());

    let cfg = api.project_config("alpha").await.expect("project config");
    assert_eq!(cfg.name, "alpha");
}

#[tokio::test]
async fn project_config_missing_returns_err() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_workspace(tmp.path());
    let api = make_api(tmp.path());

    let result = api.project_config("does_not_exist").await;
    assert!(result.is_err(), "should error for unknown project");
}

// ── hash ──────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn hash_workspace_returns_non_empty_string() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_workspace(tmp.path());
    let api = make_api(tmp.path());

    let resp = api.hash_workspace().await.expect("workspace hash");
    assert!(!resp.hash.is_empty(), "workspace hash should be non-empty");
}

#[tokio::test]
async fn hash_project_returns_non_empty_string() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_workspace(tmp.path());
    let api = make_api(tmp.path());

    let resp = api.hash_project("alpha", &[]).await.expect("project hash");
    assert!(!resp.hash.is_empty(), "project hash should be non-empty");
}

// ── env ───────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn env_all_returns_ok() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_workspace(tmp.path());
    let api = make_api(tmp.path());

    let resp = api
        .get_env(EnvRequest { key: None })
        .await
        .expect("get all env");
    let _ = resp.vars;
}

#[tokio::test]
async fn env_get_specific_key_filters_result() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_workspace(tmp.path());
    let api = make_api(tmp.path());

    let all = api
        .get_env(EnvRequest { key: None })
        .await
        .expect("get all env");
    if let Some(key) = all.vars.keys().next().cloned() {
        let specific = api
            .get_env(EnvRequest {
                key: Some(key.clone()),
            })
            .await
            .expect("get specific key");
        assert_eq!(specific.vars.len(), 1);
        assert!(specific.vars.contains_key(&key));
    }
}

#[tokio::test]
async fn env_get_missing_key_returns_empty_map() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_workspace(tmp.path());
    let api = make_api(tmp.path());

    let resp = api
        .get_env(EnvRequest {
            key: Some("DOES_NOT_EXIST_XYZ".into()),
        })
        .await
        .expect("get missing key");
    assert!(resp.vars.is_empty());
}

// ── cache dir ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn cache_dir_is_inside_workspace() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_workspace(tmp.path());
    let api = make_api(tmp.path());

    let dir = api.cache_dir().await;
    let canonical_tmp = tmp.path().canonicalize().unwrap();
    assert!(
        dir.starts_with(&canonical_tmp),
        "cache_dir {dir:?} should be inside workspace {canonical_tmp:?}"
    );
}

// ── generator inspect helpers ─────────────────────────────────────────────────

/// Write a minimal generator named `gen_name` with a boolean and a string
/// input into a `generators/` subdirectory of `workspace_dir`.
fn write_generator(workspace_dir: &Path, gen_name: &str) {
    let gen_dir = workspace_dir.join("generators").join(gen_name);
    std::fs::create_dir_all(&gen_dir).unwrap();
    std::fs::write(
        gen_dir.join("generator.omni.yaml"),
        format!(
            r#"
name: {gen_name}
inputs:
  - type: boolean
    name: flag
    message: "Enable?"
  - type: string
    name: proj_name
    message: "Project name"
actions: []
"#
        ),
    )
    .unwrap();
}

/// Build a workspace + generator and return an `OmniApi` for it.
fn make_api_with_generator(
    workspace_dir: &Path,
    gen_name: &str,
) -> OmniApi<RealSys, NoopSubscriber> {
    write_workspace(workspace_dir);
    // Register the generators directory in the workspace config.
    std::fs::write(
        workspace_dir.join("workspace.omni.yaml"),
        "projects:\n  - \"projects/**\"\ngenerators:\n  - source: local\n    path: \"generators/**\"\n",
    )
    .unwrap();
    write_generator(workspace_dir, gen_name);
    make_api(workspace_dir)
}

// ── generator inspect ────────────────────────────────────────────────────────────

#[tokio::test]
async fn inspect_widget_view_infers_confirm_and_text_kinds() {
    let tmp = tempfile::TempDir::new().unwrap();
    let api = make_api_with_generator(tmp.path(), "my-gen");

    let resp = api
        .generator_inspect("my-gen", omni_api::InspectViewKind::Widget)
        .await
        .expect("inspect should succeed");

    let omni_api::GeneratorInspectResponse::Widget(node) = resp else {
        panic!("expected Widget response");
    };
    assert_eq!(node.name, "my-gen");
    assert_eq!(node.inputs.len(), 2);

    let flag = node.inputs.iter().find(|i| i.name == "flag").unwrap();
    assert!(
        matches!(flag.kind, omni_api::GeneratorInputKind::Confirm),
        "boolean should infer Confirm, got {:?}",
        flag.kind
    );

    let name_input =
        node.inputs.iter().find(|i| i.name == "proj_name").unwrap();
    assert!(
        matches!(name_input.kind, omni_api::GeneratorInputKind::Text),
        "string without allowed should infer Text, got {:?}",
        name_input.kind
    );
}

#[tokio::test]
async fn inspect_data_view_strips_presentation_extras() {
    let tmp = tempfile::TempDir::new().unwrap();
    let api = make_api_with_generator(tmp.path(), "my-gen");

    let resp = api
        .generator_inspect("my-gen", omni_api::InspectViewKind::Data)
        .await
        .expect("inspect should succeed");

    let omni_api::GeneratorInspectResponse::Data(node) = resp else {
        panic!("expected Data response");
    };
    assert_eq!(node.name, "my-gen");
    assert_eq!(node.inputs.len(), 2);

    // Data view returns Input<()>; verify kinds match source types.
    let kinds: Vec<String> = node
        .inputs
        .iter()
        .map(|i| format!("{:?}", i.kind()))
        .collect();
    assert!(kinds.contains(&"Boolean".to_string()), "kinds: {kinds:?}");
    assert!(kinds.contains(&"String".to_string()), "kinds: {kinds:?}");

    // Data view must not leak presentation extras (message, remember).
    let json = serde_json::to_string(&node).expect("should serialize");
    assert!(!json.contains("\"message\""), "message leaked: {json}");
    assert!(!json.contains("remember"), "remember leaked: {json}");
}

#[tokio::test]
async fn inspect_widget_view_sets_has_dynamic_default() {
    let tmp = tempfile::TempDir::new().unwrap();
    // Write workspace config referencing a generators directory.
    write_workspace(tmp.path());
    std::fs::write(
        tmp.path().join("workspace.omni.yaml"),
        "projects:\n  - \"projects/**\"\ngenerators:\n  - source: local\n    path: \"generators/**\"\n",
    )
    .unwrap();
    // Write a generator with a boolean that has default but no static
    // default, and an integer that has only a static default.
    let gen_dir = tmp.path().join("generators").join("dyndefault");
    std::fs::create_dir_all(&gen_dir).unwrap();
    std::fs::write(
        gen_dir.join("generator.omni.yaml"),
        r#"
name: dyndefault
inputs:
  - type: boolean
    name: use_ssl
    message: Enable SSL?
    default: "{{ env == 'prod' }}"
  - type: integer
    name: port
    message: Port?
    default: 8080
actions: []
"#,
    )
    .unwrap();
    let api = make_api(tmp.path());

    let resp = api
        .generator_inspect("dyndefault", omni_api::InspectViewKind::Widget)
        .await
        .unwrap();
    let omni_api::GeneratorInspectResponse::Widget(node) = resp else {
        panic!("expected Widget response");
    };

    // use_ssl: has default_expr, no static default → has_dynamic_default=true, required=false
    let use_ssl = node.inputs.iter().find(|i| i.name == "use_ssl").unwrap();
    assert!(
        use_ssl.has_dynamic_default,
        "expected has_dynamic_default=true"
    );
    assert!(!use_ssl.required, "expected required=false");
    assert!(use_ssl.default.is_none(), "expected no static default");

    // port: has static default=8080 → has_dynamic_default=false, required=false
    let port = node.inputs.iter().find(|i| i.name == "port").unwrap();
    assert!(!port.has_dynamic_default);
    assert!(!port.required);
}

#[tokio::test]
async fn inspect_missing_generator_returns_err() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_workspace(tmp.path());
    let api = make_api(tmp.path());

    let result = api
        .generator_inspect("nonexistent", omni_api::InspectViewKind::Widget)
        .await;
    assert!(result.is_err(), "should error for unknown generator");
}

// ── tool list / inspect ────────────────────────────────────────────────

fn make_api_with_tool(
    workspace_dir: &Path,
) -> OmniApi<RealSys, NoopSubscriber> {
    std::fs::write(
        workspace_dir.join("workspace.omni.yaml"),
        "projects:\n  - \"projects/**\"\ntools:\n  - source: local\n    path: \"tools/**\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(workspace_dir.join("projects/alpha")).unwrap();
    std::fs::write(
        workspace_dir.join("projects/alpha/project.omni.yaml"),
        "name: alpha\ntasks:\n  build:\n    exec: echo \"alpha\"\n",
    )
    .unwrap();

    let tool_dir = workspace_dir.join("tools/summarize");
    std::fs::create_dir_all(&tool_dir).unwrap();
    std::fs::write(
        tool_dir.join("tool.omni.yaml"),
        r#"
type: js
name: summarize
description: Summarize a directory
entrypoint: ./index.mjs
inputs:
  - type: string
    name: dir
  - type: string
    name: format
    default: md
    allowed: [md, json]
"#,
    )
    .unwrap();

    make_api(workspace_dir)
}

#[tokio::test]
async fn tool_list_returns_discovered_tools() {
    let tmp = tempfile::TempDir::new().unwrap();
    let api = make_api_with_tool(tmp.path());

    let resp = api.tool_list().await.expect("tool_list should succeed");
    let names: Vec<String> =
        resp.tools.iter().map(|t| t.name.clone()).collect();
    assert_eq!(names, vec!["summarize".to_string()]);
}

#[tokio::test]
async fn tool_inspect_returns_input_schema() {
    let tmp = tempfile::TempDir::new().unwrap();
    let api = make_api_with_tool(tmp.path());

    let resp = api
        .tool_inspect("summarize")
        .await
        .expect("tool_inspect should succeed");
    assert_eq!(resp.name, "summarize");

    let schema = &resp.input_schema;
    assert!(schema.is_object(), "schema should be an object: {schema}");
    let props = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .expect("schema has properties");
    assert!(props.contains_key("dir"), "schema has `dir`: {schema}");
    assert!(
        props.contains_key("format"),
        "schema has `format`: {schema}"
    );
}

#[tokio::test]
async fn tool_inspect_missing_tool_returns_err() {
    let tmp = tempfile::TempDir::new().unwrap();
    let api = make_api_with_tool(tmp.path());

    let result = api.tool_inspect("nonexistent").await;
    assert!(result.is_err(), "should error for unknown tool");
}

// ── projection ──────────────────────────────────────────────────────────

/// Write a workspace with a single local projection source that mirrors
/// `vendor/skills` into `.agents/skills`.
fn write_projection_workspace(dir: &Path) {
    std::fs::write(
        dir.join("workspace.omni.yaml"),
        concat!(
            "projects:\n",
            "  - \"projects/**\"\n",
            "projections:\n",
            "  - source: local\n",
            "    path: ./vendor/skills\n",
            "    id: local-skills\n",
            "    routes:\n",
            "      - strategy: mirror\n",
            "        target: \"@workspace/.agents/skills\"\n",
        ),
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("vendor/skills")).unwrap();
    std::fs::write(dir.join("vendor/skills/rust.md"), b"# rust\n").unwrap();
}

#[tokio::test]
async fn projection_sync_dry_run_plans_without_writing() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_projection_workspace(tmp.path());
    let api = make_api(tmp.path());

    let resp = api
        .projection_sync(omni_api::ProjectionSyncRequest {
            dry_run: true,
            ..Default::default()
        })
        .await
        .expect("dry-run sync");

    assert!(resp.dry_run);
    assert_eq!(resp.planned.len(), 1);
    assert_eq!(resp.planned[0].dest, ".agents/skills/rust.md");
    assert!(resp.applied.is_empty(), "dry-run applies nothing");
    assert!(
        !tmp.path().join(".agents/skills/rust.md").exists(),
        "dry-run must not touch the filesystem"
    );
}

#[tokio::test]
async fn projection_sync_is_idempotent() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_projection_workspace(tmp.path());
    let api = make_api(tmp.path());

    let first = api
        .projection_sync(omni_api::ProjectionSyncRequest::default())
        .await
        .expect("first sync");
    assert_eq!(first.applied.len(), 1);
    assert!(!first.applied[0].skipped, "first sync creates the link");
    assert!(tmp.path().join(".agents/skills/rust.md").exists());

    let second = api
        .projection_sync(omni_api::ProjectionSyncRequest::default())
        .await
        .expect("second sync");
    assert_eq!(second.applied.len(), 1);
    assert!(
        second.applied[0].skipped,
        "an unchanged re-sync must be a no-op"
    );
}

#[tokio::test]
async fn projection_status_reports_ok_after_sync() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_projection_workspace(tmp.path());
    let api = make_api(tmp.path());

    api.projection_sync(omni_api::ProjectionSyncRequest::default())
        .await
        .expect("sync");

    let status = api
        .projection_status(omni_api::ProjectionStatusRequest { verbose: true })
        .await
        .expect("status");
    assert!(!status.has_problems, "a fresh sync should be all-ok");
    assert_eq!(status.ok, 1);
}

#[tokio::test]
async fn projection_unlink_removes_recorded_links() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_projection_workspace(tmp.path());
    let api = make_api(tmp.path());

    api.projection_sync(omni_api::ProjectionSyncRequest::default())
        .await
        .expect("sync");
    assert!(tmp.path().join(".agents/skills/rust.md").exists());

    let resp = api
        .projection_unlink(omni_api::ProjectionUnlinkRequest {
            id: "local-skills".to_string(),
            backup_handling: None,
        })
        .await
        .expect("unlink");
    assert_eq!(resp.removed.len(), 1);
    assert!(!tmp.path().join(".agents/skills/rust.md").exists());
}

/// Write a workspace with two local projection sources mirroring into distinct
/// destinations.
fn write_two_source_workspace(dir: &Path) {
    std::fs::write(
        dir.join("workspace.omni.yaml"),
        concat!(
            "projects:\n",
            "  - \"projects/**\"\n",
            "projections:\n",
            "  - source: local\n",
            "    path: ./vendor/a\n",
            "    id: skills-a\n",
            "    routes:\n",
            "      - strategy: mirror\n",
            "        target: \"@workspace/.agents/a\"\n",
            "  - source: local\n",
            "    path: ./vendor/b\n",
            "    id: skills-b\n",
            "    routes:\n",
            "      - strategy: mirror\n",
            "        target: \"@workspace/.agents/b\"\n",
        ),
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("vendor/a")).unwrap();
    std::fs::write(dir.join("vendor/a/one.md"), b"# one\n").unwrap();
    std::fs::create_dir_all(dir.join("vendor/b")).unwrap();
    std::fs::write(dir.join("vendor/b/two.md"), b"# two\n").unwrap();
}

/// The same workspace with `skills-b` removed from config.
fn write_single_source_workspace(dir: &Path) {
    std::fs::write(
        dir.join("workspace.omni.yaml"),
        concat!(
            "projects:\n",
            "  - \"projects/**\"\n",
            "projections:\n",
            "  - source: local\n",
            "    path: ./vendor/a\n",
            "    id: skills-a\n",
            "    routes:\n",
            "      - strategy: mirror\n",
            "        target: \"@workspace/.agents/a\"\n",
        ),
    )
    .unwrap();
}

#[tokio::test]
async fn projection_full_sync_reconciles_removed_source() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_two_source_workspace(tmp.path());

    make_api(tmp.path())
        .projection_sync(omni_api::ProjectionSyncRequest::default())
        .await
        .expect("initial sync");
    assert!(tmp.path().join(".agents/a/one.md").exists());
    assert!(tmp.path().join(".agents/b/two.md").exists());

    write_single_source_workspace(tmp.path());
    let resp = make_api(tmp.path())
        .projection_sync(omni_api::ProjectionSyncRequest::default())
        .await
        .expect("reconciling sync");

    assert!(
        resp.removed.iter().any(|r| r.ends_with("two.md")),
        "dropped source's dest reported as removed: {:?}",
        resp.removed
    );
    assert!(
        !tmp.path().join(".agents/b/two.md").exists(),
        "dropped source's dest is removed"
    );
    assert!(
        tmp.path().join(".agents/a/one.md").exists(),
        "surviving source is untouched"
    );

    let status = make_api(tmp.path())
        .projection_status(omni_api::ProjectionStatusRequest { verbose: true })
        .await
        .expect("status");
    assert_eq!(status.entries.len(), 1, "ledger holds only the survivor");
    assert_eq!(status.entries[0].source_id, "skills-a");
}

#[tokio::test]
async fn projection_filtered_sync_does_not_reconcile_orphans() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_two_source_workspace(tmp.path());

    make_api(tmp.path())
        .projection_sync(omni_api::ProjectionSyncRequest::default())
        .await
        .expect("initial sync");

    write_single_source_workspace(tmp.path());
    let resp = make_api(tmp.path())
        .projection_sync(omni_api::ProjectionSyncRequest {
            source: Some("skills-a".to_string()),
            ..Default::default()
        })
        .await
        .expect("filtered sync");

    assert!(
        resp.removed.is_empty(),
        "a filtered run reconciles nothing: {:?}",
        resp.removed
    );
    assert!(
        tmp.path().join(".agents/b/two.md").exists(),
        "orphan survives a filtered run"
    );

    let resp = make_api(tmp.path())
        .projection_sync(omni_api::ProjectionSyncRequest::default())
        .await
        .expect("later full sync");
    assert!(
        !tmp.path().join(".agents/b/two.md").exists(),
        "a later full sync removes the orphan"
    );
    assert!(resp.removed.iter().any(|r| r.ends_with("two.md")));
}

#[tokio::test]
async fn projection_dry_run_reports_orphans_without_removing() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_two_source_workspace(tmp.path());

    make_api(tmp.path())
        .projection_sync(omni_api::ProjectionSyncRequest::default())
        .await
        .expect("initial sync");

    write_single_source_workspace(tmp.path());
    let resp = make_api(tmp.path())
        .projection_sync(omni_api::ProjectionSyncRequest {
            dry_run: true,
            ..Default::default()
        })
        .await
        .expect("dry-run sync");

    assert!(
        resp.removed.iter().any(|r| r.ends_with("two.md")),
        "dry-run reports the orphan: {:?}",
        resp.removed
    );
    assert!(
        tmp.path().join(".agents/b/two.md").exists(),
        "dry-run removes nothing"
    );

    let status = make_api(tmp.path())
        .projection_status(omni_api::ProjectionStatusRequest { verbose: true })
        .await
        .expect("status");
    assert_eq!(status.entries.len(), 2, "dry-run leaves the ledger intact");
}

/// A source that ships its own `projection.omni.yaml` and a workspace that omits
/// `routes`, so the owned manifest is inherited.
fn write_owned_projection_workspace(dir: &Path) {
    std::fs::write(
        dir.join("workspace.omni.yaml"),
        concat!(
            "projects:\n",
            "  - \"projects/**\"\n",
            "projections:\n",
            "  - source: local\n",
            "    path: ./vendor/skills\n",
            "    id: local-skills\n",
        ),
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("vendor/skills")).unwrap();
    std::fs::write(dir.join("vendor/skills/rust.md"), b"# rust\n").unwrap();
    std::fs::write(
        dir.join("vendor/skills/projection.omni.yaml"),
        concat!(
            "routes:\n",
            "  - strategy: mirror\n",
            "    scope: \"*.md\"\n",
            "    target: \"@workspace/.agents/skills\"\n",
        ),
    )
    .unwrap();
}

#[tokio::test]
async fn projection_owned_manifest_is_inherited() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_owned_projection_workspace(tmp.path());
    let api = make_api(tmp.path());

    let resp = api
        .projection_sync(omni_api::ProjectionSyncRequest::default())
        .await
        .expect("sync with inherited owned routes");
    assert_eq!(resp.applied.len(), 1);
    assert!(tmp.path().join(".agents/skills/rust.md").exists());
}

#[tokio::test]
async fn projection_workspace_routes_override_owned_manifest() {
    let tmp = tempfile::TempDir::new().unwrap();
    // Source ships a manifest targeting `.agents/skills`, but the workspace
    // overrides it to `.other`.
    std::fs::write(
        tmp.path().join("workspace.omni.yaml"),
        concat!(
            "projects:\n",
            "  - \"projects/**\"\n",
            "projections:\n",
            "  - source: local\n",
            "    path: ./vendor/skills\n",
            "    id: local-skills\n",
            "    routes:\n",
            "      - strategy: mirror\n",
            "        scope: \"*.md\"\n",
            "        target: \"@workspace/.other\"\n",
        ),
    )
    .unwrap();
    std::fs::create_dir_all(tmp.path().join("vendor/skills")).unwrap();
    std::fs::write(tmp.path().join("vendor/skills/rust.md"), b"# rust\n")
        .unwrap();
    std::fs::write(
        tmp.path().join("vendor/skills/projection.omni.yaml"),
        "routes:\n  - strategy: mirror\n    scope: \"*.md\"\n    target: \"@workspace/.agents/skills\"\n",
    )
    .unwrap();

    let api = make_api(tmp.path());
    api.projection_sync(omni_api::ProjectionSyncRequest::default())
        .await
        .expect("sync with workspace override");

    assert!(
        tmp.path().join(".other/rust.md").exists(),
        "override target used"
    );
    assert!(
        !tmp.path().join(".agents/skills/rust.md").exists(),
        "owned manifest target must be ignored when overridden"
    );
}

#[tokio::test]
async fn projection_empty_routes_is_an_error() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("workspace.omni.yaml"),
        concat!(
            "projects:\n",
            "  - \"projects/**\"\n",
            "projections:\n",
            "  - source: local\n",
            "    path: ./vendor/skills\n",
            "    id: local-skills\n",
            "    routes: []\n",
        ),
    )
    .unwrap();
    std::fs::create_dir_all(tmp.path().join("vendor/skills")).unwrap();

    let api = make_api(tmp.path());
    let result = api
        .projection_sync(omni_api::ProjectionSyncRequest {
            dry_run: true,
            ..Default::default()
        })
        .await;
    assert!(result.is_err(), "an empty routes list must fail fast");
}

#[tokio::test]
async fn projection_no_routes_anywhere_is_an_error() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("workspace.omni.yaml"),
        concat!(
            "projects:\n",
            "  - \"projects/**\"\n",
            "projections:\n",
            "  - source: local\n",
            "    path: ./vendor/skills\n",
            "    id: local-skills\n",
        ),
    )
    .unwrap();
    std::fs::create_dir_all(tmp.path().join("vendor/skills")).unwrap();
    std::fs::write(tmp.path().join("vendor/skills/rust.md"), b"# rust\n")
        .unwrap();

    let api = make_api(tmp.path());
    let result = api
        .projection_sync(omni_api::ProjectionSyncRequest::default())
        .await;
    assert!(
        result.is_err(),
        "a source with no workspace routes and no manifest is an error"
    );
}

#[tokio::test]
async fn projection_pattern_dir_links_directories() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("workspace.omni.yaml"),
        concat!(
            "projects:\n",
            "  - \"projects/**\"\n",
            "projections:\n",
            "  - source: local\n",
            "    path: ./vendor/skills\n",
            "    id: local-skills\n",
            "    routes:\n",
            "      - strategy: pattern\n",
            "        target: \"@workspace/.agents/skills\"\n",
            "        rules:\n",
            "          - match: \"*\"\n",
            "            match_kind: dir\n",
            "            dest: \"@target/{basename}\"\n",
        ),
    )
    .unwrap();
    std::fs::create_dir_all(tmp.path().join("vendor/skills/tdd")).unwrap();
    std::fs::write(tmp.path().join("vendor/skills/tdd/SKILL.md"), b"# tdd\n")
        .unwrap();
    std::fs::create_dir_all(tmp.path().join("vendor/skills/review")).unwrap();
    std::fs::write(
        tmp.path().join("vendor/skills/review/SKILL.md"),
        b"# review\n",
    )
    .unwrap();

    let api = make_api(tmp.path());
    let first = api
        .projection_sync(omni_api::ProjectionSyncRequest::default())
        .await
        .expect("dir sync");
    assert_eq!(first.applied.len(), 2, "one link per matched directory");
    // The linked directories resolve through to the source content.
    assert_eq!(
        std::fs::read_to_string(tmp.path().join(".agents/skills/tdd/SKILL.md"))
            .unwrap(),
        "# tdd\n"
    );
    assert!(tmp.path().join(".agents/skills/review/SKILL.md").exists());

    let second = api
        .projection_sync(omni_api::ProjectionSyncRequest::default())
        .await
        .expect("idempotent dir re-sync");
    assert!(
        second.applied.iter().all(|a| a.skipped),
        "an unchanged directory-link re-sync must be a no-op"
    );
}

/// Two local sources that both mirror into `.agents/shared`, so their `x.md`
/// files resolve to one destination. Source `a` also has a non-colliding file.
fn write_colliding_two_source_workspace(dir: &Path) {
    std::fs::write(
        dir.join("workspace.omni.yaml"),
        concat!(
            "projects:\n",
            "  - \"projects/**\"\n",
            "projections:\n",
            "  - source: local\n",
            "    path: ./vendor/a\n",
            "    id: skills-a\n",
            "    routes:\n",
            "      - strategy: mirror\n",
            "        target: \"@workspace/.agents/shared\"\n",
            "  - source: local\n",
            "    path: ./vendor/b\n",
            "    id: skills-b\n",
            "    routes:\n",
            "      - strategy: mirror\n",
            "        target: \"@workspace/.agents/shared\"\n",
        ),
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("vendor/a")).unwrap();
    std::fs::write(dir.join("vendor/a/x.md"), b"# a\n").unwrap();
    std::fs::write(dir.join("vendor/a/only-a.md"), b"# only a\n").unwrap();
    std::fs::create_dir_all(dir.join("vendor/b")).unwrap();
    std::fs::write(dir.join("vendor/b/x.md"), b"# b\n").unwrap();
}

#[tokio::test]
async fn projection_run_wide_collision_aborts_before_any_write() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_colliding_two_source_workspace(tmp.path());
    let api = make_api(tmp.path());

    let result = api
        .projection_sync(omni_api::ProjectionSyncRequest::default())
        .await;
    assert!(
        result.is_err(),
        "a cross-source destination collision must abort the run"
    );

    assert!(
        !tmp.path().join(".agents/shared/x.md").exists(),
        "the colliding destination must not be written"
    );
    assert!(
        !tmp.path().join(".agents/shared/only-a.md").exists(),
        "no source is materialized when the whole run aborts"
    );

    let status = api
        .projection_status(omni_api::ProjectionStatusRequest { verbose: true })
        .await
        .expect("status");
    assert!(
        status.entries.is_empty(),
        "an aborted run persists no ledger links"
    );
}

#[tokio::test]
async fn projection_dry_run_reports_collisions_without_writing() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_colliding_two_source_workspace(tmp.path());
    let api = make_api(tmp.path());

    let result = api
        .projection_sync(omni_api::ProjectionSyncRequest {
            dry_run: true,
            ..Default::default()
        })
        .await;
    assert!(
        result.is_err(),
        "a dry run over a conflicting plan reports the conflict"
    );
    assert!(
        !tmp.path().join(".agents/shared/x.md").exists(),
        "a dry run must not touch the filesystem"
    );
}

#[tokio::test]
async fn projection_targeted_source_applies_only_that_source() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_two_source_workspace(tmp.path());
    let api = make_api(tmp.path());

    let resp = api
        .projection_sync(omni_api::ProjectionSyncRequest {
            source: Some("skills-a".to_string()),
            ..Default::default()
        })
        .await
        .expect("targeted sync");

    assert_eq!(resp.applied.len(), 1, "only the targeted source applies");
    assert!(tmp.path().join(".agents/a/one.md").exists());
    assert!(
        !tmp.path().join(".agents/b/two.md").exists(),
        "an untargeted source is left untouched"
    );
}
