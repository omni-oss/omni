//! Integration tests for hierarchical generator sessions: walk-up restore,
//! delta-only save, `root: true` sealing, and the `inherit_session` opt-out.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use maps::UnorderedMap;
use omni_api::{GeneratorRunRequest, OmniApi};
use omni_generator_configurations::Generator;
use omni_input_provider::InputProvider;
use omni_input_provider::scripted::ScriptedInputProvider;
use omni_messages::NoopSubscriber;
use omni_tracing_subscriber::TracingConfig;
use system_traits::impls::RealSys;
use value_bag::{OwnedValueBag, ValueBag};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Write a workspace with a `scaffold` generator that has two remembered string
/// inputs (`subject` default `world`, `scope` default `none`) and no actions.
fn write_scaffold_workspace(root: &Path) {
    std::fs::write(
        root.join("workspace.omni.yaml"),
        "projects:\n  - \"projects/**\"\ngenerators:\n  - source: local\n    path: \"generators/**\"\n",
    )
    .unwrap();
    let gen_dir = root.join("generators/scaffold");
    std::fs::create_dir_all(&gen_dir).unwrap();
    std::fs::write(
        gen_dir.join("generator.omni.yaml"),
        r#"
name: scaffold
inputs:
  - type: string
    name: subject
    message: Subject
    default: world
    remember: true
  - type: string
    name: scope
    message: Scope
    default: none
    remember: true
actions: []
"#,
    )
    .unwrap();
}

fn make_api(root: &Path) -> OmniApi<RealSys, NoopSubscriber> {
    let ctx = omni_context::Context::new(
        RealSys,
        "development",
        root,
        false,
        "workspace.omni.yaml",
        None,
        &TracingConfig::disabled(),
    )
    .expect("context creation failed");
    OmniApi::new_with_sys(ctx, NoopSubscriber)
}

