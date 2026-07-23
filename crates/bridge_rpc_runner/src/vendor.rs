//! Embeds the prebuilt `@omni-oss/bridge-service` JavaScript bundle into the
//! binary and restores it on demand to a location appropriate for the calling
//! context.
//!
//! The bundle (everything under `services/bridge-service/dist` plus its
//! `package.json`) is embedded at compile time via [`rust_embed`]. At runtime
//! [`VendoredBridgeService::ensure`] writes those files to disk so a JS runtime
//! (node/bun/deno) can execute them.
//!
//! Restore location
//! ----------------
//! The destination depends on the *context directory* passed by the caller:
//!
//! * If `<context>/node_modules` exists, the bundle is written to
//!   `<context>/node_modules/@omni-oss/bridge-service-vendored` so it resolves
//!   like any other installed package. The embedded `package.json` `name` is
//!   rewritten to `@omni-oss/bridge-service-vendored` to match.
//! * Otherwise it is written to
//!   `<context>/.omni/vendored-scripts/bridge-service`.
//!
//! Versioning
//! ----------
//! The caller bakes a *version* into [`VendoredBridgeService::new`] (typically
//! the embedding binary's version). A `.omni-bridge-service-version` marker is
//! written alongside the bundle; if the marker already matches the requested
//! version (and the entrypoint exists) the existing files are reused untouched,
//! otherwise the destination is overwritten. This guarantees the binary always
//! runs against the bundle it shipped with.

use std::path::{Path, PathBuf};

use rust_embed::RustEmbed;

use crate::{BridgeRunnerError, error};

/// The package name used for the restored bundle when written into a
/// `node_modules` directory.
pub const VENDORED_PACKAGE_NAME: &str = "@omni-oss/bridge-service-vendored";

/// Name of the marker file recording which version is currently materialized.
const DEFAULT_VERSION_MARKER: &str = ".omni-bridge-service-version";

/// Relative path (within the bundle root) of the CLI entrypoint.
const ENTRYPOINT_REL: &str = "dist/bridge-service-cli.mjs";

/// File name (within `dist`) of the CLI entrypoint.
const ENTRYPOINT_DIST_NAME: &str = "bridge-service-cli.mjs";

/// The embedded `@omni-oss/bridge-service` `dist` directory.
///
/// We embed `dist` directly (a relative path with no `node_modules`, so the
/// compile-time scan never touches the package's symlink farm). The sibling
/// `package.json` is embedded separately via [`PACKAGE_JSON`].
#[derive(RustEmbed)]
#[folder = "../../services/bridge-service/dist"]
struct BridgeServiceDist;

/// The embedded `@omni-oss/bridge-service` `package.json`.
const PACKAGE_JSON: &[u8] =
    include_bytes!("../../../services/bridge-service/package.json");

/// Where a vendored bundle was materialized on disk.
#[derive(Debug, Clone)]
pub struct VendoredLocation {
    /// The package root directory (contains `dist/` and `package.json`).
    pub root: PathBuf,
    /// The CLI entrypoint (`<root>/dist/bridge-service-cli.mjs`).
    pub entrypoint: PathBuf,
}

/// Restores the embedded `@omni-oss/bridge-service` bundle to disk.
#[derive(Debug, Clone)]
pub struct VendoredBridgeService {
    version: String,
    version_file_name: String,
}

impl VendoredBridgeService {
    /// Creates a vendoring helper baked to `version`.
    ///
    /// `version` should uniquely identify the embedded bundle for the lifetime
    /// of the binary (e.g. the binary's own `CARGO_PKG_VERSION`), so that
    /// upgrading the binary refreshes any stale on-disk copy.
    /// The `version_file_name` is the name of the marker file that records which version is currently materialized on disk.
    /// It defaults to `.omni-bridge-service-version` but can be customized for testing purposes.
    pub fn new(
        version: impl Into<String>,
        version_file_name: Option<impl Into<String>>,
    ) -> Self {
        Self {
            version: version.into(),
            version_file_name: version_file_name
                .map(Into::into)
                .unwrap_or_else(|| DEFAULT_VERSION_MARKER.to_string()),
        }
    }

    /// Resolves the destination directory for `context_dir`.
    fn resolve_target(&self, context_dir: &Path) -> PathBuf {
        let node_modules = context_dir.join("node_modules");
        if node_modules.is_dir() {
            node_modules
                .join("@omni-oss")
                .join("bridge-service-vendored")
        } else {
            context_dir
                .join(".omni")
                .join("vendored-scripts")
                .join("bridge-service")
        }
    }

