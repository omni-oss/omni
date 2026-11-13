//! The extension point for remote-source kinds. A *kind* is the id string
//! (`"git"`) used as a store path segment and a lockfile key; a *driver* is the
//! [`SourceDriver`] implementation for one kind. The store core owns locking,
//! publishing, manifests, pins and prune; a driver only resolves a pin and
//! exports its files.
//!
//! Unstable until the registry RFC settles credentials, offline policy and
//! per-pin metadata.

use std::{collections::BTreeMap, fmt, path::Path, path::PathBuf, sync::Arc};

use crate::{error::Error, sys::RemoteSourceSys};

/// The address of a source as configured, credentials included. Held in
/// memory only: it has no `Serialize`, and `Debug`/`Display` redact userinfo.
#[derive(Clone, PartialEq, Eq)]
pub struct Connection(String);

impl Connection {
    pub fn new(address: impl Into<String>) -> Self {
        Self(address.into())
    }

    /// The raw address, for connecting only. Never log or persist it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Connection({})",
            omni_redact::redact_url_userinfo(&self.0)
        )
    }
}

impl fmt::Display for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&omni_redact::redact_url_userinfo(&self.0))
    }
}

/// Kind-neutral address of one source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSpec {
    /// Stable lockfile/refs key for the place the source lives, in canonical
    /// form and without credentials.
    pub locator: String,
    /// What the user asked for at that place (git: a rev).
    pub selector: String,
    /// How to reach it, credentials included. Drivers connect with this, never
    /// with `locator`.
    pub connection: Connection,
}

/// Driver settings. Empty today; the registry RFC fills it in.
#[derive(Debug, Clone, Default)]
pub struct DriverConfig {}

pub struct DriverContext<'a, TSys> {
    pub sys: &'a TSys,
    /// `<store>/raw/<kind>/<slug>/`, written by the driver. Not created by the
    /// core, and deleted only by prune.
    pub raw_dir: PathBuf,
    /// No network allowed.
    pub offline: bool,
    pub config: &'a DriverConfig,
}

#[async_trait::async_trait]
pub trait SourceDriver<TSys: RemoteSourceSys>: Send + Sync {
    /// Path segment and lockfile/refs key, e.g. `"git"`. Must be unique,
    /// stable and a safe segment.
    fn kind(&self) -> &'static str;

    /// One path segment naming the place. Must be injective over
    /// `spec.locator`; the core hashes it when it is not a safe segment.
    fn slug(&self, spec: &SourceSpec) -> String;

    /// Turn `spec` (and the locked pin, if any) into a pin, fetching into
    /// `ctx.raw_dir` when needed. Called with the per-remote lock held. When
    /// `locked` is in the raw cache already, must not use the network.
    async fn resolve(
        &self,
        ctx: &DriverContext<'_, TSys>,
        spec: &SourceSpec,
        locked: Option<&str>,
    ) -> Result<String, Error>;

    /// Write the plain files of `pin` into `dest`, an empty directory. Must not
    /// touch anything outside `dest` and `ctx.raw_dir`.
    async fn export(
        &self,
        ctx: &DriverContext<'_, TSys>,
        spec: &SourceSpec,
        pin: &str,
        dest: &Path,
    ) -> Result<(), Error>;
}

/// The drivers a manager knows, keyed by kind. Also the set of kinds a
/// lockfile or refs file may contain.
pub struct DriverRegistry<TSys: RemoteSourceSys> {
    drivers: BTreeMap<&'static str, Arc<dyn SourceDriver<TSys>>>,
}

impl<TSys: RemoteSourceSys> Default for DriverRegistry<TSys> {
    fn default() -> Self {
        Self {
            drivers: BTreeMap::new(),
        }
    }
}

impl<TSys: RemoteSourceSys> DriverRegistry<TSys> {
    /// The built-in drivers.
    pub fn builtin() -> Self {
        let mut registry = Self::default();
        registry.register(Arc::new(crate::git::GitDriver));
        registry
    }

    /// Add a driver. Panics on a duplicate or unsafe kind: both are programming
    /// errors.
    pub fn register(&mut self, driver: Arc<dyn SourceDriver<TSys>>) {
        let kind = driver.kind();
        assert!(is_safe_segment(kind), "unsafe driver kind {kind:?}");
        assert!(
            self.drivers.insert(kind, driver).is_none(),
            "driver kind {kind:?} registered twice"
        );
    }

    pub fn get(&self, kind: &str) -> Option<&Arc<dyn SourceDriver<TSys>>> {
        self.drivers.get(kind)
    }

    pub fn contains(&self, kind: &str) -> bool {
        self.drivers.contains_key(kind)
    }

    pub fn kinds(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.drivers.keys().copied()
    }
}

const MAX_SEGMENT_LEN: usize = 128;
const HASHED_SLUG_PREFIX: &str = "h-";

/// Whether `s` is safe as one store path segment: `[a-z0-9._-]`, 1 to 128
/// bytes, no leading `.`, and no suffix reserved for sibling markers.
pub fn is_safe_segment(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_SEGMENT_LEN
        && !s.starts_with('.')
        && !s.ends_with(".manifest")
        && !s.ends_with(".used")
        && s.bytes().all(|b| {
            b.is_ascii_lowercase()
                || b.is_ascii_digit()
                || matches!(b, b'.' | b'_' | b'-')
        })
}

/// The slug used on disk: the driver's slug when it is safe, otherwise
/// `h-<blake3 hex>`. Literal slugs starting with `h-` are hashed too, so a
/// hashed and a literal slug never collide.
pub(crate) fn safe_slug(slug: &str) -> String {
    if is_safe_segment(slug) && !slug.starts_with(HASHED_SLUG_PREFIX) {
        slug.to_string()
    } else {
        format!(
            "{HASHED_SLUG_PREFIX}{}",
            blake3::hash(slug.as_bytes()).to_hex()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_segments() {
        for ok in ["git", "c1", "a.b_c-d", &"a".repeat(128)] {
            assert!(is_safe_segment(ok), "{ok}");
        }
        for bad in [
            "",
            ".",
            "..",
            "../x",
            "a/b",
            "A1",
            ".hidden",
            "x.manifest",
            "x.used",
            "a b",
            &"a".repeat(129),
        ] {
            assert!(!is_safe_segment(bad), "{bad}");
        }
    }

    #[test]
    fn unsafe_and_h_prefixed_slugs_are_hashed() {
        assert_eq!(safe_slug("plain"), "plain");
        for s in ["UPPER", "h-abc", "a/b", &"a".repeat(200)] {
            let hashed = safe_slug(s);
            assert!(hashed.starts_with("h-"), "{s}");
            assert!(is_safe_segment(&hashed), "{s}");
        }
        assert_ne!(safe_slug("h-abc"), "h-abc");
    }

    #[test]
    fn connection_redacts_credentials() {
        let c = Connection::new("https://user:token@host/r.git");
        assert!(!format!("{c:?}").contains("token"));
        assert!(!format!("{c}").contains("token"));
        assert_eq!(c.expose(), "https://user:token@host/r.git");
    }
}
