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
        RealSys::default(),
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
