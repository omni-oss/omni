//! The bare mirror and tree export the shared remote-source store is built on
//! (RFC 0027): fetch every branch and tag once, resolve revs offline, export any
//! commit as a plain directory.
//!
//! Local `git` repositories are the fixtures, so these need a `git` binary and
//! skip (rather than fail) without one, like the other git-backed suites.

use std::{path::Path, process::Command};

use omni_git_utils::{export_tree, fetch_bare, resolve_commit};
use system_traits::impls::RealSys;

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
        .unwrap()
        .trim_end()
        .to_string()
}

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn commit(dir: &Path, file: &str, body: &str) -> String {
    std::fs::write(dir.join(file), body).unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", file]);
    git(dir, &["rev-parse", "HEAD"])
}

/// A repo with commits c1, c2 on main, an annotated tag v1 on c2 and a feature
/// branch holding c3. Returns `(dir, url, [c1, c2, c3])`.
fn fixture() -> (tempfile::TempDir, String, [String; 3]) {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    git(p, &["init", "-q", "-b", "main"]);
    let c1 = commit(p, "a.txt", "one");
    let c2 = commit(p, "b.txt", "two");
    git(p, &["tag", "-a", "v1", "-m", "release"]);
    git(p, &["checkout", "-q", "-b", "feature"]);
    let c3 = commit(p, "c.txt", "three");
    git(p, &["checkout", "-q", "main"]);
    let url = format!("file://{}", p.display());
    (dir, url, [c1, c2, c3])
}

#[tokio::test]
async fn fetch_brings_every_branch_and_tag_and_resolves_offline() {
    if !omni_test_capabilities::gate(
        "git",
        git_available(),
        "git binary not found on PATH",
    ) {
        return;
    }
    let (src, url, [c1, c2, c3]) = fixture();
    let store = tempfile::tempdir().unwrap();
    let bare = store.path().join("git/remote.git");

    fetch_bare(&RealSys, &url, &bare).await.expect("clone");
    // Remove the remote: everything below must work from the mirror alone.
    drop(src);

    assert_eq!(resolve_commit(&bare, "main"), Some(c2.clone()));
    assert_eq!(
        resolve_commit(&bare, "feature"),
        Some(c3),
        "a non-default branch is mirrored"
    );
    assert_eq!(
        resolve_commit(&bare, "v1"),
        Some(c2.clone()),
        "an annotated tag peels to its commit"
    );
    assert_eq!(resolve_commit(&bare, &c1), Some(c1.clone()));
    assert_eq!(resolve_commit(&bare, "no-such-ref"), None);
    assert_eq!(
        resolve_commit(&store.path().join("absent.git"), "main"),
        None
    );
}

#[tokio::test]
async fn a_second_fetch_is_incremental_and_picks_up_new_refs() {
    if !omni_test_capabilities::gate(
        "git",
        git_available(),
        "git binary not found on PATH",
    ) {
        return;
    }
    let (src, url, _) = fixture();
    let store = tempfile::tempdir().unwrap();
    let bare = store.path().join("remote.git");
    fetch_bare(&RealSys, &url, &bare).await.expect("clone");

    let c4 = commit(src.path(), "d.txt", "four");
    git(src.path(), &["tag", "v2"]);
    git(src.path(), &["branch", "later"]);
    assert_eq!(resolve_commit(&bare, &c4), None, "not fetched yet");

    fetch_bare(&RealSys, &url, &bare)
        .await
        .expect("incremental fetch");
    assert_eq!(resolve_commit(&bare, "main"), Some(c4.clone()));
    assert_eq!(resolve_commit(&bare, "v2"), Some(c4.clone()));
    assert_eq!(resolve_commit(&bare, "later"), Some(c4));
}

#[tokio::test]
async fn a_failed_first_clone_leaves_nothing_behind() {
    let store = tempfile::tempdir().unwrap();
    let bare = store.path().join("git/remote.git");

    let result =
        fetch_bare(&RealSys, "file:///definitely/not/a/repo", &bare).await;

    assert!(result.is_err());
    assert!(!bare.exists());
    let leftovers: Vec<_> = std::fs::read_dir(store.path().join("git"))
        .map(|d| d.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(leftovers.is_empty(), "staging left behind: {leftovers:?}");
}

#[tokio::test]
async fn export_writes_a_plain_tree_of_the_requested_commit() {
    if !omni_test_capabilities::gate(
        "git",
        git_available(),
        "git binary not found on PATH",
    ) {
        return;
    }
    let (_src, url, [c1, c2, c3]) = fixture();
    let store = tempfile::tempdir().unwrap();
    let bare = store.path().join("remote.git");
    fetch_bare(&RealSys, &url, &bare).await.expect("clone");

    let list = |dir: &Path| {
        let mut names: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };

    let first = store.path().join("trees/c1");
    export_tree(&RealSys, &bare, &c1, &first)
        .await
        .expect("export c1");
    assert_eq!(list(&first), ["a.txt"]);

    let second = store.path().join("trees/c2");
    export_tree(&RealSys, &bare, &c2, &second)
        .await
        .expect("export c2");
    assert_eq!(list(&second), ["a.txt", "b.txt"]);
    assert_eq!(
        std::fs::read_to_string(second.join("b.txt")).unwrap(),
        "two"
    );

    // A commit only the feature branch reaches exports too.
    let third = store.path().join("trees/c3");
    export_tree(&RealSys, &bare, &c3, &third)
        .await
        .expect("export c3");
    assert_eq!(list(&third), ["a.txt", "b.txt", "c.txt"]);
    assert!(
        !third.join(".git").exists(),
        "an export is not a repository"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn export_keeps_executable_bits_and_symlinks() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    if !omni_test_capabilities::gate(
        "git",
        git_available(),
        "git binary not found on PATH",
    ) {
        return;
    }
    let src = tempfile::tempdir().unwrap();
    let p = src.path();
    git(p, &["init", "-q", "-b", "main"]);
    std::fs::write(p.join("run.sh"), "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(
        p.join("run.sh"),
        PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    symlink("run.sh", p.join("link")).unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-q", "-m", "x"]);
    let head = git(p, &["rev-parse", "HEAD"]);

    let store = tempfile::tempdir().unwrap();
    let bare = store.path().join("remote.git");
    fetch_bare(&RealSys, &format!("file://{}", p.display()), &bare)
        .await
        .unwrap();
    let out = store.path().join("tree");
    export_tree(&RealSys, &bare, &head, &out).await.unwrap();

    let mode = std::fs::metadata(out.join("run.sh"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o111, 0o111, "executable bits survive");
    assert!(
        std::fs::symlink_metadata(out.join("link"))
            .unwrap()
            .is_symlink()
    );
}
