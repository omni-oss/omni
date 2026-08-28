//! Regression test for cloning at an *annotated* tag.
//!
//! `rev_parse_single` resolves an annotated tag to the tag object's OID (not
//! the commit it points to). `clone_repo` used to hand that OID straight to
//! `find_commit`, which failed with:
//!
//! > Object named <sha> was supposed to be of kind commit, but was kind tag.
//!
//! The fix peels through any tags to the underlying commit. This test builds a
//! throwaway local repo with an annotated tag and asserts the clone succeeds
//! and reports the *commit* hash (not the tag object hash).

use std::{path::Path, process::Command};

use omni_git_utils::clone_repo;
use system_traits::impls::RealSys;

/// Run `git` in `cwd` with an isolated config (no user/system config, no GPG
/// signing) so the test is deterministic regardless of the host environment.
/// Returns stdout with the trailing newline trimmed.
fn git(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", "omni test")
        .env("GIT_AUTHOR_EMAIL", "omni@example.com")
        .env("GIT_COMMITTER_NAME", "omni test")
        .env("GIT_COMMITTER_EMAIL", "omni@example.com")
        .output()
        .expect("failed to spawn git");

    assert!(
        output.status.success(),
        "git {args:?} failed:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );

    String::from_utf8(output.stdout)
        .expect("git output was not utf-8")
        .trim_end()
        .to_string()
}

/// True when a usable `git` binary is on PATH. The test skips (rather than
/// fails) when it isn't, mirroring the network-gated e2e suites.
fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[tokio::test]
async fn clone_at_annotated_tag_resolves_to_the_underlying_commit() {
    if !git_available() {
        eprintln!("skipping: `git` is not available on PATH");
        return;
    }

    let src = tempfile::tempdir().expect("source tempdir");
    let src_path = src.path();

    // Build a minimal repo: one commit on `main`, then an annotated tag whose
    // object OID differs from the commit OID.
    git(src_path, &["init", "-q", "-b", "main"]);
    std::fs::write(src_path.join("README.md"), b"hello\n")
        .expect("write README");
    git(src_path, &["add", "."]);
    git(src_path, &["commit", "-q", "-m", "initial commit"]);
    git(src_path, &["tag", "-a", "v1.0.0", "-m", "release 1.0.0"]);

    let commit_sha = git(src_path, &["rev-parse", "HEAD"]);
    let tag_object_sha = git(src_path, &["rev-parse", "v1.0.0"]);

    // Sanity check: this is genuinely an annotated tag, so the tag object and
    // the commit have distinct OIDs. (A lightweight tag would make them equal
    // and the test would not exercise the peel path.)
    assert_ne!(
        tag_object_sha, commit_sha,
        "expected an annotated tag whose OID differs from the commit",
    );

    let dest = tempfile::tempdir().expect("dest tempdir");
    // Clone into a not-yet-existing subdirectory (matches real callers).
    let dest_path = dest.path().join("clone");

    let info = clone_repo(
        &RealSys,
        src_path.to_str().expect("utf-8 source path"),
        Some("v1.0.0"),
        &dest_path,
    )
    .await
    .expect("cloning at an annotated tag should succeed");

    // The reported commit must be the peeled commit, never the tag object.
    assert_eq!(
        info.commit, commit_sha,
        "clone should resolve the annotated tag to its underlying commit",
    );
    assert_ne!(
        info.commit, tag_object_sha,
        "clone must not report the annotated tag's object hash",
    );

    // The tagged content should be checked out into the worktree.
    assert_eq!(
        std::fs::read_to_string(dest_path.join("README.md"))
            .expect("checked-out README"),
        "hello\n",
    );
}
