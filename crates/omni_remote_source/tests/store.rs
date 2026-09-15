//! Integration tests for the shared, content-addressed source store.
//!
//! These build throwaway local git repositories and materialize them through
//! `RemoteSourceManager`, so they need a `git` binary on PATH. They skip
//! (rather than fail) when it is missing, mirroring the network-gated suites.

use std::{path::Path, process::Command};

use omni_remote_sources::{
    RemoteSource, RemoteSourceRef,
    manager::{RemoteSourceManager, config::RemoteSourceConfig},
};
use system_traits::{FsMetadataAsync, impls::RealSys};
use tempfile::TempDir;
use url::Url;

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

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Build a one-commit repo and return `(tempdir, file:// url, commit sha)`.
fn build_repo(content: &str) -> (TempDir, Url, String) {
    let src = tempfile::tempdir().expect("source tempdir");
    let path = src.path();

    git(path, &["init", "-q", "-b", "main"]);
    std::fs::write(path.join("README.md"), content).expect("write README");
    git(path, &["add", "."]);
    git(path, &["commit", "-q", "-m", "initial commit"]);

    let commit = git(path, &["rev-parse", "HEAD"]);
    let url = Url::from_directory_path(path).expect("file url");

    (src, url, commit)
}

struct Workspace {
    _dir: TempDir,
    sources_root: std::path::PathBuf,
}

impl Workspace {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("workspace tempdir");
        let sources_root = dir.path().join(".omni/sources");
        std::fs::create_dir_all(&sources_root).expect("create sources root");
        Workspace {
            _dir: dir,
            sources_root,
        }
    }

    async fn manager(&self) -> RemoteSourceManager<RealSys> {
        RemoteSourceManager::new(
            RemoteSourceConfig::builder()
                .lockfile_path(self.sources_root.join("lock.json"))
                .store_root_path(self.sources_root.join("store"))
                .build(),
            RealSys,
        )
        .await
        .expect("build manager")
    }
}

#[tokio::test]
async fn publish_is_idempotent_and_offline_after_locking() {
    if !git_available() {
        eprintln!("skipping: `git` is not available on PATH");
        return;
    }

    let (_src, url, commit) = build_repo("hello\n");
    let ws = Workspace::new();
    let manager = ws.manager().await;

    let source = RemoteSource::Git {
        uri: url.clone(),
        rev: "main".to_string(),
    };

    let first = manager.materialize(&source).await.expect("materialize");
    assert_eq!(first.pin, commit);
    assert!(RealSys.fs_exists_no_err_async(&first.root).await);

    // Second materialization returns the same directory without re-cloning.
    let second = manager.materialize(&source).await.expect("materialize");
    assert_eq!(first, second);

    // A fresh manager over the committed lockfile resolves offline from the
    // checkout already present in the store.
    manager.persist_pins().await.expect("persist pins");
    let reopened = ws.manager().await;
    let offline = reopened.materialize(&source).await.expect("offline");
    assert_eq!(offline.pin, commit);
    assert_eq!(offline.root, first.root);
}

#[tokio::test]
async fn revs_resolving_to_the_same_commit_share_one_checkout() {
    if !git_available() {
        eprintln!("skipping: `git` is not available on PATH");
        return;
    }

    let (src, url, commit) = build_repo("hello\n");
    // A second ref (a tag) that points at the same commit.
    git(src.path(), &["tag", "v1"]);

    let ws = Workspace::new();
    let manager = ws.manager().await;

    let by_branch = manager
        .materialize(&RemoteSource::Git {
            uri: url.clone(),
            rev: "main".to_string(),
        })
        .await
        .expect("materialize branch");
    let by_tag = manager
        .materialize(&RemoteSource::Git {
            uri: url.clone(),
            rev: "v1".to_string(),
        })
        .await
        .expect("materialize tag");

    assert_eq!(by_branch.pin, commit);
    assert_eq!(by_branch.root, by_tag.root);
}

#[tokio::test]
async fn retain_keeps_cross_subsystem_refs_and_removes_orphans() {
    if !git_available() {
        eprintln!("skipping: `git` is not available on PATH");
        return;
    }

    let (_a, url_a, commit_a) = build_repo("a\n");
    let (_b, url_b, _commit_b) = build_repo("b\n");

    let ws = Workspace::new();
    let manager = ws.manager().await;

    let source_a = RemoteSource::Git {
        uri: url_a.clone(),
        rev: "main".to_string(),
    };
    let source_b = RemoteSource::Git {
        uri: url_b.clone(),
        rev: "main".to_string(),
    };

    let mat_a = manager.materialize(&source_a).await.expect("materialize a");
    let mat_b = manager.materialize(&source_b).await.expect("materialize b");

    // Only the "generator" subsystem references A; nobody references B.
    manager
        .record_refs(
            "generator",
            &[RemoteSourceRef {
                source: source_a.clone(),
                pin: commit_a.clone(),
            }],
        )
        .await
        .expect("record refs");

    manager.retain().await.expect("retain");

    assert!(
        RealSys.fs_exists_no_err_async(&mat_a.root).await,
        "referenced checkout must survive"
    );
    assert!(
        !RealSys.fs_exists_no_err_async(&mat_b.root).await,
        "unreferenced checkout must be removed"
    );

    // The pruned pin is gone from the lockfile; the retained one is present.
    let reopened = ws.manager().await;
    assert_eq!(reopened.locked_commit(&url_a, "main").await, Some(commit_a));
    assert_eq!(reopened.locked_commit(&url_b, "main").await, None);
}
