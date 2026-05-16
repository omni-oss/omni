mod error;
mod sys;
use std::{collections::HashSet, path::Path, sync::LazyLock};

use derive_new::new;
pub use error::Error;
use gix::progress::Discard;
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
        .fetch_then_checkout(Discard, &gix::interrupt::IS_INTERRUPTED)
        .map_err(gix::Error::from_error)?;

    let (repo, _) = checkout
        .main_worktree(Discard, &gix::interrupt::IS_INTERRUPTED)
        .map_err(gix::Error::from_error)?;

    let oid = repo
        .rev_parse_single(rev.unwrap_or("HEAD"))
        .map_err(gix::Error::from_error)?
        .detach();
    let commit = repo.find_commit(oid).map_err(gix::Error::from_error)?;
    let tree_id = commit.tree().map_err(gix::Error::from_error)?.id();

    let mut index_state = gix::index::State::from_tree(
        &tree_id,
        &repo.objects,
        Default::default(),
    )
    .map_err(gix::Error::from_error)?;

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

    gix::worktree::state::checkout(
        &mut index_state,
        workdir,
        repo.objects.clone(),
        &gix::progress::Discard,
        &gix::progress::Discard,
        &gix::interrupt::IS_INTERRUPTED,
        gix::worktree::state::checkout::Options {
            overwrite_existing: true, // overwrite files from the HEAD checkout
            destination_is_initially_empty: false,
            ..Default::default()
        },
    )
    .map_err(gix::Error::from_error)?;

    Ok(CloneInfo::new(oid.to_string()))
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
