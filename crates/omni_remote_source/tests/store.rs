//! Integration tests for the shared, content-addressed source store.
//!
//! These build throwaway local git repositories and materialize them through
//! `RemoteSourceManager`, so they need a `git` binary on PATH. They skip
//! (rather than fail) when it is missing, mirroring the network-gated suites.

use std::{path::Path, process::Command};

use omni_remote_source::{
    RemoteSource, RemoteSourceRef,
    manager::{
        RemoteSourceManager, StorePruneFilter, config::RemoteSourceConfig,
    },
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
    store_root: std::path::PathBuf,
}

impl Workspace {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("workspace tempdir");
        let sources_root = dir.path().join(".omni/sources");
        std::fs::create_dir_all(&sources_root).expect("create sources root");
        let store_root = sources_root.join("store");
        Workspace {
            _dir: dir,
            sources_root,
            store_root,
        }
    }

    /// A different workspace (own lockfile and refs) whose store root is
    /// `store_root`, as when two worktrees share the global store.
    fn with_store(store_root: &Path) -> Self {
        Workspace {
            store_root: store_root.to_path_buf(),
            ..Workspace::new()
        }
    }

    async fn manager(&self) -> RemoteSourceManager<RealSys> {
        RemoteSourceManager::new(
            RemoteSourceConfig::builder()
                .lockfile_path(self.sources_root.join("lock.json"))
                .store_root_path(self.store_root.clone())
                .build(),
            RealSys,
        )
        .await
        .expect("build manager")
    }
}

