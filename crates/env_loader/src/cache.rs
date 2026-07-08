use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use maps::Map;
use mockall::automock;
use system_traits::{FsCanonicalize, auto_impl};

#[auto_impl]
pub trait EnvCacheSys: FsCanonicalize {}

#[automock]
pub trait EnvCache {
    fn is_cached(&self, path: &Path) -> bool;
    fn get(&self, path: &Path) -> Option<Arc<Map<String, String>>>;
    fn clear(&mut self, path: &Path);
    fn clear_all(&mut self);
    fn insert(&mut self, path: PathBuf, vars: Option<Arc<Map<String, String>>>);
}

pub trait EnvCacheExt: EnvCache {
    fn insert_none(&mut self, path: PathBuf) {
        self.insert(path, None);
    }

    fn insert_shared(
        &mut self,
        path: PathBuf,
        value: Arc<Map<String, String>>,
    ) {
        self.insert(path, Some(value));
    }

    fn insert_value(&mut self, path: PathBuf, value: Map<String, String>) {
        self.insert(path, Some(Arc::new(value)));
    }
}

impl<TCache: EnvCache> EnvCacheExt for TCache {}

#[derive(Clone, Debug, Default)]
pub struct DefaultEnvCache<TSys: EnvCacheSys> {
    inner: Map<PathBuf, Option<Arc<Map<String, String>>>>,
    /// Memoized `path -> canonicalized path` mappings.
    ///
    /// Canonicalizing a path is a `readlink`-based syscall, and the same
    /// directories are looked up repeatedly across projects (every project
    /// walks up to the shared workspace root). Since the filesystem layout is
    /// stable for the lifetime of a cache, we memoize the results to avoid
    /// re-issuing the syscall for paths we've already normalized.
    canonical: Map<PathBuf, PathBuf>,
    sys: TSys,
}

impl<TSys: EnvCacheSys> DefaultEnvCache<TSys> {
    pub fn new(sys: TSys) -> Self {
        Self {
            inner: maps::map!(),
            canonical: maps::map!(),
            sys,
        }
    }

    /// Canonicalize `path`, reusing a previously memoized result when possible.
    ///
    /// This takes `&self`, so it can be used from read-only cache operations.
    /// On a memo miss it falls back to a live canonicalization but does not
    /// store the result (that only happens through the mutating cache paths).
    fn canonical_key(&self, path: &Path) -> PathBuf {
        if let Some(canonical) = self.canonical.get(path) {
            return canonical.clone();
        }

        canonicalize(path, &self.sys)
    }

    /// Canonicalize `path`, memoizing the result for future lookups.
    fn canonical_key_memoized(&mut self, path: &Path) -> PathBuf {
        if let Some(canonical) = self.canonical.get(path) {
            return canonical.clone();
        }

        let canonical = canonicalize(path, &self.sys);
        self.canonical.insert(path.to_path_buf(), canonical.clone());
        canonical
    }
}

/// Two caches are considered equal when they hold the same data and system;
/// the canonicalization memo is a pure-function cache and is intentionally
/// excluded from equality.
impl<TSys: EnvCacheSys + PartialEq> PartialEq for DefaultEnvCache<TSys> {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner && self.sys == other.sys
    }
}

impl<TSys: EnvCacheSys + Eq> Eq for DefaultEnvCache<TSys> {}

impl<TSys: EnvCacheSys> EnvCache for DefaultEnvCache<TSys> {
    fn insert(
        &mut self,
        path: PathBuf,
        vars: Option<Arc<Map<String, String>>>,
    ) {
        let key = self.canonical_key_memoized(&path);

        self.inner.insert(key, vars);
    }

    fn is_cached(&self, path: &Path) -> bool {
        let k = self.canonical_key(path);
        let value = if let Some(value) = self.inner.get(&k) {
            value
        } else {
            return false;
        };

        if value.is_some() {
            return true;
        } else {
            for p in path.ancestors() {
                let key = self.canonical_key(p);

                match self.inner.get(&key) {
                    Some(Some(_)) => return true,
                    Some(None) => {
                        // Continue to the next ancestor, this just means that it was processed but no new vars were added
                    }
                    None => return false,
                }
            }
        }

        false
    }

