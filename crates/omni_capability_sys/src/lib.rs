//! # `omni_capability_sys`
//!
//! The **in-process broker** enforcement mechanism: a [`PolicyEnforcingSys`]
//! decorator that wraps any `system_traits` handle and authorizes every
//! filesystem operation against a capability policy before it touches the real
//! system.
//!
//! This is the runtime counterpart to `omni_capability_enforcement`. Where that
//! crate lowers a policy into *pre-spawn* restrictions (Deno/Node flags), this
//! crate enforces the policy *per operation* at omni's own I/O boundary — the
//! point where a sandboxed script's RPC calls (via the bridge fs services)
//! actually hit the filesystem. Because it evaluates the full policy on every
//! call, it enforces arbitrary patterns — globs, deep denials — that coarse
//! pre-spawn backends cannot express, which is why it resolves their gaps in a
//! defense-in-depth stack.
//!
//! ## Usage
//!
//! ```ignore
//! use std::sync::Arc;
//! use omni_capability_sys::{EvaluatingAuthorizer, PolicyEnforcingSys};
//!
//! let authorizer = EvaluatingAuthorizer::new(chain, roots, context);
//! let sys = Arc::new(PolicyEnforcingSys::new(real_sys, authorizer));
//! // `sys` is a drop-in `FsSys` + `ProcSys` for the bridge RPC services, but
//! // now every fs read/write is checked against the policy first.
//! ```
//!
//! The impls are **conditional** (`impl Trait for PolicyEnforcingSys<TSys, A>
//! where TSys: Trait`), so decorating never removes a capability — it only
//! gates the ones the wrapped handle already has. See [`sys`] for exactly which
//! operations are gated (filesystem) versus passed through (env / cwd).

pub mod authorize;
pub mod env;
pub mod sys;

// @anchor:mods

pub use authorize::{CapabilityAuthorizer, EvaluatingAuthorizer};
pub use env::EnvAccess;
pub use sys::PolicyEnforcingSys;

// @anchor:uses

#[cfg(test)]
mod tests {
    use std::path::Path;

    use omni_capabilities::{
        CapabilityRules, Decision, PathRoots, Request, Root,
    };
    use system_traits::impls::InMemorySys;
    use system_traits::{
        BaseFsGlobAsync, BaseFsReadAsync as _, EnvSnapshot as _,
        FsCreateDirAllAsync as _, FsGlobAsync as _, FsReadAsync as _,
        FsWriteAsync as _,
    };

    use super::*;

    /// A tiny authorizer for tests: allow reads/writes only under `/repo`,
    /// deny everything else — exactly what the real engine would produce for
    /// `allow fs.* @workspace/**`, but without wiring a full profile.
    struct RepoOnly;
    impl CapabilityAuthorizer for RepoOnly {
        fn authorize(&self, request: &Request<'_>) -> Decision {
            let allowed = matches!(
                request,
                Request::Fs { path, .. } if path.starts_with("/repo")
            );
            if allowed {
                Decision::Allow
            } else {
                Decision::Deny(omni_capabilities::DenyReason {
                    domain: request.domain(),
                    value: request.value_string(),
                    cause: omni_capabilities::DenyCause::NoMatch,
                })
            }
        }
    }

    async fn wrapped() -> PolicyEnforcingSys<InMemorySys, RepoOnly> {
        let inner = InMemorySys::default();
        inner.fs_create_dir_all_async("/repo").await.unwrap();
        inner.fs_write_async("/repo/ok.txt", b"OK").await.unwrap();
        inner.fs_write_async("/secret.txt", b"TOP").await.unwrap();
        PolicyEnforcingSys::new(inner, RepoOnly)
    }

    #[tokio::test]
    async fn allowed_read_is_forwarded() {
        let sys = wrapped().await;
        let bytes = sys.fs_read_async("/repo/ok.txt").await.unwrap();
        assert_eq!(&*bytes, b"OK");
    }

    #[tokio::test]
    async fn denied_read_returns_permission_denied() {
        let sys = wrapped().await;
        let err = sys
            .fs_read_async("/secret.txt")
            .await
            .expect_err("out-of-policy read must be denied");
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(err.to_string().contains("/secret.txt"), "{err}");
    }

