//! Live end-to-end enforcement test: spawns a **real Deno process** through the
//! generator's [`LazyScriptRunner`] and proves that a hijacked-style script is
//! confined to exactly the capability policy its generator declared.
//!
//! The scenario mirrors the pilot design:
//!
//! * policy = `allow fs.read @workspace/**` + `allow fs.write @workspace/**` +
//!   `deny fs.write @workspace/generated/**`.
//! * a single script writes to a path handed to it via `data.target`.
//! * writing **inside** the allowed subtree succeeds and the bytes reach the
//!   (guarded) transactional sys.
//! * writing **into the denied subtree** is refused by the in-process broker
//!   ([`PolicyEnforcingSys`](omni_capability_sys::PolicyEnforcingSys)); the RPC
//!   surfaces a `PermissionDenied` error and nothing is written.
//!
//! The test is skipped (not failed) when `deno` is not on `PATH`, so it is safe
//! to run in environments without a JS runtime.

use std::path::{Path, PathBuf};

use bridge_rpc_runner::DelegatingJsRuntimeOption;
use omni_capabilities::{CapabilityRules, PathRoots, Root};
use omni_generator::{
    EffectivePolicy, JsScriptRunner, LazyScriptRunner, ScriptInvocation,
    ScriptParams, TransactionSys,
};
use omni_generator_configurations::{Generator, GeneratorContext};
use system_traits::FsReadAsync;
use system_traits::impls::RealSys;

/// The single script both invocations run. It writes `data.content` to the
/// path in `data.target` using the *bridged* system handle, so every write is
/// authorized by the Rust-side broker before it is buffered.
const WRITE_SCRIPT: &str = r#"
export default async function (ctx) {
    await ctx.sys.fs.writeStringToFile(ctx.data.target, ctx.data.content);
}
"#;

fn deno_available() -> bool {
    which::which("deno").is_ok()
}

/// The pilot policy: read/write anywhere in the workspace, except writes into
/// `generated/`, which are denied (deny-dominant).
fn enforced_policy(ws: &Path, output_dir: &Path) -> EffectivePolicy {
    let chain: CapabilityRules<Generator> = serde_json::from_str(
        r#"[
            { "access": "allow", "domain": "fs.read",  "patterns": ["@workspace/**"] },
            { "access": "allow", "domain": "fs.write", "patterns": ["@workspace/**"] },
            { "access": "deny",  "domain": "fs.write", "patterns": ["@workspace/generated/**"] }
        ]"#,
    )
    .expect("valid capabilities chain");

    EffectivePolicy {
        levels: vec![chain],
        roots: PathRoots::new()
            .with(Root::Workspace, ws)
            .with(Root::Project, output_dir),
        context: GeneratorContext {
            action: Some("run-javascript".to_string()),
            target: None,
        },
        strictness: Default::default(),
    }
}

