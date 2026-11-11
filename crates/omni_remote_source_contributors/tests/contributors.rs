//! Contributor integration tests. They build throwaway local git repositories
//! and materialize them through a real store, so they need a `git` binary on
//! PATH and skip when it is missing.

use std::{path::Path, process::Command};

use omni_configurations::{
    GitSource, LocalSource, PackSourceConfiguration, ProjectionId,
    ProjectionRoutes, SourceConfig, types::SingleOrMany,
};
use omni_remote_source::{
    InstallOptions, RemoteSourceContributor,
    manager::{RemoteSourceManager, config::RemoteSourceConfig},
};
use omni_remote_source_contributors::{
    GeneratorRemoteContributor, ProjectionRemoteContributor, expand_packs,
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
        let file = path.join(name);
        std::fs::create_dir_all(file.parent().expect("file has a parent"))
            .expect("create parent dirs");
        std::fs::write(file, content).expect("write file");
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
        base: (),
        extra: (),
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
        "name: \"@vendor/child\"\nroutes:\n  - strategy: mirror\n",
    )]);

    // A local bundle inside the workspace whose manifest is a meta bundle
    // pointing at the git child.
    let (ws, sources_root) = workspace();
    let bundle_dir = ws.path().join("bundle");
    std::fs::create_dir_all(&bundle_dir).expect("bundle dir");
    let manifest = format!(
        "name: \"@vendor/bundle\"\nsources:\n  - source: git\n    uri: {}\n    rev: main\n    id: child\n",
        child_url
    );
    std::fs::write(bundle_dir.join("projection.omni.yaml"), manifest)
        .expect("write bundle manifest");

    let manager = manager(&sources_root).await;

    let top = SourceConfig::Local(LocalSource {
        path: SingleOrMany::Single("./bundle".to_string()),
        base: ProjectionRoutes { routes: None },
        extra: ProjectionId {
            id: "b".to_string(),
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

/// Everything a git pack contributes lives in the store checkout, including its
/// local children and the paths it declares as `local`, so none of it may see
/// the workspace's ignore rules.
#[tokio::test]
async fn a_git_pack_and_everything_nested_in_it_is_not_local() {
    if !git_available() {
        eprintln!("skipping: `git` is not available on PATH");
        return;
    }

    let (_src, url, _commit) = build_repo(&[
        (
            "pack.omni.yaml",
            "name: \"@org/kit\"\nprojections:\n  - source: local\n    path: ./skills\n    id: skills\ngenerators:\n  - source: local\n    path: ./gens\npacks:\n  - source: local\n    path: ./inner\n    id: inner\n",
        ),
        (
            "inner/pack.omni.yaml",
            "name: \"@org/inner\"\ntools:\n  - source: local\n    path: ./tools\n",
        ),
    ]);
    let (ws, sources_root) = workspace();
    let manager = manager(&sources_root).await;

    let pack: PackSourceConfiguration = serde_json::from_str(&format!(
        r#"{{"source":"git","uri":"{url}","rev":"main","id":"kit"}}"#
    ))
    .expect("valid pack source");
    let (packs, _refs) =
        expand_packs(&manager, &[pack], ws.path(), false, None)
            .await
            .expect("expand");

    assert_eq!(packs.nodes.len(), 2);
    assert!(
        packs.nodes.iter().all(|n| !n.is_local),
        "the git pack and its local child are both non-local"
    );
    let generators = packs.effective_generator_sources();
    let tools = packs.effective_tool_sources();
    assert_eq!(generators.len(), 1);
    assert_eq!(tools.len(), 1);
    assert!(!generators[0].is_local);
    assert!(!tools[0].is_local);
    assert_eq!(
        packs.non_local_projection_ids(),
        std::collections::HashSet::from(["kit/skills".to_string()]),
        "the pack's projection is matched by its qualified id"
    );
}

/// A git pack and a workspace-local pack can reach the same directory: the
/// local one is configured to point at the git pack's checkout. They share a
/// root but not a way in, so each keeps its own locality, and so do their
/// children.
#[tokio::test]
async fn packs_sharing_a_root_keep_their_own_locality() {
    if !git_available() {
        eprintln!("skipping: `git` is not available on PATH");
        return;
    }

    let (_src, url, _commit) = build_repo(&[
        (
            "pack.omni.yaml",
            "name: \"@org/kit\"\npacks:\n  - source: local\n    path: ./inner\n    id: inner\n",
        ),
        (
            "inner/pack.omni.yaml",
            "name: \"@org/inner\"\ntools:\n  - source: local\n    path: ./tools\n",
        ),
    ]);
    let (ws, sources_root) = workspace();
    let manager = manager(&sources_root).await;

    let git_pack: PackSourceConfiguration = serde_json::from_str(&format!(
        r#"{{"source":"git","uri":"{url}","rev":"main","id":"remote"}}"#
    ))
    .expect("valid pack source");

    // Materialize once to learn where the checkout lives.
    let (first, _refs) = expand_packs(
        &manager,
        std::slice::from_ref(&git_pack),
        ws.path(),
        false,
        None,
    )
    .await
    .expect("expand git pack");
    let checkout = first.nodes[0].root.clone();
    let relative = checkout.strip_prefix(ws.path()).expect("inside workspace");

    let local_pack: PackSourceConfiguration = serde_json::from_str(&format!(
        r#"{{"source":"local","path":"./{}","id":"mirror"}}"#,
        relative.display().to_string().replace('\\', "/")
    ))
    .expect("valid pack source");

    let (packs, _refs) =
        expand_packs(&manager, &[git_pack, local_pack], ws.path(), false, None)
            .await
            .expect("expand both");

    let by_id = |qid: &str| {
        packs
            .nodes
            .iter()
            .find(|n| n.qualified_id == qid)
            .unwrap_or_else(|| panic!("node {qid} expanded"))
    };
    assert_eq!(by_id("remote").root, by_id("mirror").root, "shared root");
    assert!(!by_id("remote").is_local);
    assert!(!by_id("remote::inner").is_local);
    assert!(by_id("mirror").is_local);
    assert!(
        by_id("mirror::inner").is_local,
        "a local child is not tainted by a git entry for the same root"
    );
}