#[tokio::test]
async fn publish_is_idempotent_and_offline_after_locking() {
    if !omni_test_capabilities::gate(
        "git",
        git_available(),
        "git binary not found on PATH",
    ) {
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
    if !omni_test_capabilities::gate(
        "git",
        git_available(),
        "git binary not found on PATH",
    ) {
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
async fn reconcile_keeps_every_checkout_and_drops_unreferenced_pins() {
    if !omni_test_capabilities::gate(
        "git",
        git_available(),
        "git binary not found on PATH",
    ) {
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

    manager.reconcile_pins().await.expect("reconcile");

    // The store is a cache owned by no single workspace: nothing is deleted.
    assert!(RealSys.fs_exists_no_err_async(&mat_a.root).await);
    assert!(
        RealSys.fs_exists_no_err_async(&mat_b.root).await,
        "reconcile must never delete from the store"
    );

    // The unreferenced pin is gone from the lockfile; the referenced one stays.
    let reopened = ws.manager().await;
    assert_eq!(reopened.locked_commit(&url_a, "main").await, Some(commit_a));
    assert_eq!(reopened.locked_commit(&url_b, "main").await, None);
}

#[tokio::test]
async fn one_workspace_reconciling_never_deletes_another_workspaces_checkout() {
    if !omni_test_capabilities::gate(
        "git",
        git_available(),
        "git binary not found on PATH",
    ) {
        return;
    }

    let (_src, url, commit) = build_repo("shared\n");
    let shared_store = tempfile::tempdir().expect("shared store");
    let store_root = shared_store.path().join("sources");

    let a = Workspace::with_store(&store_root);
    let b = Workspace::with_store(&store_root);
    let source = RemoteSource::Git {
        uri: url.clone(),
        rev: "main".to_string(),
    };

    // B pins and references the checkout; A references nothing.
    let mgr_b = b.manager().await;
    let mat_b = mgr_b.materialize(&source).await.expect("materialize b");
    mgr_b
        .record_refs(
            "generator",
            &[RemoteSourceRef {
                source: source.clone(),
                pin: commit.clone(),
            }],
        )
        .await
        .expect("record refs");
    mgr_b.persist_pins().await.expect("persist pins");

    // `omni remote-sources install` in A reconciles against A's refs only.
    a.manager()
        .await
        .reconcile_pins()
        .await
        .expect("reconcile a");

    assert!(
        RealSys.fs_exists_no_err_async(&mat_b.root).await,
        "B's checkout in the shared store was deleted by A"
    );
}

#[tokio::test]
async fn concurrent_materialization_of_one_remote_publishes_once() {
    if !omni_test_capabilities::gate(
        "git",
        git_available(),
        "git binary not found on PATH",
    ) {
        return;
    }

    let (_src, url, commit) = build_repo("race\n");
    let shared_store = tempfile::tempdir().expect("shared store");
    let store_root = shared_store.path().join("sources");
    let source = RemoteSource::Git {
        uri: url,
        rev: "main".to_string(),
    };

    let mut tasks = Vec::new();
    for _ in 0..4 {
        let ws = Workspace::with_store(&store_root);
        let source = source.clone();
        tasks.push(tokio::spawn(async move {
            let manager = ws.manager().await;
            manager.materialize(&source).await.map(|m| (m, ws))
        }));
    }
    let mut roots = std::collections::HashSet::new();
    for task in tasks {
        let (mat, _ws) = task.await.expect("join").expect("materialize");
        assert_eq!(mat.pin, commit);
        roots.insert(mat.root);
    }
    assert_eq!(roots.len(), 1);

    let leftovers: Vec<_> = std::fs::read_dir(store_root.join(".pending"))
        .map(|d| d.flatten().collect())
        .unwrap_or_default();
    assert!(leftovers.is_empty(), "pending dirs left: {leftovers:?}");
}

fn age_marker(commit_dir: &Path, days: u64) {
    let mut marker = commit_dir.as_os_str().to_owned();
    marker.push(".used");
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&marker)
        .expect("a materialized checkout has a .used marker");
    file.set_modified(
        std::time::SystemTime::now()
            - std::time::Duration::from_secs(days * 86_400),
    )
    .expect("set mtime");
}

async fn two_checkouts(
    ws: &Workspace,
) -> (
    RemoteSourceManager<RealSys>,
    [(RemoteSource, std::path::PathBuf); 2],
    [TempDir; 2],
) {
    let (a, url_a, _) = build_repo("alpha alpha alpha\n");
    let (b, url_b, _) = build_repo("b\n");
    let manager = ws.manager().await;
    let mut out = Vec::new();
    for url in [url_a, url_b] {
        let source = RemoteSource::Git {
            uri: url,
            rev: "main".to_string(),
        };
        let mat = manager.materialize(&source).await.expect("materialize");
        out.push((source, mat.root));
    }
    let [first, second]: [_; 2] = out.try_into().unwrap();
    (manager, [first, second], [a, b])
}

#[tokio::test]
async fn store_entries_report_size_and_last_use() {
    if !omni_test_capabilities::gate(
        "git",
        git_available(),
        "git binary not found on PATH",
    ) {
        return;
    }
    let ws = Workspace::new();
    let (manager, [(_, dir_a), (_, dir_b)], _repos) = two_checkouts(&ws).await;

    let entries = manager.store_entries().await.expect("entries");
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().any(|e| e.dir == dir_a));
    assert!(entries.iter().any(|e| e.dir == dir_b));
    for entry in &entries {
        assert!(entry.size > 0, "{entry:?}");
        assert!(entry.last_used.elapsed().unwrap().as_secs() < 60);
    }
}

#[tokio::test]
async fn prune_needs_criteria_and_dry_run_deletes_nothing() {
    if !omni_test_capabilities::gate(
        "git",
        git_available(),
        "git binary not found on PATH",
    ) {
        return;
    }
    let ws = Workspace::new();
    let (manager, [(_, dir_a), (_, dir_b)], _repos) = two_checkouts(&ws).await;

    // No filter selects nothing, even for a real run.
    let none = manager
        .prune_store(&StorePruneFilter::default(), false)
        .await
        .expect("prune");
    assert!(none.is_empty());

    let all = StorePruneFilter {
        all: true,
        ..Default::default()
    };
    let would = manager.prune_store(&all, true).await.expect("dry run");
    assert_eq!(would.len(), 2);
    assert!(
        dir_a.exists() && dir_b.exists(),
        "dry run deleted something"
    );
}

#[tokio::test]
async fn prune_by_age_removes_only_old_checkouts_and_they_refetch() {
    if !omni_test_capabilities::gate(
        "git",
        git_available(),
        "git binary not found on PATH",
    ) {
        return;
    }
    let ws = Workspace::new();
    let (manager, [(source_a, dir_a), (_, dir_b)], _repos) =
        two_checkouts(&ws).await;
    age_marker(&dir_a, 30);

    let filter = StorePruneFilter {
        older_than: Some(std::time::Duration::from_secs(7 * 86_400)),
        ..Default::default()
    };
    let removed = manager.prune_store(&filter, false).await.expect("prune");

    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].dir, dir_a);
    assert!(!dir_a.exists(), "the old checkout is gone");
    assert!(
        !dir_a.parent().unwrap().exists(),
        "an emptied remote directory is removed"
    );
    assert!(dir_b.exists(), "the recently used checkout stays");

    // The store is a cache: the next use refetches the pinned commit.
    let again = manager.materialize(&source_a).await.expect("refetch");
    assert_eq!(again.root, dir_a);
    assert!(dir_a.exists());
}