#[tokio::test]
async fn deno_broker_confines_writes_to_the_declared_policy() {
    if !deno_available() {
        eprintln!("skipping: `deno` not found on PATH");
        return;
    }

    // ── workspace layout ────────────────────────────────────────────────────
    let ws = tempfile::tempdir().expect("tempdir");
    let ws_dir: PathBuf = ws.path().to_path_buf();
    let script_path = ws_dir.join("write.mjs");
    std::fs::write(&script_path, WRITE_SCRIPT).expect("write script");

    let allowed_target = ws_dir.join("allowed.txt");
    let denied_target = ws_dir.join("generated").join("blocked.txt");

    // ── enforced runner over a real, transactional filesystem ───────────────
    let sys = TransactionSys::new(RealSys);
    // Retain a clone to inspect the buffered overlay after the scripts run;
    // `TransactionSys` shares its state behind an `Arc`, so this observes the
    // same writes the JS process performed through the broker.
    let observer = sys.clone();

    let runner =
        LazyScriptRunner::new(sys, ws_dir.clone(), "deno-e2e-test".to_string());

    let policy = enforced_policy(&ws_dir, &ws_dir);

    // ── 1. an allowed write succeeds and reaches the guarded sys ────────────
    let allowed_inv = ScriptInvocation {
        path: script_path.to_string_lossy().into_owned(),
        params: ScriptParams {
            dry_run: false,
            data: serde_json::json!({
                "target": allowed_target.to_string_lossy(),
                "content": "hello from an allowed write",
            }),
            output_dir: ws_dir.to_string_lossy().into_owned(),
        },
    };

    runner
        .run_scripts(
            DelegatingJsRuntimeOption::Deno,
            &policy,
            std::slice::from_ref(&allowed_inv),
        )
        .await
        .expect("an allowed write must succeed");

    let written = observer
        .fs_read_async(&allowed_target)
        .await
        .expect("allowed file should be present in the transaction overlay");
    assert_eq!(
        written.into_owned(),
        b"hello from an allowed write".to_vec(),
        "the allowed write did not reach the guarded sys"
    );

    // ── 2. a denied write is refused by the broker ──────────────────────────
    let denied_inv = ScriptInvocation {
        path: script_path.to_string_lossy().into_owned(),
        params: ScriptParams {
            dry_run: false,
            data: serde_json::json!({
                "target": denied_target.to_string_lossy(),
                "content": "this must never be written",
            }),
            output_dir: ws_dir.to_string_lossy().into_owned(),
        },
    };

    let err = runner
        .run_scripts(
            DelegatingJsRuntimeOption::Deno,
            &policy,
            std::slice::from_ref(&denied_inv),
        )
        .await
        .expect_err("a write into the denied subtree must fail");

    let msg = err.to_string();
    eprintln!("denied-write error surfaced to the runner: {msg}");
    assert!(
        msg.contains("capability policy denied") && msg.contains("fs.write"),
        "error should be the broker's capability denial, got: {msg}"
    );

    // Nothing was written into the denied subtree.
    assert!(
        observer.fs_read_async(&denied_target).await.is_err(),
        "the denied write must not have reached the sys"
    );

    runner.shutdown().await;
}

