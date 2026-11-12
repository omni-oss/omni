#![allow(clippy::redundant_field_names)]

mod error;
mod sys;
use std::{collections::HashSet, path::Path, sync::LazyLock};

use derive_new::new;
pub use error::Error;
use gix::{
    index, interrupt,
    progress::{self, Discard},
    worktree::{self, state::checkout::Options},
};
pub use sys::GitUtilsSys;
use url::Url;

#[derive(Debug, new)]
pub struct CloneInfo {
    pub commit: String,
}

pub async fn clone_repo(
    sys: &impl GitUtilsSys,
    uri: &str,
    rev: Option<&str>,
    destination: &Path,
) -> Result<CloneInfo, Error> {
    sys.fs_create_dir_all_async(destination).await?;

    let mut prepare_clone =
        gix::prepare_clone(uri, destination).map_err(gix::Error::from_error)?;

    let (mut checkout, _outcome) = prepare_clone
        .fetch_then_checkout(Discard, &interrupt::IS_INTERRUPTED)
        .map_err(gix::Error::from_error)?;

    let (repo, _outcome) = checkout
        .main_worktree(Discard, &interrupt::IS_INTERRUPTED)
        .map_err(gix::Error::from_error)?;

    let oid = peel_commit(&repo, rev.unwrap_or("HEAD"))?;
    repo.reference(
        "HEAD",
        oid,
        gix::refs::transaction::PreviousValue::Any,
        "clone checkout",
    )
    .map_err(gix::Error::from_error)?;

    let workdir = repo
        .workdir()
        .ok_or_else(|| eyre::eyre!("repository has no workdir"))?;
    // Overwrite the files of the HEAD checkout the clone made.
    write_commit_tree(&repo, oid, workdir)?;

    Ok(CloneInfo::new(oid.to_string()))
}

/// The refspecs a bare mirror fetches: every branch and tag, kept under the
/// same names. A bare clone configures none of its own, and a fetch without
/// any fails, so they are always passed explicitly.
const MIRROR_REFSPECS: [&str; 2] =
    ["+refs/heads/*:refs/heads/*", "+refs/tags/*:refs/tags/*"];

