//! Runtime-authoritative import-closure computation for confined generator spawns.
//!
//! [`scan_closure`] calls the bridge service's `/import-scan` RPC endpoint
//! (implemented by `ImportScan` in `bridge-rpc-services`) over an **unconfined**
//! runner and returns the flat set of paths the confined child needs read access
//! to. [`ClosureCache`] wraps it with a manifest-hash cache so repeated calls
//! for the same generator are cheap.

use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bridge_rpc_runner::BridgeServiceRunner;
use serde::{Deserialize, Serialize};

use crate::error::Error;

/// RPC path for the bridge service's import-scan endpoint.
pub const IMPORT_SCAN_PATH: &str = "/import-scan";

// ── wire types ───────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ScanRequest<'a> {
    entries: &'a [String],
}

#[derive(Deserialize)]
struct ScanResponse {
    closure: Vec<String>,
    #[serde(rename = "packageRoots", default)]
    package_roots: Vec<String>,
    #[serde(default)]
    diagnostics: Vec<String>,
}

// ── public types ─────────────────────────────────────────────────────────────

/// The flat, deduplicated set of paths the confined child needs read access to,
/// plus any non-fatal diagnostics the harness produced (e.g. unresolved
/// specifiers, non-literal dynamic imports).
#[derive(Debug, Clone, Default)]
pub struct ImportClosure {
    /// Sorted, canonical paths (first-party files union package roots).
    pub paths: Vec<PathBuf>,
    pub diagnostics: Vec<String>,
}

// ── scan ───────────────────────────────────────────────────────────────────

/// Compute the runtime-authoritative import closure of `entries` by invoking the
/// bridge service's `/import-scan` endpoint over `runner`.
///
/// `runner` must be an **unconfined** bridge-service process for the same
/// runtime the generator will run on: the harness drives that runtime's own
/// resolver (or `deno info`) and reads the scanned files directly, executing
/// none of them.
#[allow(clippy::result_large_err)]
pub async fn scan_closure(
    runner: &BridgeServiceRunner,
    entries: &[PathBuf],
) -> Result<ImportClosure, Error> {
    let entry_strings: Vec<String> = entries
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();

    let body = runner
        .call(
            IMPORT_SCAN_PATH,
            &ScanRequest {
                entries: &entry_strings,
            },
        )
        .await
        .map_err(|e| Error::custom(e.to_string()))?;

    let response: ScanResponse =
        serde_json::from_slice(&body).map_err(|e| {
            Error::custom(format!("invalid import-scan response: {e}"))
        })?;

    // Union the first-party closure and the package-root boundaries into one
    // sorted, deduplicated grant set.
    let mut paths: BTreeSet<PathBuf> = BTreeSet::new();
    for p in response.closure.into_iter().chain(response.package_roots) {
        paths.insert(PathBuf::from(p));
    }

    Ok(ImportClosure {
        paths: paths.into_iter().collect(),
        diagnostics: response.diagnostics,
    })
}

// ── cache ──────────────────────────────────────────────────────────────────

/// A hash over the *contents* of the governing manifests
/// (`package.json`/`deno.json`/`tsconfig.json`/lockfile) that decide how a
/// generator's specifiers resolve. When any of them changes, the hash changes
/// and the cached closure for that entry set is invalidated.
///
/// The hash need only be stable within a single process (the cache is
/// in-memory, never persisted), so the standard-library hasher is sufficient.
/// An absent manifest folds in a fixed marker so adding/removing one still
/// flips the hash.
pub fn manifest_hash(manifests: &[PathBuf]) -> u64 {
    let mut sorted: Vec<&PathBuf> = manifests.iter().collect();
    sorted.sort();
    sorted.dedup();

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for manifest in sorted {
        manifest.hash(&mut hasher);
        match std::fs::read(manifest) {
            Ok(content) => {
                1u8.hash(&mut hasher);
                content.hash(&mut hasher);
            }
            Err(_) => 0u8.hash(&mut hasher),
        }
    }
    hasher.finish()
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    entries: Vec<String>,
    manifest_hash: u64,
}

impl CacheKey {
    fn new(entries: &[PathBuf], manifest_hash: u64) -> Self {
        let mut entries: Vec<String> = entries
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        entries.sort();
        Self {
            entries,
            manifest_hash,
        }
    }
}

/// An in-memory cache of computed import closures, keyed by the entry set plus a
/// [`manifest_hash`]. Editing a governing manifest flips the key, so the next
/// lookup misses and recomputes.
#[derive(Debug, Default)]
pub struct ClosureCache {
    inner: Mutex<std::collections::HashMap<CacheKey, ImportClosure>>,
}