    /// Ensures the bundle is present (and up to date) relative to
    /// `context_dir`, returning where it lives and how to launch it.
    ///
    /// Files are only (re)written when the on-disk version marker does not match
    /// the baked [`version`](Self::new) or the entrypoint is missing.
    pub async fn ensure(
        &self,
        context_dir: &Path,
    ) -> Result<VendoredLocation, BridgeRunnerError> {
        let root = self.resolve_target(context_dir);
        let entrypoint = root.join(ENTRYPOINT_REL);

        if self.is_up_to_date(&root, &entrypoint).await {
            trace::trace!(
                root = %root.display(),
                "vendored bridge-service already up to date"
            );
            return Ok(VendoredLocation {
                root: omni_utils::path::clean(root),
                entrypoint: omni_utils::path::clean(entrypoint),
            });
        }

        trace::debug!(
            root = %root.display(),
            version = %self.version,
            "materializing vendored bridge-service"
        );

        // Start from a clean slate so removed files don't linger.
        if tokio::fs::try_exists(&root).await.unwrap_or(false) {
            tokio::fs::remove_dir_all(&root).await.map_err(|e| {
                error::error!(
                    "failed to clear vendored dir {}: {e}",
                    root.display()
                )
            })?;
        }

        let dist_root = root.join("dist");
        tokio::fs::create_dir_all(&dist_root).await.map_err(|e| {
            error::error!("failed to create dir {}: {e}", dist_root.display())
        })?;

        // Write the (renamed) package.json at the bundle root.
        let package_json = rewrite_package_name(PACKAGE_JSON)?;
        tokio::fs::write(root.join("package.json"), &package_json)
            .await
            .map_err(|e| error::error!("failed to write package.json: {e}"))?;

        // Write every embedded `dist` file under `<root>/dist`.
        let mut wrote_entrypoint = false;
        for path in BridgeServiceDist::iter() {
            let file = BridgeServiceDist::get(&path).ok_or_else(|| {
                error::error!("embedded asset disappeared: {path}")
            })?;

            let dest = dist_root.join(path.as_ref());
            if let Some(parent) = dest.parent() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    error::error!(
                        "failed to create dir {}: {e}",
                        parent.display()
                    )
                })?;
            }

            tokio::fs::write(&dest, &file.data).await.map_err(|e| {
                error::error!("failed to write {}: {e}", dest.display())
            })?;

            if path.as_ref() == ENTRYPOINT_DIST_NAME {
                wrote_entrypoint = true;
            }
        }

        if !wrote_entrypoint {
            return Err(error::error!(
                "vendored bridge-service is missing its entrypoint `{ENTRYPOINT_REL}`; \
                 was the `@omni-oss/bridge-service` build run before embedding?"
            )
            .into());
        }

        tokio::fs::write(
            root.join(&self.version_file_name),
            self.version.as_bytes(),
        )
        .await
        .map_err(|e| error::error!("failed to write version marker: {e}"))?;

        Ok(VendoredLocation {
            root: omni_utils::path::clean(root),
            entrypoint: omni_utils::path::clean(entrypoint),
        })
    }

    async fn is_up_to_date(&self, root: &Path, entrypoint: &Path) -> bool {
        if !tokio::fs::try_exists(entrypoint).await.unwrap_or(false) {
            return false;
        }
        match tokio::fs::read_to_string(root.join(DEFAULT_VERSION_MARKER)).await
        {
            Ok(marker) => marker.trim() == self.version,
            Err(_) => false,
        }
    }
}

/// Parses the embedded `package.json`, rewrites its `name` to
/// [`VENDORED_PACKAGE_NAME`], and reserializes it.
fn rewrite_package_name(raw: &[u8]) -> Result<Vec<u8>, BridgeRunnerError> {
    let mut value: serde_json::Value = serde_json::from_slice(raw)
        .map_err(|e| error::error!("failed to parse package.json: {e}"))?;

    if let Some(obj) = value.as_object_mut() {
        obj.insert(
            "name".to_string(),
            serde_json::Value::String(VENDORED_PACKAGE_NAME.to_string()),
        );
    }

    serde_json::to_vec_pretty(&value).map_err(|e| {
        error::error!("failed to serialize package.json: {e}").into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ensure_materializes_bundle_and_renames_package() {
        let tmp = tempfile::tempdir().expect("tempdir");

        let loc = VendoredBridgeService::new("test-version", None::<String>)
            .ensure(tmp.path())
            .await
            .expect(
                "ensure should succeed (run the bridge-service build first)",
            );

        // No node_modules in a fresh temp dir, so it lands under .omni.
        assert!(
            loc.root.ends_with(".omni/vendored-scripts/bridge-service"),
            "unexpected root: {}",
            loc.root.display()
        );
        assert!(loc.entrypoint.exists(), "entrypoint should be written");

        let pkg = tokio::fs::read_to_string(loc.root.join("package.json"))
            .await
            .expect("package.json should be written");
        assert!(
            pkg.contains(VENDORED_PACKAGE_NAME),
            "package.json name should be rewritten"
        );
    }

    #[tokio::test]
    async fn ensure_writes_into_node_modules_when_present() {
        let tmp = tempfile::tempdir().expect("tempdir");
        tokio::fs::create_dir_all(tmp.path().join("node_modules"))
            .await
            .expect("create node_modules");

        let loc = VendoredBridgeService::new("v1", None::<String>)
            .ensure(tmp.path())
            .await
            .expect("ensure should succeed");

        assert!(
            loc.root
                .ends_with("node_modules/@omni-oss/bridge-service-vendored"),
            "unexpected root: {}",
            loc.root.display()
        );
    }

    #[tokio::test]
    async fn ensure_is_idempotent_for_same_version() {
        let tmp = tempfile::tempdir().expect("tempdir");

        let first = VendoredBridgeService::new("v1", None::<String>)
            .ensure(tmp.path())
            .await
            .expect("first ensure");
        let marker = first.root.join(DEFAULT_VERSION_MARKER);
        let written_at = tokio::fs::metadata(&marker)
            .await
            .and_then(|m| m.modified())
            .expect("marker mtime");

        let second = VendoredBridgeService::new("v1", None::<String>)
            .ensure(tmp.path())
            .await
            .expect("second ensure");

        assert_eq!(first.entrypoint, second.entrypoint);
        // The marker should not have been rewritten on the second call.
        let written_at_2 = tokio::fs::metadata(&marker)
            .await
            .and_then(|m| m.modified())
            .expect("marker mtime");
        assert_eq!(written_at, written_at_2);
    }
}