#[tokio::test]
async fn prune_by_size_and_all() {
    if !omni_test_capabilities::gate(
        "git",
        git_available(),
        "git binary not found on PATH",
    ) {
        return;
    }
    let ws = Workspace::new();
    let (manager, [(_, dir_a), (_, dir_b)], _repos) = two_checkouts(&ws).await;
    let sizes: Vec<u64> = manager
        .store_entries()
        .await
        .unwrap()
        .iter()
        .map(|e| e.size)
        .collect();
    let (min, max) =
        (*sizes.iter().min().unwrap(), *sizes.iter().max().unwrap());
    assert!(min < max, "fixtures must differ in size: {sizes:?}");

    // Strictly larger than the smaller checkout: only the bigger one goes.
    let removed = manager
        .prune_store(
            &StorePruneFilter {
                larger_than: Some(min),
                ..Default::default()
            },
            false,
        )
        .await
        .unwrap();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].size, max);

    let rest = manager
        .prune_store(
            &StorePruneFilter {
                all: true,
                ..Default::default()
            },
            false,
        )
        .await
        .unwrap();
    assert_eq!(rest.len(), 1);
    assert!(!dir_a.exists() && !dir_b.exists());
    assert!(manager.store_entries().await.unwrap().is_empty());
}

#[tokio::test]
async fn concurrent_prune_and_materialize_both_succeed_and_leave_a_usable_store()
 {
    if !omni_test_capabilities::gate(
        "git",
        git_available(),
        "git binary not found on PATH",
    ) {
        return;
    }
    let (_src, url, commit) = build_repo("race\n");
    let shared = tempfile::tempdir().expect("shared store");
    let store_root = shared.path().join("sources");
    let source = RemoteSource::Git {
        uri: url,
        rev: "main".to_string(),
    };
    let all = StorePruneFilter {
        all: true,
        ..Default::default()
    };

    for _ in 0..5 {
        let installer = Workspace::with_store(&store_root);
        let pruner = Workspace::with_store(&store_root);
        let source = source.clone();
        let install = tokio::spawn(async move {
            installer
                .manager()
                .await
                .materialize(&source)
                .await
                .map(|_| ())
        });
        let prune = tokio::spawn(async move {
            pruner
                .manager()
                .await
                .prune_store(&all, false)
                .await
                .map(|_| ())
        });
        install.await.expect("join").expect("materialize");
        prune.await.expect("join").expect("prune");
    }

    // Whatever the interleaving, the next use yields a complete checkout.
    let ws = Workspace::with_store(&store_root);
    let mat = ws
        .manager()
        .await
        .materialize(&source)
        .await
        .expect("final");
    assert_eq!(mat.pin, commit);
    assert!(mat.root.join("README.md").exists());
}