fn write_session(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn session_file(dir: &Path) -> PathBuf {
    dir.join(".omni/generator.json")
}

fn read_session(path: &Path) -> serde_json::Value {
    let raw = std::fs::read_to_string(path).unwrap();
    serde_json::from_str(&raw).unwrap()
}

fn empty_provider() -> Arc<dyn InputProvider<Generator>> {
    Arc::new(ScriptedInputProvider::new(std::iter::empty::<(
        String,
        String,
    )>()))
}

fn input_values(pairs: &[(&str, &str)]) -> UnorderedMap<String, OwnedValueBag> {
    pairs
        .iter()
        .map(|(k, v)| {
            (
                k.to_string(),
                ValueBag::capture_serde1(&v.to_string()).to_owned(),
            )
        })
        .collect()
}

fn base_req(output_dir: PathBuf) -> GeneratorRunRequest {
    GeneratorRunRequest {
        name: Some("scaffold".to_string()),
        output_dir,
        project: None,
        target: UnorderedMap::default(),
        dry_run: false,
        overwrite: None,
        save_session: Some(true),
        ignore_session: None,
        inherit_session: None,
        input_values: UnorderedMap::default(),
        use_defaults: true,
        input_provider: empty_provider(),
        max_depth: None,
    }
}

/// Canonicalized workspace root so paths line up with `Context`'s own
/// canonicalization of the root dir.
fn setup() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    write_scaffold_workspace(&root);
    (tmp, root)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn inherited_value_writes_no_leaf_file() {
    let (_tmp, root) = setup();
    write_session(
        &session_file(&root),
        r#"{ "version": "1.0.0", "generators": { "scaffold": { "inputs": { "subject": "inherited", "scope": "@acme" } } } }"#,
    );

    let out = root.join("app/web");
    std::fs::create_dir_all(&out).unwrap();

    let resp = make_api(&root)
        .generator_run(base_req(out.clone()))
        .await
        .expect("run should succeed");

    assert!(!resp.session_saved);
    assert!(
        !session_file(&out).exists(),
        "an all-inherited run must not write a leaf session file"
    );
}

#[tokio::test]
async fn leaf_override_persists_only_the_delta() {
    let (_tmp, root) = setup();
    write_session(
        &session_file(&root),
        r#"{ "version": "1.0.0", "generators": { "scaffold": { "inputs": { "subject": "inherited", "scope": "@acme" } } } }"#,
    );

    let out = root.join("app/web");
    std::fs::create_dir_all(&out).unwrap();

    let mut req = base_req(out.clone());
    req.input_values = input_values(&[("subject", "local")]);

    let resp = make_api(&root)
        .generator_run(req)
        .await
        .expect("run succeeds");
    assert!(resp.session_saved);

    let leaf = read_session(&session_file(&out));
    let inputs = &leaf["generators"]["scaffold"]["inputs"];
    assert_eq!(inputs["subject"], serde_json::json!("local"));
    // `scope` was inherited unchanged and must not be copied into the leaf.
    assert!(inputs.get("scope").is_none(), "leaf leaked scope: {leaf}");
}

#[tokio::test]
async fn explicit_value_equal_to_inherited_is_pinned() {
    let (_tmp, root) = setup();
    write_session(
        &session_file(&root),
        r#"{ "version": "1.0.0", "generators": { "scaffold": { "inputs": { "subject": "inherited" } } } }"#,
    );

    let out = root.join("app/web");
    std::fs::create_dir_all(&out).unwrap();

    let mut req = base_req(out.clone());
    // Same value as inherited, but provided explicitly => must persist.
    req.input_values = input_values(&[("subject", "inherited")]);

    let resp = make_api(&root)
        .generator_run(req)
        .await
        .expect("run succeeds");
    assert!(resp.session_saved);

    let leaf = read_session(&session_file(&out));
    assert_eq!(
        leaf["generators"]["scaffold"]["inputs"]["subject"],
        serde_json::json!("inherited")
    );
}

#[tokio::test]
async fn root_true_seals_inheritance_above_it() {
    let (_tmp, root) = setup();
    // Workspace root carries `scope`; the sealing mid-file does not.
    write_session(
        &session_file(&root),
        r#"{ "version": "1.0.0", "generators": { "scaffold": { "inputs": { "subject": "root-subject", "scope": "root-scope" } } } }"#,
    );
    let mid = root.join("mid");
    write_session(
        &session_file(&mid),
        r#"{ "version": "1.0.0", "root": true, "generators": { "scaffold": { "inputs": { "subject": "mid-subject" } } } }"#,
    );

    let out = mid.join("leaf");
    std::fs::create_dir_all(&out).unwrap();

    let resp = make_api(&root)
        .generator_run(base_req(out.clone()))
        .await
        .expect("run succeeds");
    assert!(resp.session_saved);

    // The seal stops the walk at `mid`, so `scope` is never inherited from the
    // workspace root and resolves to its default instead.
    let leaf = read_session(&session_file(&out));
    assert_eq!(
        leaf["generators"]["scaffold"]["inputs"]["scope"],
        serde_json::json!("none"),
        "root-level scope must not cross the seal: {leaf}"
    );
    assert!(
        leaf["generators"]["scaffold"]["inputs"]
            .get("subject")
            .is_none()
    );
}

#[tokio::test]
async fn inherit_session_false_ignores_ancestors() {
    let (_tmp, root) = setup();
    write_session(
        &session_file(&root),
        r#"{ "version": "1.0.0", "generators": { "scaffold": { "inputs": { "subject": "inherited" } } } }"#,
    );

    let out = root.join("app/web");
    std::fs::create_dir_all(&out).unwrap();

    let mut req = base_req(out.clone());
    req.inherit_session = Some(false);

    let resp = make_api(&root)
        .generator_run(req)
        .await
        .expect("run succeeds");
    assert!(resp.session_saved);

    // With inheritance off, the ancestor is invisible, so the resolved default
    // `subject` is a full local delta rather than an inherited-equal value.
    let leaf = read_session(&session_file(&out));
    assert_eq!(
        leaf["generators"]["scaffold"]["inputs"]["subject"],
        serde_json::json!("world")
    );
}

#[tokio::test]
async fn dry_run_writes_nothing() {
    let (_tmp, root) = setup();

    let out = root.join("app/web");
    std::fs::create_dir_all(&out).unwrap();

    let mut req = base_req(out.clone());
    req.dry_run = true;
    req.input_values = input_values(&[("subject", "local")]);

    let resp = make_api(&root)
        .generator_run(req)
        .await
        .expect("run succeeds");
    assert!(!resp.session_saved);
    assert!(!session_file(&out).exists());
}

#[tokio::test]
async fn unchanged_rerun_is_idempotent() {
    let (_tmp, root) = setup();

    let out = root.join("app/web");
    std::fs::create_dir_all(&out).unwrap();

    let mut first = base_req(out.clone());
    first.input_values = input_values(&[("subject", "local")]);
    make_api(&root)
        .generator_run(first)
        .await
        .expect("first run");

    let written = read_session(&session_file(&out));

    // Re-run with the leaf file now present; the leaf value is restored and the
    // delta is unchanged, so nothing should be rewritten.
    let mut second = base_req(out.clone());
    second.input_values = input_values(&[("subject", "local")]);
    let resp = make_api(&root)
        .generator_run(second)
        .await
        .expect("second run");

    assert!(!resp.session_saved, "an unchanged re-run must not save");
    assert_eq!(read_session(&session_file(&out)), written);
}

#[tokio::test]
async fn ancestor_shared_value_is_not_rewritten_in_leaf() {
    let (_tmp, root) = setup();
    write_session(
        &session_file(&root),
        r#"{ "version": "1.0.0", "shared": { "inputs": { "scope": "@acme" } } }"#,
    );

    let out = root.join("app/web");
    std::fs::create_dir_all(&out).unwrap();

    let resp = make_api(&root)
        .generator_run(base_req(out.clone()))
        .await
        .expect("run succeeds");
    assert!(resp.session_saved);

    // `scope` was provided by the ancestor's shared block and must not be copied
    // into the leaf; `subject` resolves to its default and is a genuine local
    // delta.
    let leaf = read_session(&session_file(&out));
    let inputs = &leaf["generators"]["scaffold"]["inputs"];
    assert_eq!(inputs["subject"], serde_json::json!("world"));
    assert!(
        inputs.get("scope").is_none(),
        "shared scope leaked into the leaf: {leaf}"
    );
}

#[tokio::test]
async fn leaf_equal_to_own_shared_writes_nothing() {
    let (_tmp, root) = setup();

    let out = root.join("app/web");
    std::fs::create_dir_all(&out).unwrap();
    // The output directory's own file carries a shared block covering every
    // remembered input, so a plain run resolves entirely from it and the delta
    // is empty.
    write_session(
        &session_file(&out),
        r#"{ "version": "1.0.0", "shared": { "inputs": { "subject": "shared-subject", "scope": "shared-scope" } } }"#,
    );

    let resp = make_api(&root)
        .generator_run(base_req(out.clone()))
        .await
        .expect("run succeeds");

    assert!(
        !resp.session_saved,
        "a run equal to the file's own shared block writes nothing"
    );
    // The shared block is preserved and no generator entry is added.
    let file = read_session(&session_file(&out));
    assert_eq!(
        file["shared"]["inputs"]["scope"],
        serde_json::json!("shared-scope")
    );
    assert!(
        file.get("generators").is_none(),
        "leaf gained a generator entry: {file}"
    );
}