impl ClosureCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the cached closure for `entries` under the current `manifests`
    /// contents, if present.
    pub fn get(
        &self,
        entries: &[PathBuf],
        manifests: &[PathBuf],
    ) -> Option<ImportClosure> {
        let key = CacheKey::new(entries, manifest_hash(manifests));
        let guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        guard.get(&key).cloned()
    }

    /// Store `closure` for `entries` under the current `manifests` contents.
    pub fn insert(
        &self,
        entries: &[PathBuf],
        manifests: &[PathBuf],
        closure: ImportClosure,
    ) {
        let key = CacheKey::new(entries, manifest_hash(manifests));
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        guard.insert(key, closure);
    }

    /// Return the cached closure for `entries`, or compute it with `f`, store it,
    /// and return it. `f` is only invoked on a miss.
    #[allow(clippy::result_large_err)]
    pub async fn get_or_compute<F, Fut>(
        &self,
        entries: &[PathBuf],
        manifests: &[PathBuf],
        f: F,
    ) -> Result<ImportClosure, Error>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<ImportClosure, Error>>,
    {
        if let Some(hit) = self.get(entries, manifests) {
            return Ok(hit);
        }
        let computed = f().await?;
        self.insert(entries, manifests, computed.clone());
        Ok(computed)
    }
}

/// Best-effort list of the manifests near `entry` that govern its resolution,
/// used as the cache's invalidation inputs. Walks up from the entry to
/// `workspace_root` (inclusive), collecting any `package.json`, `deno.json(c)`,
/// `tsconfig.json`, and lockfiles it finds.
pub fn governing_manifests(
    entry: &Path,
    workspace_root: &Path,
) -> Vec<PathBuf> {
    const NAMES: &[&str] = &[
        "package.json",
        "deno.json",
        "deno.jsonc",
        "tsconfig.json",
        "package-lock.json",
        "pnpm-lock.yaml",
        "bun.lock",
        "bun.lockb",
        "deno.lock",
    ];
    let mut out = Vec::new();
    let mut dir = entry.parent();
    while let Some(current) = dir {
        for name in NAMES {
            let candidate = current.join(name);
            if candidate.is_file() {
                out.push(candidate);
            }
        }
        if current == workspace_root {
            break;
        }
        dir = current.parent();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closure_of(paths: &[&str]) -> ImportClosure {
        ImportClosure {
            paths: paths.iter().map(PathBuf::from).collect(),
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn manifest_hash_is_stable_for_identical_contents() {
        let dir = tempfile::tempdir().unwrap();
        let m = dir.path().join("package.json");
        std::fs::write(&m, r#"{"name":"a"}"#).unwrap();
        let manifests = vec![m];
        assert_eq!(manifest_hash(&manifests), manifest_hash(&manifests));
    }

    #[test]
    fn manifest_hash_changes_when_a_manifest_is_edited() {
        let dir = tempfile::tempdir().unwrap();
        let m = dir.path().join("package.json");
        std::fs::write(&m, r#"{"name":"a"}"#).unwrap();
        let manifests = vec![m.clone()];
        let before = manifest_hash(&manifests);
        std::fs::write(&m, r#"{"name":"a","deps":1}"#).unwrap();
        assert_ne!(
            before,
            manifest_hash(&manifests),
            "edit must flip the hash"
        );
    }

    #[test]
    fn manifest_hash_distinguishes_absent_from_present() {
        let dir = tempfile::tempdir().unwrap();
        let m = dir.path().join("package.json");
        let manifests = vec![m.clone()];
        let absent = manifest_hash(&manifests);
        std::fs::write(&m, "{}").unwrap();
        assert_ne!(absent, manifest_hash(&manifests));
    }

    #[test]
    fn cache_hits_on_identical_inputs_and_misses_after_a_manifest_edit() {
        let dir = tempfile::tempdir().unwrap();
        let m = dir.path().join("package.json");
        std::fs::write(&m, r#"{"name":"a"}"#).unwrap();
        let entries = vec![dir.path().join("gen.ts")];
        let manifests = vec![m.clone()];

        let cache = ClosureCache::new();
        assert!(cache.get(&entries, &manifests).is_none(), "cold miss");

        cache.insert(&entries, &manifests, closure_of(&["/a"]));
        let hit = cache.get(&entries, &manifests).expect("warm hit");
        assert_eq!(hit.paths, vec![PathBuf::from("/a")]);

        // Editing a governing manifest invalidates the entry.
        std::fs::write(&m, r#"{"name":"a","changed":true}"#).unwrap();
        assert!(
            cache.get(&entries, &manifests).is_none(),
            "a manifest edit must miss"
        );
    }

    #[tokio::test]
    async fn get_or_compute_computes_once_then_caches() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![dir.path().join("gen.ts")];
        let manifests: Vec<PathBuf> = vec![];
        let cache = ClosureCache::new();
        let calls = std::sync::atomic::AtomicUsize::new(0);

        let compute = || async {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(closure_of(&["/x"]))
        };

        let first = cache
            .get_or_compute(&entries, &manifests, compute)
            .await
            .unwrap();
        assert_eq!(first.paths, vec![PathBuf::from("/x")]);
        let second = cache
            .get_or_compute(&entries, &manifests, || async {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(closure_of(&["/should-not-run"]))
            })
            .await
            .unwrap();
        assert_eq!(
            second.paths,
            vec![PathBuf::from("/x")],
            "served from cache"
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
