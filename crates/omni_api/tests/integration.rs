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