fn source_of(url: &Url, rev: &str) -> RemoteSource {
    RemoteSource::Git {
        uri: url.clone(),
        rev: rev.to_string(),
    }
}

#[tokio::test]
async fn a_published_tree_is_plain_sealed_and_recorded_in_the_layout() {
    if !omni_test_capabilities::gate(
        "git",
        git_available(),
        "git binary not found on PATH",
    ) {
        return;
    }
    let (_src, url, _) = build_repo("sealed\n");
    let ws = Workspace::new();
    let mat = ws
        .manager()
        .await
        .materialize(&source_of(&url, "main"))
        .await
        .expect("materialize");

    assert!(
        !mat.root.join(".git").exists(),
        "a tree is not a repository"
    );
    assert!(mat.root.starts_with(ws.store_root.join("trees")));
    assert_eq!(
        std::fs::read_to_string(ws.store_root.join("VERSION"))
            .unwrap()
            .trim(),
        "1"
    );
    let readme = std::fs::metadata(mat.root.join("README.md")).unwrap();
    assert!(
        readme.permissions().readonly(),
        "published files are read-only"
    );
    // One bare mirror per remote.
    let mirrors: Vec<_> = std::fs::read_dir(ws.store_root.join("git"))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(mirrors.len(), 1);
    assert!(mirrors[0].file_name().to_string_lossy().ends_with(".git"));
}

#[tokio::test]
async fn deleting_only_the_trees_re_exports_offline_from_the_mirror() {
    if !omni_test_capabilities::gate(
        "git",
        git_available(),
        "git binary not found on PATH",
    ) {
        return;
    }
    let (src, url, commit) = build_repo("offline\n");
    let ws = Workspace::new();
    let manager = ws.manager().await;
    let source = source_of(&url, "main");
    let first = manager.materialize(&source).await.expect("materialize");
    manager.persist_pins().await.unwrap();

    // The remote is gone and the trees are deleted; the mirror remains.
    drop(src);
    std::fs::remove_dir_all(ws.store_root.join("trees")).unwrap();

    let again = ws
        .manager()
        .await
        .materialize(&source)
        .await
        .expect("re-export without the network");
    assert_eq!(again.pin, commit);
    assert_eq!(again.root, first.root);
    assert_eq!(
        std::fs::read_to_string(again.root.join("README.md")).unwrap(),
        "offline\n"
    );
}

#[tokio::test]
async fn with_the_mirror_gone_too_it_names_the_remote_when_offline() {
    if !omni_test_capabilities::gate(
        "git",
        git_available(),
        "git binary not found on PATH",
    ) {
        return;
    }
    let (src, url, _) = build_repo("gone\n");
    let ws = Workspace::new();
    let manager = ws.manager().await;
    let source = source_of(&url, "main");
    manager.materialize(&source).await.expect("materialize");
    manager.persist_pins().await.unwrap();

    drop(src);
    std::fs::remove_dir_all(&ws.store_root).unwrap();

    let error = ws
        .manager()
        .await
        .materialize(&source)
        .await
        .expect_err("cannot refetch");
    let message = format!("{error:#}");
    assert!(message.contains("could not fetch"), "{message}");
    assert!(
        message.contains(url.as_str()),
        "names the remote: {message}"
    );
    assert!(message.contains("network"), "{message}");
}

#[tokio::test]
async fn a_partially_deleted_tree_is_re_exported_not_reused() {
    if !omni_test_capabilities::gate(
        "git",
        git_available(),
        "git binary not found on PATH",
    ) {
        return;
    }
    let (src, url, _) = build_repo("whole\n");
    std::fs::create_dir_all(src.path().join("sub")).unwrap();
    std::fs::write(src.path().join("sub/inner.txt"), "inner").unwrap();
    git(src.path(), &["add", "."]);
    git(src.path(), &["commit", "-q", "-m", "second file"]);
    let ws = Workspace::new();
    let manager = ws.manager().await;
    let source = source_of(&url, "main");
    let mat = manager.materialize(&source).await.expect("materialize");
    manager.persist_pins().await.unwrap();

    // A cache cleaner removed one file; the directory itself is still there.
    std::fs::remove_file(mat.root.join("sub/inner.txt")).unwrap();

    let again = ws
        .manager()
        .await
        .materialize(&source)
        .await
        .expect("materialize");
    assert_eq!(again.root, mat.root);
    assert_eq!(
        std::fs::read_to_string(again.root.join("sub/inner.txt")).unwrap(),
        "inner",
        "the missing file was restored"
    );
}

