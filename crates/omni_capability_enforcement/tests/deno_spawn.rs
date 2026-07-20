//! Integration test: take a real capability policy, lower it to Deno flags with
//! [`DenoFlags`], then **actually spawn Deno** and prove the process is confined
//! to the policy.
//!
//! The test is skipped (not failed) when the `deno` binary is unavailable, so
//! it stays friendly on machines/CI without Deno installed.

use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use omni_capabilities::{CapabilityRules, PathRoots, Root, project};
use omni_capability_enforcement::{DenoFlags, EnforcementBackend, build_plan};

/// A Deno script that: (1) reads an allowed file (must succeed), then (2) tries
/// to read a denied file (must be blocked by the permission sandbox). Exit code
/// communicates the outcome so the Rust side can assert precisely.
const PROBE_SCRIPT: &str = r#"
const [okPath, secretPath] = Deno.args;

// (1) The allowed read must succeed.
let ok;
try {
  ok = Deno.readTextFileSync(okPath);
} catch (e) {
  console.error("FAIL allowed-read threw " + e.name + ": " + e.message);
  Deno.exit(10);
}
if (ok.trim() !== "OK") {
  console.error("FAIL unexpected allowed content: " + JSON.stringify(ok));
  Deno.exit(11);
}

// (2) The denied read must throw a permission error.
try {
  Deno.readTextFileSync(secretPath);
  console.error("SECURITY denied path was readable");
  Deno.exit(3);
} catch (e) {
  // Deno 2 throws `NotCapable`; Deno 1 threw `PermissionDenied`.
  if (e.name === "NotCapable" || e.name === "PermissionDenied") {
    console.log("DENIED_OK");
    Deno.exit(0);
  }
  console.error("FAIL unexpected error " + e.name + ": " + e.message);
  Deno.exit(4);
}
"#;

fn deno_available() -> bool {
    Command::new("deno")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_deno(
    extra_args: &[String],
    script: &Path,
    ok: &Path,
    secret: &Path,
) -> Output {
    Command::new("deno")
        .arg("run")
        .args(extra_args)
        .arg(script)
        .arg(ok)
        .arg(secret)
        .stdin(Stdio::null())
        .output()
        .expect("failed to spawn deno")
}

#[test]
fn deno_enforces_filesystem_read_confinement() {
    if !deno_available() {
        eprintln!("skipping: `deno` not found on PATH");
        return;
    }

    // ── temp workspace layout ────────────────────────────────────────────────
    let tmp = tempfile::tempdir().expect("temp dir");
    // Canonicalize so symlinked temp roots (e.g. macOS /var → /private/var)
    // match the paths Deno reports.
    let base = fs::canonicalize(tmp.path()).expect("canonicalize base");

    let allowed_dir = base.join("allowed");
    fs::create_dir_all(&allowed_dir).unwrap();
    let ok_file = allowed_dir.join("ok.txt");
    fs::write(&ok_file, "OK").unwrap();

    let secret_file = base.join("secret.txt"); // outside the allowed subtree
    fs::write(&secret_file, "TOPSECRET").unwrap();

    let script = base.join("probe.js");
    fs::write(&script, PROBE_SCRIPT).unwrap();

    // ── policy → RequiredCapabilities → Deno flags ───────────────────────────
    // Allow reading only within the allowed dir (mapped to @workspace).
    let cfg: CapabilityRules = serde_json::from_str(
        r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/**"] }]"#,
    )
    .unwrap();
    let req = project(&cfg, &());
    let roots = PathRoots::new().with(Root::Workspace, &allowed_dir);

    let backends: [&dyn EnforcementBackend; 1] = [&DenoFlags];
    let plan = build_plan(&req, &roots, &backends).expect("fully covered");

    // Sanity: the generated flag confines reads to the allowed dir and never
    // opens the barn door.
    let expected = format!(
        "--allow-read={}",
        allowed_dir.to_string_lossy().replace('\\', "/")
    );
    assert!(
        plan.spawn.args.contains(&expected),
        "expected {expected:?} in {:?}",
        plan.spawn.args
    );
    assert!(!plan.spawn.args.iter().any(|a| a == "--allow-all"));

    // ── sandboxed run: denied read must be blocked ───────────────────────────
    let sandboxed = run_deno(&plan.spawn.args, &script, &ok_file, &secret_file);
    assert_eq!(
        sandboxed.status.code(),
        Some(0),
        "sandboxed run should read the allowed file and be denied the secret.\n\
         stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&sandboxed.stdout),
        String::from_utf8_lossy(&sandboxed.stderr),
    );
    assert!(
        String::from_utf8_lossy(&sandboxed.stdout).contains("DENIED_OK"),
        "stderr: {}",
        String::from_utf8_lossy(&sandboxed.stderr)
    );

    // ── control run: with --allow-all the same script CAN read the secret ─────
    // This proves the confinement above comes from our flags, not from some
    // unrelated reason the file was unreadable.
    let control = run_deno(
        &["--allow-all".to_string()],
        &script,
        &ok_file,
        &secret_file,
    );
    assert_eq!(
        control.status.code(),
        Some(3),
        "with --allow-all the secret must be readable (exit 3).\n\
         stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&control.stdout),
        String::from_utf8_lossy(&control.stderr),
    );
}
