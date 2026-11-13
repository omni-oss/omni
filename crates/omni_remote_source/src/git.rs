//! The git driver: a bare mirror per remote in the raw cache, pinned by commit.

use std::path::Path;

use url::Url;

use crate::{
    driver::{DriverContext, SourceDriver, SourceSpec},
    error::Error,
    sys::RemoteSourceSys,
};

pub const GIT_KIND: &str = "git";

/// The lockfile key of a git remote: lowercase scheme and host, no default
/// port (both from `Url` parsing), no userinfo, no trailing `/`. A `.git`
/// suffix is kept.
pub fn canonical_git_locator(uri: &Url) -> String {
    let mut uri = uri.clone();
    let _ = uri.set_username("");
    let _ = uri.set_password(None);
    let mut locator = uri.as_str().to_string();
    if locator.ends_with('/') && uri.path() != "/" {
        locator.pop();
    }
    locator
}

/// The store directory name of a remote, derived from its full identity:
/// scheme, host, port and path, without credentials. The encoding is injective
/// (every character other than letters, digits, `-` and `.` becomes `_` plus its
/// hex code, `_` included), so distinct remotes such as `host/a/b` and
/// `host/a_b` can never share a mirror, while the same remote with different
/// credentials does.
pub(crate) fn store_slug(uri: &Url) -> String {
    let mut identity = String::new();
    identity.push_str(uri.scheme());
    identity.push_str("://");
    identity.push_str(uri.host_str().unwrap_or("local"));
    if let Some(port) = uri.port() {
        identity.push_str(&format!(":{port}"));
    }
    identity.push_str(uri.path().trim_end_matches('/'));
    identity
        .trim_end_matches(".git")
        .chars()
        .flat_map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                vec![c]
            } else {
                format!("_{:02x}", c as u32).chars().collect()
            }
        })
        .collect()
}

pub struct GitDriver;

#[async_trait::async_trait]
impl<TSys: RemoteSourceSys> SourceDriver<TSys> for GitDriver {
    fn kind(&self) -> &'static str {
        GIT_KIND
    }

    fn slug(&self, spec: &SourceSpec) -> String {
        match Url::parse(&spec.locator) {
            Ok(uri) => store_slug(&uri),
            Err(_) => spec.locator.clone(),
        }
    }

    /// A locked commit already in the mirror needs no network; anything else
    /// (a moving ref, a first use, a pin the mirror lacks) fetches first.
    async fn resolve(
        &self,
        ctx: &DriverContext<'_, TSys>,
        spec: &SourceSpec,
        locked: Option<&str>,
    ) -> Result<String, Error> {
        let bare = ctx.raw_dir.as_path();
        if let Some(commit) = locked
            && omni_git_utils::resolve_commit(bare, commit).as_deref()
                == Some(commit)
        {
            return Ok(commit.to_string());
        }
        if ctx.offline {
            return Err(Error::custom(format!(
                "{} is not in the local mirror and the network is not allowed",
                spec.connection
            )));
        }

        omni_git_utils::fetch_bare(ctx.sys, spec.connection.expose(), bare)
            .await
            .map_err(|error| {
                Error::custom(format!(
                    "could not fetch {} (the network is needed when the local mirror does not have `{}`): {}",
                    spec.connection,
                    locked.unwrap_or(&spec.selector),
                    omni_redact::redact_url_userinfo(&error.to_string()),
                ))
            })?;
        tracing::trace!("fetched git repo uri: {}", spec.connection);

        match locked {
            Some(commit) => omni_git_utils::resolve_commit(bare, commit)
                .filter(|found| found == commit)
                .ok_or_else(|| {
                    Error::custom(format!(
                        "locked commit `{commit}` of {} is no longer served by the remote; run `omni remote-sources install --update` to re-lock it",
                        spec.connection
                    ))
                }),
            None => omni_git_utils::resolve_commit(bare, &spec.selector)
                .ok_or_else(|| {
                    Error::custom(format!(
                        "revision `{}` was not found in {}",
                        spec.selector, spec.connection
                    ))
                }),
        }
    }

    async fn export(
        &self,
        ctx: &DriverContext<'_, TSys>,
        _spec: &SourceSpec,
        pin: &str,
        dest: &Path,
    ) -> Result<(), Error> {
        omni_git_utils::export_tree(ctx.sys, &ctx.raw_dir, pin, dest).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slug(uri: &str) -> String {
        store_slug(&Url::parse(uri).unwrap())
    }

    fn locator(uri: &str) -> String {
        canonical_git_locator(&Url::parse(uri).unwrap())
    }

    #[test]
    fn distinct_remotes_never_share_a_store_name() {
        let all = [
            "https://host/a/b",
            "https://host/a_b",
            "https://host/a/b/c",
            "http://host/a/b",
            "https://host:8443/a/b",
            "https://other/a/b",
            "ssh://git@host/a/b",
        ];
        let mut seen = std::collections::HashSet::new();
        for uri in all {
            assert!(seen.insert(slug(uri)), "collision for {uri}");
        }
    }

    #[test]
    fn the_same_remote_keeps_one_name_regardless_of_credentials_or_dot_git() {
        let plain = slug("https://host/owner/repo");
        assert_eq!(plain, slug("https://user:token@host/owner/repo"));
        assert_eq!(plain, slug("https://host/owner/repo.git"));
        assert_eq!(plain, slug("https://host/owner/repo/"));
    }

    #[test]
    fn a_name_is_a_safe_single_path_segment() {
        let name = slug("https://host:8443/a b/c?x=1#frag");
        assert!(
            name.chars()
                .all(|c| c.is_ascii_alphanumeric()
                    || matches!(c, '-' | '.' | '_')),
            "{name}"
        );
    }

    #[test]
    fn locators_are_canonical_and_credential_free() {
        for uri in [
            "https://host/r.git/",
            "https://HOST:443/r.git",
            "https://user@host/r.git",
            "https://user:token@host/r.git",
        ] {
            assert_eq!(locator(uri), "https://host/r.git", "{uri}");
        }
        assert_eq!(locator("https://host:8443/r"), "https://host:8443/r");
    }
}