#[tokio::test]
async fn commits_of_one_remote_share_one_mirror_with_a_tree_each() {
    if !omni_test_capabilities::gate(
        "git",
        git_available(),
        "git binary not found on PATH",
    ) {
        return;
    }
    let (src, url, c1) = build_repo("one\n");
    let ws = Workspace::new();
    let manager = ws.manager().await;
    let first = manager
        .materialize(&source_of(&url, &c1))
        .await
        .expect("c1");

    std::fs::write(src.path().join("two.txt"), "two").unwrap();
    git(src.path(), &["add", "."]);
    git(src.path(), &["commit", "-q", "-m", "two"]);
    let second = manager
        .materialize(&source_of(&url, "main"))
        .await
        .expect("c2 by branch");

    assert_ne!(first.pin, second.pin);
    assert!(first.root.exists() && second.root.exists());
    assert!(!first.root.join("two.txt").exists());
    assert!(second.root.join("two.txt").exists());
    let mirrors = std::fs::read_dir(ws.store_root.join("git"))
        .unwrap()
        .count();
    assert_eq!(mirrors, 1, "history is stored once, not per commit");
}

#[tokio::test]
async fn an_unknown_revision_is_an_error_naming_it() {
    if !omni_test_capabilities::gate(
        "git",
        git_available(),
        "git binary not found on PATH",
    ) {
        return;
    }
    let (_src, url, _) = build_repo("x\n");
    let ws = Workspace::new();
    let error = ws
        .manager()
        .await
        .materialize(&source_of(&url, "no-such-branch"))
        .await
        .expect_err("unknown rev");
    assert!(format!("{error:#}").contains("no-such-branch"), "{error:#}");
}

#[tokio::test]
async fn a_store_from_a_newer_layout_is_refused_not_modified() {
    if !omni_test_capabilities::gate(
        "git",
        git_available(),
        "git binary not found on PATH",
    ) {
        return;
    }
    let (_src, url, _) = build_repo("x\n");
    let ws = Workspace::new();
    std::fs::create_dir_all(&ws.store_root).unwrap();
    std::fs::write(ws.store_root.join("VERSION"), "99\n").unwrap();

    let error = ws
        .manager()
        .await
        .materialize(&source_of(&url, "main"))
        .await
        .expect_err("newer layout");
    let message = format!("{error:#}");
    assert!(message.contains("layout 99"), "{message}");
    assert!(!ws.store_root.join("trees").exists(), "nothing was written");
}

#[tokio::test]
async fn a_tracked_dot_manifest_file_is_exported_untouched() {
    if !omni_test_capabilities::gate(
        "git",
        git_available(),
        "git binary not found on PATH",
    ) {
        return;
    }
    let (src, url, _) = build_repo("x\n");
    std::fs::write(src.path().join(".manifest"), "tracked by the repo")
        .unwrap();
    git(src.path(), &["add", "."]);
    git(src.path(), &["commit", "-q", "-m", "tracked .manifest"]);

    let ws = Workspace::new();
    let mat = ws
        .manager()
        .await
        .materialize(&source_of(&url, "main"))
        .await
        .expect("materialize");

    assert_eq!(
        std::fs::read_to_string(mat.root.join(".manifest")).unwrap(),
        "tracked by the repo"
    );
    // The store's own manifest sits beside the tree, outside it.
    let mut beside = mat.root.as_os_str().to_owned();
    beside.push(".manifest");
    assert!(std::path::Path::new(&beside).is_file());
}