/// Bring the bare repository at `bare_dir` up to date with `uri`, creating it on
/// the first call. Every branch and tag is fetched, so any commit a ref reaches
/// can be exported later without the network.
///
/// A first clone lands in a sibling temporary directory and is renamed into
/// place, so a failed or interrupted clone never leaves a half-built repository
/// behind. Callers serialize calls for one `bare_dir` (concurrent fetches into
/// one repository race on its refs).
pub async fn fetch_bare(
    sys: &impl GitUtilsSys,
    uri: &str,
    bare_dir: &Path,
) -> Result<(), Error> {
    if let Ok(repo) = gix::open(bare_dir) {
        let remote = repo
            .remote_at(uri)
            .map_err(gix::Error::from_error)?
            .with_refspecs(MIRROR_REFSPECS, gix::remote::Direction::Fetch)
            .map_err(gix::Error::from_error)?;
        remote
            .connect(gix::remote::Direction::Fetch)
            .map_err(gix::Error::from_error)?
            .prepare_fetch(Discard, Default::default())
            .map_err(gix::Error::from_error)?
            .receive(Discard, &interrupt::IS_INTERRUPTED)
            .map_err(gix::Error::from_error)?;
        return Ok(());
    }

    let parent = bare_dir
        .parent()
        .ok_or_else(|| Error::custom("bare repository path has no parent"))?;
    sys.fs_create_dir_all_async(parent).await?;
    let mut name = bare_dir
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(format!(".clone-{}", std::process::id()));
    let staging = parent.join(name);
    // A leftover from a crashed attempt of this process id is not a repository
    // we trust.
    let _ = std::fs::remove_dir_all(&staging);
    sys.fs_create_dir_all_async(&staging).await?;

    let cloned = (|| -> Result<(), Error> {
        let mut prepare = gix::prepare_clone_bare(uri, &staging)
            .map_err(gix::Error::from_error)?;
        prepare
            .fetch_only(Discard, &interrupt::IS_INTERRUPTED)
            .map_err(gix::Error::from_error)?;
        Ok(())
    })();
    if let Err(error) = cloned {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    // A directory that is not a repository (its `HEAD` was deleted, say) would
    // block the rename. Set it aside, and put it back if publishing fails so a
    // failed attempt never loses what was there.
    let aside = if bare_dir.exists() {
        let mut name = bare_dir
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_default();
        name.push(format!(".invalid-{}", std::process::id()));
        let aside = parent.join(name);
        let _ = std::fs::remove_dir_all(&aside);
        std::fs::rename(bare_dir, &aside)?;
        Some(aside)
    } else {
        None
    };
    if let Err(error) = std::fs::rename(&staging, bare_dir) {
        let _ = std::fs::remove_dir_all(&staging);
        if let Some(aside) = &aside {
            let _ = std::fs::rename(aside, bare_dir);
        }
        return Err(error.into());
    }
    if let Some(aside) = aside {
        let _ = std::fs::remove_dir_all(aside);
    }
    Ok(())
}

/// The commit `rev` (a branch, tag, annotated tag or commit id) names in the
/// bare repository at `bare_dir`, without touching the network. `None` when the
/// repository does not have it.
pub fn resolve_commit(bare_dir: &Path, rev: &str) -> Option<String> {
    let repo = gix::open(bare_dir).ok()?;
    peel_commit(&repo, rev).ok().map(|id| id.to_string())
}

/// The commit `rev` names. `rev` may resolve to an annotated tag (or a chain of
/// tags) rather than a commit directly, so peel through any tags to the
/// underlying commit.
fn peel_commit(
    repo: &gix::Repository,
    rev: &str,
) -> Result<gix::ObjectId, Error> {
    Ok(repo
        .rev_parse_single(rev)
        .map_err(gix::Error::from_error)?
        .object()
        .map_err(gix::Error::from_error)?
        .peel_to_commit()
        .map_err(gix::Error::from_error)?
        .id)
}

/// Write the files of `commit` into `dest` as a plain directory (no `.git`),
/// applying the repository's filters (line endings) like a checkout does.
/// `dest` is created and should not already exist.
pub async fn export_tree(
    sys: &impl GitUtilsSys,
    bare_dir: &Path,
    commit: &str,
    dest: &Path,
) -> Result<(), Error> {
    let repo = gix::open(bare_dir).map_err(gix::Error::from_error)?;
    let oid = gix::ObjectId::from_hex(commit.as_bytes()).map_err(|e| {
        Error::custom(format!("invalid commit id {commit}: {e}"))
    })?;
    sys.fs_create_dir_all_async(dest).await?;
    write_commit_tree(&repo, oid, dest)
}

/// Write the files of `commit` (a commit id, or a tag that peels to one) into
/// `dest`, applying the repository's filters (line endings) like a checkout
/// does and overwriting what is there. Shared by the full clone and the tree
/// export so the two cannot drift.
fn write_commit_tree(
    repo: &gix::Repository,
    commit: gix::ObjectId,
    dest: &Path,
) -> Result<(), Error> {
    let tree_id = repo
        .find_object(commit)
        .map_err(gix::Error::from_error)?
        .peel_to_commit()
        .map_err(gix::Error::from_error)?
        .tree_id()
        .map_err(gix::Error::from_error)?
        .detach();

    let mut index_state =
        index::State::from_tree(&tree_id, &repo.objects, Default::default())?;

    // The filter pipeline is keyed by the tree, not the commit.
    let (pipeline, _) = repo.filter_pipeline(Some(tree_id))?;
    let (filters, _) = pipeline.into_parts();

    worktree::state::checkout(
        &mut index_state,
        dest,
        repo.objects.clone(),
        &progress::Discard,
        &progress::Discard,
        &interrupt::IS_INTERRUPTED,
        Options {
            overwrite_existing: true,
            ..Options::new(filters)
        },
    )?;
    Ok(())
}

static UNALLOWED_SPEC_CHARS: LazyLock<HashSet<char>> = LazyLock::new(|| {
    ['/', '\\', ':', '*', '?', '"', '<', '>', '|', '\'']
        .into_iter()
        .collect::<HashSet<_>>()
});

pub fn url_to_safe_dir_name(repo_url: &str) -> Result<String, Error> {
    // 1. Normalize SCP-like SSH syntax (git@domain.com:owner/repo.git)
    // into a standard URL format (ssh://git@domain.com/owner/repo.git)
    let normalized_url = if !repo_url.contains("://") && repo_url.contains('@')
    {
        format!("ssh://{}", repo_url.replace(':', "/"))
    } else {
        repo_url.to_string()
    };

    // 2. Parse the URL
    let parsed = Url::parse(&normalized_url)?;

    // 3. Extract the host (domain) and clean it up (lowercase, no port)
    let host = parsed.host_str().unwrap_or("unknown_domain").to_lowercase();

    // 4. Extract the path, remove leading/trailing slashes and ".git"
    let path = parsed.path().trim_matches('/');
    let clean_path = path.trim_end_matches(".git");

    // 5. Combine domain and path
    let combined = format!("{}_{}", host, clean_path);

    // 6. Sanitize: Replace any character that isn't alphanumeric, dash, or underscore
    let safe_dir: String = combined
        .chars()
        .map(|c| {
            if c.is_alphanumeric()
                || c == '-'
                || c == '_'
                || c == '.' && !UNALLOWED_SPEC_CHARS.contains(&c)
            {
                c
            } else {
                '_' // Replaces dots in domains and slashes in paths with underscores
            }
        })
        .collect();

    Ok(safe_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    // --- HTTPS ---
    #[case(
        "https://github.com/rust-lang/cargo.git",
        "github.com_rust-lang_cargo"
    )]
    #[case(
        "https://gitlab.com/username/my-project",
        "gitlab.com_username_my-project"
    )]
    // --- SSH Standard ---
    #[case("ssh://git@github.com/org/repo.git", "github.com_org_repo")]
    // --- SCP-like SSH Syntax ---
    #[case("git@github.com:owner/repo.git", "github.com_owner_repo")]
    #[case(
        "git@gitlab.company.internal:team/subgroup/project.git",
        "gitlab.company.internal_team_subgroup_project"
    )]
    // --- Custom Ports (Ports should be stripped out) ---
    #[case(
        "https://gitea.local:8443/devops/ci-scripts.git",
        "gitea.local_devops_ci-scripts"
    )]
    #[case(
        "ssh://git@bitbucket.org:2222/company/infrastructure.git",
        "bitbucket.org_company_infrastructure"
    )]
    // --- Deeply Nested Paths (GitLab subgroups) ---
    #[case(
        "https://gitlab.com/org/department/team/project.git",
        "gitlab.com_org_department_team_project"
    )]
    // --- Edge Cases & Special Characters ---
    #[case("https://GitHub.Com/Caps/Mix.git", "github.com_Caps_Mix")]
    // Casing check, the domain name must be lowercased, the path names are unaffected
    #[case(
        "https://github.com/owner/repo.inside.dots.git",
        "github.com_owner_repo.inside.dots"
    )]
    #[case(
        "https://github.com/owner/repo_with_underscores-and-dashes",
        "github.com_owner_repo_with_underscores-and-dashes"
    )]
    fn test_url_to_safe_dir_name_variants(
        #[case] input_url: &str,
        #[case] expected_slug: &str,
    ) {
        let result = url_to_safe_dir_name(input_url)
            .unwrap_or_else(|_| panic!("Failed to parse URL: {}", input_url));

        assert_eq!(
            result, expected_slug,
            "\nFailed Case!\nInput:    {}\nExpected: {}\nGot:      {}\n",
            input_url, expected_slug, result
        );
    }

    #[test]
    fn test_invalid_url_returns_error() {
        // Completely garbage string that the `url` crate will fail to parse
        let invalid_url = "this-is-not-a-valid-url-at-all";
        let result = url_to_safe_dir_name(invalid_url);

        assert!(
            result.is_err(),
            "Expected an error for an invalid URL format"
        );
    }

    #[test]
    fn test_no_extension_handling() {
        // Ensure that having or not having `.git` results in the identical output
        let with_git =
            url_to_safe_dir_name("https://github.com/user/repo.git").unwrap();
        let without_git =
            url_to_safe_dir_name("https://github.com/user/repo").unwrap();

        assert_eq!(with_git, without_git);
    }
}
