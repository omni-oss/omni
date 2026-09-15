//! Contributor integration tests. They build throwaway local git repositories
//! and materialize them through a real store, so they need a `git` binary on
//! PATH and skip when it is missing.

use std::{path::Path, process::Command};

use omni_configurations::{
    GitSource, LocalSource, SourceConfig, types::SingleOrMany,
};
use omni_projection_configurations::ProjectionExtra;
use omni_remote_source_contributors::{
    GeneratorRemoteContributor, ProjectionRemoteContributor,
};
use omni_remote_source::{
    InstallOptions, RemoteSourceContributor,
    manager::{RemoteSourceManager, config::RemoteSourceConfig},
};
use system_traits::impls::RealSys;
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

/// Build a one-commit repo with the given files and return `(dir, url, commit)`.
fn build_repo(files: &[(&str, &str)]) -> (TempDir, Url, String) {
    let src = tempfile::tempdir().expect("source tempdir");
    let path = src.path();

    git(path, &["init", "-q", "-b", "main"]);
    for (name, content) in files {
        std::fs::write(path.join(name), content).expect("write file");
    }
    git(path, &["add", "."]);
    git(path, &["commit", "-q", "-m", "initial commit"]);

    let commit = git(path, &["rev-parse", "HEAD"]);
    let url = Url::from_directory_path(path).expect("file url");

    (src, url, commit)
}

fn workspace() -> (TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("workspace tempdir");
    let sources_root = dir.path().join(".omni/sources");
    std::fs::create_dir_all(&sources_root).expect("create sources root");
    (dir, sources_root)
}

async fn manager(sources_root: &Path) -> RemoteSourceManager<RealSys> {
    RemoteSourceManager::new(
        RemoteSourceConfig::builder()
            .lockfile_path(sources_root.join("lock.json"))
            .store_root_path(sources_root.join("store"))
            .build(),
        RealSys,
    )
    .await
    .expect("build manager")
}

fn git_source(uri: &Url) -> SourceConfig {
    SourceConfig::Git(GitSource {
        uri: uri.clone(),
        rev: "main".to_string(),
        extra: Default::default(),
    })
}

#[tokio::test]
async fn generator_contributor_materializes_git_sources() {
    if !git_available() {
        eprintln!("skipping: `git` is not available on PATH");
        return;
    }

    let (_src, url, commit) = build_repo(&[("gen.omni.yaml", "name: g\n")]);
    let (_ws, sources_root) = workspace();
    let manager = manager(&sources_root).await;

    let contributor = GeneratorRemoteContributor::new(vec![git_source(&url)]);

    let refs = contributor
        .contribute(&manager, &InstallOptions::default())
        .await
        .expect("contribute");

    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].pin, commit);
    assert_eq!(manager.locked_commit(&url, "main").await, Some(commit));
}

#[tokio::test]
async fn projection_contributor_recurses_meta_bundles_to_git_children() {
    if !git_available() {
        eprintln!("skipping: `git` is not available on PATH");
        return;
    }

    // A git child that the bundle references.
    let (_child, child_url, child_commit) = build_repo(&[(
        "projection.omni.yaml",
        "routes:\n  - strategy: mirror\n",
    )]);

    // A local bundle inside the workspace whose manifest is a meta bundle
    // pointing at the git child.
    let (ws, sources_root) = workspace();
    let bundle_dir = ws.path().join("bundle");
    std::fs::create_dir_all(&bundle_dir).expect("bundle dir");
    let manifest = format!(
        "sources:\n  - source: git\n    uri: {}\n    rev: main\n    id: child\n",
        child_url
    );
    std::fs::write(bundle_dir.join("projection.omni.yaml"), manifest)
        .expect("write bundle manifest");

    let manager = manager(&sources_root).await;

    let top = SourceConfig::Local(LocalSource {
        path: SingleOrMany::Single("./bundle".to_string()),
        extra: ProjectionExtra {
            id: "b".to_string(),
            routes: None,
        },
    });

    let contributor =
        ProjectionRemoteContributor::new(vec![top], ws.path().to_path_buf());

    let refs = contributor
        .contribute(&manager, &InstallOptions::default())
        .await
        .expect("contribute");

    assert_eq!(refs.len(), 1, "the git child must be materialized");
    assert_eq!(refs[0].pin, child_commit);
    assert_eq!(
        manager.locked_commit(&child_url, "main").await,
        Some(child_commit)
    );
}