    fn get(&self, path: &Path) -> Option<Arc<Map<String, String>>> {
        let k = self.canonical_key(path);
        let value = self.inner.get(&k)?;

        if let Some(value) = value {
            return Some(value.clone());
        } else {
            for p in path.ancestors() {
                let key = self.canonical_key(p);

                match self.inner.get(&key) {
                    Some(Some(value)) => return Some(value.clone()),
                    Some(None) => {
                        // Continue to the next ancestor, this just means that it was processed but no new vars were added
                    }
                    None => return None,
                }
            }
        }

        None
    }

    fn clear(&mut self, path: &Path) {
        let k = self.canonical_key(path);
        self.inner.swap_remove(&k);
    }

    fn clear_all(&mut self) {
        self.inner.clear();
    }
}

fn canonicalize(path: &Path, sys: &impl EnvCacheSys) -> PathBuf {
    sys.fs_canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use system_traits::impls::InMemorySys;

    use super::*;
    use crate::env;

    fn create_sys() -> InMemorySys {
        InMemorySys::default()
    }

    #[test]
    fn test_clear_all() {
        let sys = create_sys();
        let mut cache = DefaultEnvCache::new(sys);
        let key = Path::new("/root/nested/project");

        cache.insert_value(
            key.to_path_buf(),
            env![
                "ROOT_ENV" => "root",
            ],
        );

        cache.clear_all();

        assert!(!cache.is_cached(key), "Should not be cached");
    }

    #[test]
    fn test_clear() {
        let sys = create_sys();
        let mut cache = DefaultEnvCache::new(sys);

        let key = Path::new("/root/nested/project");
        cache.insert_value(
            key.to_path_buf(),
            env![
                "ROOT_ENV" => "root",
            ],
        );
        let key2 = Path::new("/root/nested/project/another/child/project");
        cache
            .insert_value(key2.to_path_buf(), env!["PROJECT_ENV" => "project"]);

        cache.clear(key);

        assert!(!cache.is_cached(key,), "Should not be cached");
        assert!(cache.is_cached(key2,), "Should be cached");
    }

    #[test]
    fn test_is_cached() {
        let sys = create_sys();
        let mut cache = DefaultEnvCache::new(sys);

        let key = Path::new("/root/nested/project");
        cache.insert_value(
            key.to_path_buf(),
            env![
                "ROOT_ENV" => "root",
            ],
        );

        assert!(cache.is_cached(key), "Should be cached");
    }

    #[test]
    fn test_get() {
        let sys = create_sys();
        let mut cache = DefaultEnvCache::new(sys);

        let val = env![
            "ROOT_ENV" => "root",
            "ROOT_LOCAL_ENV" => "root-local",
            "EMPTY_ENV" => "",
            "NESTED_ENV" => "nested",
            "NESTED_LOCAL_ENV" => "nested-local",
            "PROJECT_ENV" => "project",
            "PROJECT_LOCAL_ENV" => "project-local",
            "SHARED_ENV" => "root-local-nested-local-project-local",
        ];

        let key = Path::new("/root/nested/project");
        cache.insert_value(key.to_path_buf(), val.clone());

        assert_eq!(
            *cache.get(key).expect("should be some"),
            val,
            "Should be cached"
        );
    }

    #[test]
    fn test_get_with_parent() {
        let sys = create_sys();
        let mut cache = DefaultEnvCache::new(sys);

        let val = env![
            "ROOT_ENV" => "root",
            "ROOT_LOCAL_ENV" => "root-local",
            "EMPTY_ENV" => "",
            "NESTED_ENV" => "nested",
            "NESTED_LOCAL_ENV" => "nested-local",
            "PROJECT_ENV" => "project",
            "PROJECT_LOCAL_ENV" => "project-local",
            "SHARED_ENV" => "root-local-nested-local-project-local",
        ];
        let parent_key = Path::new("/root/nested/project");

        cache.insert_value(parent_key.to_path_buf(), val.clone());

        // Insert intermediate parent
        cache.insert_none(PathBuf::from("/root/nested/project/another"));
        cache.insert_none(PathBuf::from("/root/nested/project/another/child"));

        // Insert child
        let child_key = Path::new("/root/nested/project/another/child/project");
        cache.insert_none(child_key.to_path_buf());

        assert_eq!(
            *cache.get(child_key).expect("should be some"),
            val,
            "Should be cached"
        );
    }
}
