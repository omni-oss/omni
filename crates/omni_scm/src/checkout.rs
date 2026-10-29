use std::path::{Path, PathBuf};

/// Authoritative local checkout relationships, independent of affected-file SCM
/// selection. Additional VCS providers can implement this same contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckoutLayout {
    pub checkout_root: PathBuf,
    pub primary_checkout_root: Option<PathBuf>,
}

/// Discover a Git checkout without invoking a VCS process or consulting remotes.
/// An absent repository is distinct from invalid/unreadable repository metadata.
pub fn discover_checkout(path: &Path) -> eyre::Result<Option<CheckoutLayout>> {
    let repo = match gix::discover(path) {
        Ok(repo) => repo,
        Err(error) if error.is_not_found() => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let Some(root) = repo.workdir() else {
        return Ok(None);
    };
    let checkout_root = std::fs::canonicalize(root)?;
    let primary_checkout_root = match repo.main_repo() {
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
            log::debug!("Primary Git checkout unavailable: {error}");
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
        let canonical = std::fs::canonicalize(&primary).unwrap();
        for root in [&primary, &linked] {
            let layout = discover_checkout(root).unwrap().unwrap();
            assert_eq!(
                layout.checkout_root,
                std::fs::canonicalize(root).unwrap()
            );
            assert_eq!(layout.primary_checkout_root, Some(canonical.clone()));
        }
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
}