/// Proves the symlink-escape backstop end to end through the real generator
/// path: a symlink *inside* the workspace whose target is *outside* it cannot be
/// used to write past the policy. The bridged write is authorized in the Rust
/// host (which is not itself OS-sandboxed), so this exercises the broker's
/// canonicalize-and-re-authorize backstop specifically, not the runtime/OS
/// floor. Unix-only (creates a real symlink); the backstop itself is
/// cross-platform.
#[cfg(unix)]
#[tokio::test]
async fn deno_broker_denies_a_write_escaping_via_a_symlinked_parent() {
    use std::os::unix::fs::symlink;

    if !deno_available() {
        eprintln!("skipping: `deno` not found on PATH");
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    // The workspace is a subdirectory; `secret/` is a sibling *outside* it.
    let ws_dir = tmp.path().join("ws");
    let secret_dir = tmp.path().join("secret");
    std::fs::create_dir_all(&ws_dir).expect("create ws");
    std::fs::create_dir_all(&secret_dir).expect("create secret");

    // A directory symlink inside the workspace that points outside it.
    symlink(&secret_dir, ws_dir.join("outlink")).expect("create symlink");

    let script_path = ws_dir.join("write.mjs");
    std::fs::write(&script_path, WRITE_SCRIPT).expect("write script");

    let sys = TransactionSys::new(RealSys);
    let observer = sys.clone();
    let runner =
        LazyScriptRunner::new(sys, ws_dir.clone(), "deno-e2e-test".to_string());
    let policy = enforced_policy(&ws_dir, &ws_dir);

    // The lexical path `ws/outlink/new.txt` is *inside* `@workspace/**`, but its
    // symlinked parent redirects the real target into `secret/`.
    let escaping_target = ws_dir.join("outlink").join("new.txt");
    let inv = ScriptInvocation {
        path: script_path.to_string_lossy().into_owned(),
        params: ScriptParams {
            dry_run: false,
            data: serde_json::json!({
                "target": escaping_target.to_string_lossy(),
                "content": "this must never escape the workspace",
            }),
            output_dir: ws_dir.to_string_lossy().into_owned(),
        },
    };

    let err = runner
        .run_scripts(
            DelegatingJsRuntimeOption::Deno,
            &policy,
            std::slice::from_ref(&inv),
        )
        .await
        .expect_err("a write escaping via a symlinked parent must be denied");

    let msg = err.to_string();
    eprintln!("symlink-escape error surfaced to the runner: {msg}");
    assert!(
        msg.contains("capability policy denied") && msg.contains("resolves to"),
        "error should be the broker's symlink-escape denial, got: {msg}"
    );

    // Nothing was written at the real (escaped) target or in the overlay.
    assert!(
        !secret_dir.join("new.txt").exists(),
        "the denied write leaked outside the workspace"
    );
    assert!(
        observer.fs_read_async(&escaping_target).await.is_err(),
        "the denied write must not have reached the sys"
    );

    runner.shutdown().await;
}

/// Proves the mandatory-floor guarantee end to end: a workspace-level `deny`,
/// merged ahead of a generator-level `allow` that tries to re-open it, still
/// wins through the real layered broker + Deno. A deeper `allow` can only
/// *narrow*, never widen, the workspace floor (attenuation / deny-dominant).
#[tokio::test]
async fn deno_workspace_deny_overrides_generator_allow() {
    if !deno_available() {
        eprintln!("skipping: `deno` not found on PATH");
        return;
    }

    let ws = tempfile::tempdir().expect("tempdir");
    let ws_dir: PathBuf = ws.path().to_path_buf();
    let script_path = ws_dir.join("write.mjs");
    std::fs::write(&script_path, WRITE_SCRIPT).expect("write script");

    // Workspace floor: read/write the workspace, but never write `generated/`.
    let workspace: CapabilityRules<Generator> = serde_json::from_str(
        r#"[
            { "access": "allow", "domain": "fs.read",  "patterns": ["@workspace/**"] },
            { "access": "allow", "domain": "fs.write", "patterns": ["@workspace/**"] },
            { "access": "deny",  "domain": "fs.write", "patterns": ["@workspace/generated/**"] }
        ]"#,
    )
    .expect("valid workspace chain");

    // Generator tries to widen the floor by re-allowing the denied subtree.
    let generator: CapabilityRules<Generator> = serde_json::from_str(
        r#"[{ "access": "allow", "domain": "fs.write", "patterns": ["@workspace/generated/**"] }]"#,
    )
    .expect("valid generator chain");

    // Layered exactly as `run_javascript` does: the workspace floor is the
    // outermost level, the generator's own policy the next. Under the
    // shrink-only model the workspace-level `deny` dominates and the generator
    // cannot re-open it.
    let policy = EffectivePolicy {
        levels: vec![workspace, generator],
        roots: PathRoots::new()
            .with(Root::Workspace, &ws_dir)
            .with(Root::Project, &ws_dir),
        context: GeneratorContext {
            action: Some("run-javascript".to_string()),
            target: None,
        },
        strictness: Default::default(),
    };

    let sys = TransactionSys::new(RealSys);
    let runner =
        LazyScriptRunner::new(sys, ws_dir.clone(), "deno-e2e-test".to_string());

    let denied_target = ws_dir.join("generated").join("blocked.txt");
    let inv = ScriptInvocation {
        path: script_path.to_string_lossy().into_owned(),
        params: ScriptParams {
            dry_run: false,
            data: serde_json::json!({
                "target": denied_target.to_string_lossy(),
                "content": "a generator allow must not re-open a workspace deny",
            }),
            output_dir: ws_dir.to_string_lossy().into_owned(),
        },
    };

    let err = runner
        .run_scripts(
            DelegatingJsRuntimeOption::Deno,
            &policy,
            std::slice::from_ref(&inv),
        )
        .await
        .expect_err("the workspace deny must dominate the generator allow");

    let msg = err.to_string();
    assert!(
        msg.contains("capability policy denied") && msg.contains("fs.write"),
        "error should be the broker's capability denial, got: {msg}"
    );

    runner.shutdown().await;
}