    #[tokio::test]
    async fn denied_write_does_not_touch_inner() {
        let sys = wrapped().await;
        let err = sys
            .fs_write_async("/secret.txt", b"HACKED")
            .await
            .expect_err("out-of-policy write must be denied");
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        // The inner handle must be untouched: original content intact.
        let original = sys
            .inner()
            .base_fs_read_async(Path::new("/secret.txt"))
            .await
            .unwrap();
        assert_eq!(&*original, b"TOP", "denied write leaked to the real sys");
    }

    #[tokio::test]
    async fn allowed_write_is_forwarded() {
        let sys = wrapped().await;
        sys.fs_write_async("/repo/new.txt", b"hi").await.unwrap();
        let back = sys.fs_read_async("/repo/new.txt").await.unwrap();
        assert_eq!(&*back, b"hi");
    }

    /// Smoke test that the decorator composes with the real engine's
    /// `EvaluatingAuthorizer` over the base `()` profile.
    #[tokio::test]
    async fn evaluating_authorizer_enforces_the_chain() {
        let chain: CapabilityRules = serde_json::from_str(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/**"] }]"#,
        )
        .unwrap();
        let roots = PathRoots::new().with(Root::Workspace, "/repo");
        let auth = EvaluatingAuthorizer::new(chain, roots, ());

        let inner = InMemorySys::default();
        inner.fs_create_dir_all_async("/repo").await.unwrap();
        inner.fs_write_async("/repo/a.txt", b"A").await.unwrap();
        let sys = PolicyEnforcingSys::new(inner, auth);

        assert_eq!(&*sys.fs_read_async("/repo/a.txt").await.unwrap(), b"A");
        assert_eq!(
            sys.fs_read_async("/outside.txt").await.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }

    /// Allows fs freely, and `env` only for one specific variable name.
    struct EnvNamed(&'static str);
    impl CapabilityAuthorizer for EnvNamed {
        fn authorize(&self, request: &Request<'_>) -> Decision {
            let ok = match request {
                Request::Env { name } => *name == self.0,
                _ => true,
            };
            if ok {
                Decision::Allow
            } else {
                Decision::Deny(omni_capabilities::DenyReason {
                    domain: request.domain(),
                    value: request.value_string(),
                    cause: omni_capabilities::DenyCause::NoMatch,
                })
            }
        }
    }

    #[test]
    fn env_filter_is_the_default_and_gates_by_name() {
        // Fail-closed by default: only policy-allowed names survive.
        let sys = PolicyEnforcingSys::new(
            InMemorySys::default(),
            EnvNamed("ALLOWED"),
        );
        assert_eq!(sys.env_access(), EnvAccess::Filter);
        assert!(sys.env_allows("ALLOWED"));
        assert!(!sys.env_allows("ANYTHING_ELSE"));
    }

    #[test]
    fn env_filter_gates_by_name() {
        let sys = PolicyEnforcingSys::new(
            InMemorySys::default(),
            EnvNamed("ALLOWED"),
        )
        .with_env_access(EnvAccess::Filter);
        assert!(sys.env_allows("ALLOWED"));
        assert!(!sys.env_allows("SECRET_TOKEN"));
    }

    #[test]
    fn env_snapshot_drops_disallowed_variables() {
        // Unique names so we don't disturb (or depend on) the ambient env.
        unsafe {
            std::env::set_var("OCS_ENV_KEEP", "1");
            std::env::set_var("OCS_ENV_DROP", "secret");
        }
        let sys = PolicyEnforcingSys::new(
            InMemorySys::default(),
            EnvNamed("OCS_ENV_KEEP"),
        )
        .with_env_access(EnvAccess::Filter);

        let snapshot = sys.env_snapshot();
        assert_eq!(snapshot.get("OCS_ENV_KEEP").map(String::as_str), Some("1"));
        assert!(
            !snapshot.contains_key("OCS_ENV_DROP"),
            "disallowed variable leaked into the snapshot"
        );

        unsafe {
            std::env::remove_var("OCS_ENV_KEEP");
            std::env::remove_var("OCS_ENV_DROP");
        }
    }

    #[test]
    fn env_snapshot_passthrough_keeps_disallowed_variables() {
        unsafe { std::env::set_var("OCS_ENV_PASSTHROUGH", "1") };
        // Explicitly opting into passthrough leaves the snapshot unfiltered
        // (the default is `Filter`).
        let sys = PolicyEnforcingSys::new(
            InMemorySys::default(),
            EnvNamed("SOMETHING_ELSE"),
        )
        .with_env_access(EnvAccess::Passthrough);
        assert!(sys.env_snapshot().contains_key("OCS_ENV_PASSTHROUGH"));
        unsafe { std::env::remove_var("OCS_ENV_PASSTHROUGH") };
    }

    /// A minimal `BaseFsGlobAsync` handle that returns a fixed set of paths, so
    /// the decorator's glob gating can be tested in isolation.
    struct FakeGlob(Vec<std::path::PathBuf>);
    #[async_trait::async_trait]
    impl BaseFsGlobAsync for FakeGlob {
        async fn base_fs_glob_async(
            &self,
            _root_dir: &Path,
            _patterns: &[&str],
        ) -> std::io::Result<Vec<std::path::PathBuf>> {
            Ok(self.0.clone())
        }
    }

    #[tokio::test]
    async fn glob_filters_out_denied_paths() {
        let inner = FakeGlob(vec![
            "/repo/a.txt".into(),
            "/repo/nested/b.txt".into(),
            "/secret/c.txt".into(),
        ]);
        let sys = PolicyEnforcingSys::new(inner, RepoOnly);

        let matches = sys.fs_glob_async("/repo", &["**"]).await.unwrap();
        // Only the paths under the allowed `/repo` root survive.
        assert_eq!(
            matches,
            vec![
                std::path::PathBuf::from("/repo/a.txt"),
                std::path::PathBuf::from("/repo/nested/b.txt"),
            ]
        );
    }

    #[tokio::test]
    async fn glob_denies_an_unreadable_root() {
        let inner = FakeGlob(vec!["/secret/c.txt".into()]);
        let sys = PolicyEnforcingSys::new(inner, RepoOnly);

        let err = sys
            .fs_glob_async("/secret", &["**"])
            .await
            .expect_err("globbing an unreadable root must be denied");
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    }

    /// A deep, purely-synthetic path (no real-disk counterpart) must resolve to
    /// itself so the lexical decision is preserved: the symlink backstop never
    /// turns an in-memory allow into a false deny, and never loosens a deny.
    #[tokio::test]
    async fn synthetic_paths_keep_their_lexical_decision() {
        use std::io::ErrorKind::PermissionDenied;
        let sys = wrapped().await;
        // Allowed subtree, several non-existent components deep: the backstop
        // must not deny it (a later `NotFound` from the in-memory backend is
        // fine — it proves the guard permitted the operation).
        let allowed = sys.fs_read_async("/repo/a/b/c.txt").await;
        assert!(
            !matches!(&allowed, Err(e) if e.kind() == PermissionDenied),
            "a synthetic allowed path must not be denied: {allowed:?}"
        );
        // Still denied outside the subtree.
        assert_eq!(
            sys.fs_read_async("/elsewhere/x").await.unwrap_err().kind(),
            PermissionDenied
        );
    }
}

/// Live tests for the symlink-escape backstop, exercised against a real
/// filesystem (`RealSys`) with real on-disk symlinks so `canonicalize` actually
/// resolves them. Unix-only because the tests create symlinks via the Unix API;
/// the backstop itself is cross-platform.
#[cfg(all(test, unix))]
mod symlink_backstop {
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};

    use omni_capabilities::{CapabilityRules, PathRoots, Root};
    use system_traits::impls::RealSys;
    use system_traits::{
        FsCreateDirAllAsync as _, FsReadAsync as _, FsWriteAsync as _,
    };

    use super::*;

    type RealAuthorizer = EvaluatingAuthorizer<()>;

    /// An enforcing sys over the real filesystem that allows read+write within
    /// `@workspace/**`, with the workspace root registered *canonically* (as the
    /// enforcement layer does), so a resolved real path is compared against a
    /// canonical root.
    fn enforcing(ws: &Path) -> PolicyEnforcingSys<RealSys, RealAuthorizer> {
        let chain: CapabilityRules = serde_json::from_str(
            r#"[
                { "access": "allow", "domain": "fs.read",  "patterns": ["@workspace/**"] },
                { "access": "allow", "domain": "fs.write", "patterns": ["@workspace/**"] }
            ]"#,
        )
        .unwrap();
        let roots = PathRoots::new().with(Root::Workspace, ws);
        PolicyEnforcingSys::new(
            RealSys::default(),
            EvaluatingAuthorizer::new(chain, roots, ()),
        )
    }

    /// Create `<tmp>/ws` and `<tmp>/secret`, returning the tempdir guard plus
    /// the *canonical* paths (temp dirs can live under a symlinked prefix, e.g.
    /// macOS `/tmp -> /private/tmp`, so canonicalize to match the roots).
    fn workspace() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ws = tmp.path().join("ws");
        let secret = tmp.path().join("secret");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::create_dir_all(&secret).unwrap();
        let ws = std::fs::canonicalize(&ws).unwrap();
        let secret = std::fs::canonicalize(&secret).unwrap();
        (tmp, ws, secret)
    }

    #[tokio::test]
    async fn read_through_symlink_escaping_the_workspace_is_denied() {
        let (_tmp, ws, secret) = workspace();
        std::fs::write(secret.join("passwd"), b"TOP SECRET").unwrap();
        // A symlink *inside* the workspace pointing outside it.
        symlink(secret.join("passwd"), ws.join("evil")).unwrap();

        let sys = enforcing(&ws);
        let err = sys
            .fs_read_async(ws.join("evil"))
            .await
            .expect_err("a symlink escaping the workspace must be denied");
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        // The message names the resolved real target, not just the request.
        let msg = err.to_string();
        assert!(msg.contains("resolves to"), "{msg}");
        assert!(msg.contains("passwd"), "{msg}");
    }

    #[tokio::test]
    async fn read_through_symlink_staying_inside_the_workspace_is_allowed() {
        let (_tmp, ws, _secret) = workspace();
        std::fs::create_dir_all(ws.join("data")).unwrap();
        std::fs::write(ws.join("data/file.txt"), b"inside").unwrap();
        // A symlink inside the workspace pointing to another inside location.
        symlink(ws.join("data"), ws.join("link")).unwrap();

        let sys = enforcing(&ws);
        let bytes = sys
            .fs_read_async(ws.join("link/file.txt"))
            .await
            .expect("an in-workspace symlink target must be permitted");
        assert_eq!(&*bytes, b"inside");
    }

    #[tokio::test]
    async fn plain_read_without_symlinks_is_unaffected() {
        let (_tmp, ws, _secret) = workspace();
        std::fs::write(ws.join("plain.txt"), b"hello").unwrap();

        let sys = enforcing(&ws);
        assert_eq!(
            &*sys.fs_read_async(ws.join("plain.txt")).await.unwrap(),
            b"hello"
        );
    }

    #[tokio::test]
    async fn write_through_symlinked_parent_escaping_is_denied_and_leaks_nothing()
     {
        let (_tmp, ws, secret) = workspace();
        // A directory symlink inside the workspace pointing outside it; a write
        // "under" it would land in `secret`.
        symlink(&secret, ws.join("outlink")).unwrap();

        let sys = enforcing(&ws);
        let err = sys
            .fs_write_async(ws.join("outlink/new.txt"), b"escaped")
            .await
            .expect_err("a write through a symlinked parent that escapes must be denied");
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        // The denied write must not have created the file at the real target.
        assert!(
            !secret.join("new.txt").exists(),
            "denied write leaked outside the workspace"
        );
    }

    #[tokio::test]
    async fn write_of_a_new_file_inside_the_workspace_is_allowed() {
        let (_tmp, ws, _secret) = workspace();
        std::fs::create_dir_all(ws.join("sub")).unwrap();

        let sys = enforcing(&ws);
        // `sub/new.txt` does not exist yet: resolution must fall back to the
        // existing parent and still permit the write.
        sys.fs_write_async(ws.join("sub/new.txt"), b"ok")
            .await
            .expect("a new file within the workspace must be writable");
        assert_eq!(
            &*sys.fs_read_async(ws.join("sub/new.txt")).await.unwrap(),
            b"ok"
        );
    }

    #[tokio::test]
    async fn create_dir_through_symlinked_parent_escaping_is_denied() {
        let (_tmp, ws, secret) = workspace();
        symlink(&secret, ws.join("outlink")).unwrap();

        let sys = enforcing(&ws);
        let err = sys
            .fs_create_dir_all_async(ws.join("outlink/child"))
            .await
            .expect_err(
                "creating a dir through an escaping symlink must be denied",
            );
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(
            !secret.join("child").exists(),
            "denied mkdir leaked outside"
        );
    }
}
