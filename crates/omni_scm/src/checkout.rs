use std::path::{Path, PathBuf};

/// Authoritative local checkout relationships, independent of affected-file SCM
/// selection. Additional VCS providers can implement this same contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckoutLayout {
    pub checkout_root: PathBuf,
    pub primary_checkout_root: Option<PathBuf>,
}

/// Git checkouts sharing one common administrative directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckoutFamily {
    pub checkout_root: PathBuf,
    pub roots: Vec<PathBuf>,
}

/// Enumerate the current checkout and its Git-registered peers. Unlike cache
/// path resolution, pruning cannot proceed with a partial family: a missing
/// or unverifiable worktree is an error, not an empty checkout.
pub fn discover_checkout_family(
    path: &Path,
) -> eyre::Result<Option<CheckoutFamily>> {
    let Some(repo) = discover_repo(path)? else {
        return Ok(None);
    };
    let Some(root) = repo.workdir() else {
        return Ok(None);
    };
    let checkout_root = std::fs::canonicalize(root)?;
    let common_dir = std::fs::canonicalize(repo.common_dir())?;
    let main = if repo.kind() == gix::repository::Kind::LinkedWorkTree {
        gix::open(&common_dir)?
    } else {
        repo.main_repo()?
    };
    let mut roots = vec![checkout_root.clone()];
    if !main.is_bare() {
        let root = main.workdir().ok_or_else(|| {
            eyre::eyre!("the primary Git checkout has no worktree")
        })?;
        roots.push(verify_worktree_root(root, &common_dir)?);
    }
    let worktrees = repo.worktrees()?;
    // gix omits registrations with a missing or inaccessible `gitdir`
    // pointer. Do not authorize a prune based on the resulting partial list.
    match std::fs::read_dir(common_dir.join("worktrees")) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                let name = entry.file_name();
                if entry.metadata()?.is_dir()
                    && !worktrees.iter().any(|tree| {
                        tree.git_dir().file_name() == Some(name.as_os_str())
                    })
                {
                    eyre::bail!(
                        "cannot read Git worktree registration {}",
                        entry.path().display()
                    );
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    for worktree in worktrees {
        roots.push(verify_worktree_root(
            &worktree.base()?,
            &std::fs::canonicalize(worktree.git_dir())?,
        )?);
    }
    roots.sort();
    roots.dedup();
    Ok(Some(CheckoutFamily {
        checkout_root,
        roots,
    }))
}

fn verify_worktree_root(root: &Path, git_dir: &Path) -> eyre::Result<PathBuf> {
    use eyre::WrapErr as _;

    let root = std::fs::canonicalize(root).wrap_err_with(|| {
        format!("cannot access Git worktree {}", root.display())
    })?;
    let repo = gix::discover(&root)?;
    if repo
        .workdir()
        .map(std::fs::canonicalize)
        .transpose()?
        .as_ref()
        != Some(&root)
        || std::fs::canonicalize(repo.git_dir())? != git_dir
    {
        eyre::bail!(
            "{} is not a worktree of the expected Git repository",
            root.display()
        );
    }
    Ok(root)
}

fn discover_repo(path: &Path) -> eyre::Result<Option<gix::Repository>> {
    match gix::discover(path) {
        Ok(repo) => Ok(Some(repo)),
        Err(error)
            if matches!(
                error.downcast_any_ref::<gix::discover::upwards::Error>(),
                Some(gix::discover::upwards::Error::NoGitRepository { .. }
                    | gix::discover::upwards::Error::NoGitRepositoryWithinCeiling { .. }
                    | gix::discover::upwards::Error::NoGitRepositoryWithinFs { .. })
            ) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Discover a Git checkout without invoking a VCS process or consulting remotes.
/// An absent repository is distinct from invalid/unreadable repository metadata.
pub fn discover_checkout(path: &Path) -> eyre::Result<Option<CheckoutLayout>> {
    let Some(repo) = discover_repo(path)? else {
        return Ok(None);
    };
    let Some(root) = repo.workdir() else {
        return Ok(None);
    };
    let checkout_root = std::fs::canonicalize(root)?;
    // Normalize commondir's `../..` before opening: configuration trust/origin
    // checks must see the same metadata path as core.worktree's declaring file.
    let main = if repo.kind() == gix::repository::Kind::LinkedWorkTree {
        gix::open(std::fs::canonicalize(repo.common_dir())?)
    } else {
        repo.main_repo()
    };
    let primary_checkout_root = match main {
        Ok(primary) => primary.workdir().and_then(|root| {
            let root = std::fs::canonicalize(root).ok()?;
            // Opening a common directory alone can infer its parent as a
            // workdir. Require the checkout's own metadata to confirm it.
            let check = gix::discover(&root).ok()?;
            (std::fs::canonicalize(check.workdir()?).ok()? == root
                && std::fs::canonicalize(check.common_dir()).ok()?
                    == std::fs::canonicalize(repo.common_dir()).ok()?)
            .then_some(root)
        }),
        Err(error) => {
            tracing::debug!("Primary Git checkout unavailable: {error}");
            None
        }
    };
    Ok(Some(CheckoutLayout {
        checkout_root,
        primary_checkout_root,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(root: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .current_dir(root)
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn ordinary_and_detached_worktrees_identify_the_primary() {
        let temp = tempfile::tempdir().unwrap();
        let primary = temp.path().join("primary");
        std::fs::create_dir(&primary).unwrap();
        git(&primary, &["init"]);
        git(&primary, &["commit", "--allow-empty", "-m", "init"]);
        let linked = temp.path().join("linked");
        git(
            &primary,
            &["worktree", "add", "--detach", linked.to_str().unwrap()],
        );
        let branched = temp.path().join("branched");
        git(
            &primary,
            &[
                "worktree",
                "add",
                "-b",
                "feature",
                branched.to_str().unwrap(),
            ],
        );
        let canonical = std::fs::canonicalize(&primary).unwrap();
        let mut expected_roots = [&primary, &linked, &branched]
            .map(|root| std::fs::canonicalize(root).unwrap())
            .to_vec();
        expected_roots.sort();
        for root in [&primary, &linked, &branched] {
            let layout = discover_checkout(root).unwrap().unwrap();
            assert_eq!(
                layout.checkout_root,
                std::fs::canonicalize(root).unwrap()
            );
            assert_eq!(layout.primary_checkout_root, Some(canonical.clone()));
            let family = discover_checkout_family(root).unwrap().unwrap();
            assert_eq!(family.checkout_root, layout.checkout_root);
            assert_eq!(family.roots, expected_roots);
        }
        std::fs::rename(&linked, temp.path().join("unavailable")).unwrap();
        assert!(discover_checkout_family(&primary).is_err());
    }

    #[test]
    fn incomplete_worktree_registration_does_not_yield_a_partial_family() {
        let temp = tempfile::tempdir().unwrap();
        git(temp.path(), &["init"]);
        std::fs::create_dir_all(temp.path().join(".git/worktrees/incomplete"))
            .unwrap();
        assert!(
            discover_checkout_family(temp.path())
                .unwrap_err()
                .to_string()
                .contains("cannot read Git worktree registration")
        );
    }

    #[test]
    fn separate_metadata_is_not_mistaken_for_a_checkout() {
        let temp = tempfile::tempdir().unwrap();
        let primary = temp.path().join("primary");
        let metadata = temp.path().join("metadata");
        std::fs::create_dir(&primary).unwrap();
        git(
            &primary,
            &["init", "--separate-git-dir", metadata.to_str().unwrap()],
        );
        git(&primary, &["commit", "--allow-empty", "-m", "init"]);
        let linked = temp.path().join("linked");
        git(
            &primary,
            &["worktree", "add", "--detach", linked.to_str().unwrap()],
        );
        let layout = discover_checkout(&linked).unwrap().unwrap();
        // Git does not record a reverse pointer for this layout unless
        // core.worktree is configured. Never invent a primary from its parent.
        if let Some(root) = layout.primary_checkout_root {
            assert_eq!(root, std::fs::canonicalize(&primary).unwrap());
        }
    }

    #[test]
    fn no_repository_and_bare_repository_have_no_checkout() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(discover_checkout(temp.path()).unwrap(), None);
        git(temp.path(), &["init", "--bare"]);
        assert_eq!(discover_checkout(temp.path()).unwrap(), None);
    }

    #[test]
    fn broken_git_configuration_is_reported_not_treated_as_absent() {
        let temp = tempfile::tempdir().unwrap();
        git(temp.path(), &["init"]);
        std::fs::write(
            temp.path().join(".git/config"),
            "[invalid configuration",
        )
        .unwrap();
        assert!(discover_checkout(temp.path()).is_err());
    }

    #[test]
    fn submodules_are_independent_and_clones_do_not_share() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let parent = temp.path().join("parent");
        for root in [&source, &parent] {
            std::fs::create_dir(root).unwrap();
            git(root, &["init"]);
            git(root, &["commit", "--allow-empty", "-m", "init"]);
        }
        git(
            &parent,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                source.to_str().unwrap(),
                "sub",
            ],
        );
        let sub = parent.join("sub");
        let layout = discover_checkout(&sub).unwrap().unwrap();
        assert_eq!(layout.checkout_root, std::fs::canonicalize(&sub).unwrap());
        assert_eq!(
            layout.primary_checkout_root,
            Some(std::fs::canonicalize(&sub).unwrap())
        );
        assert_ne!(
            layout.primary_checkout_root,
            Some(std::fs::canonicalize(&source).unwrap())
        );
    }

    #[test]
    fn bare_worktree_family_has_no_primary_checkout() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        std::fs::create_dir(&source).unwrap();
        git(&source, &["init"]);
        git(&source, &["commit", "--allow-empty", "-m", "init"]);
        let bare = temp.path().join("bare");
        git(
            &source,
            &[
                "clone",
                "--bare",
                source.to_str().unwrap(),
                bare.to_str().unwrap(),
            ],
        );
        let linked = temp.path().join("linked");
        git(
            &bare,
            &["worktree", "add", "--detach", linked.to_str().unwrap()],
        );
        assert_eq!(
            discover_checkout(&linked)
                .unwrap()
                .unwrap()
                .primary_checkout_root,
            None
        );
        let family = discover_checkout_family(&linked).unwrap().unwrap();
        assert_eq!(family.roots, [std::fs::canonicalize(linked).unwrap()]);
    }

    #[test]
    fn separate_metadata_with_authoritative_worktree_and_missing_primary() {
        let temp = tempfile::tempdir().unwrap();
        let primary = temp.path().join("primary");
        let metadata = temp.path().join("metadata");
        std::fs::create_dir(&primary).unwrap();
        git(
            &primary,
            &["init", "--separate-git-dir", metadata.to_str().unwrap()],
        );
        git(
            &primary,
            &["config", "core.worktree", primary.to_str().unwrap()],
        );
        git(&primary, &["commit", "--allow-empty", "-m", "init"]);
        let linked = temp.path().join("linked");
        git(
            &primary,
            &["worktree", "add", "--detach", linked.to_str().unwrap()],
        );
        assert_eq!(
            discover_checkout(&linked)
                .unwrap()
                .unwrap()
                .primary_checkout_root,
            Some(std::fs::canonicalize(&primary).unwrap())
        );
        std::fs::rename(&primary, temp.path().join("unavailable")).unwrap();
        assert_eq!(
            discover_checkout(&linked)
                .unwrap()
                .unwrap()
                .primary_checkout_root,
            None
        );
    }
}
