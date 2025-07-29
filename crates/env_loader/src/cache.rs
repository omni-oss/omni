use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use mockall::automock;
use system_traits::{FsCanonicalize, auto_impl};

#[auto_impl]
pub trait EnvCacheSys: FsCanonicalize {}

#[automock]
pub trait EnvCache {
    fn is_cached(&self, path: &Path) -> bool;
    fn get<'a>(&'a self, path: &Path) -> Option<&'a HashMap<String, String>>;
    fn clear(&mut self, path: &Path);
    fn clear_all(&mut self);
    fn insert(&mut self, path: PathBuf, vars: Option<HashMap<String, String>>);
}

pub trait EnvCacheExt: EnvCache {
    fn insert_none(&mut self, path: PathBuf) {
        self.insert(path, None);
    }

    fn insert_value(&mut self, path: PathBuf, value: HashMap<String, String>) {
        self.insert(path, Some(value));
    }
}

impl<TCache: EnvCache> EnvCacheExt for TCache {}

#[derive(Clone, PartialEq, Eq, Debug, Default)]

pub struct DefaultEnvCache<TSys: EnvCacheSys> {
    inner: HashMap<PathBuf, Option<HashMap<String, String>>>,
    sys: TSys,
}

impl<TSys: EnvCacheSys> DefaultEnvCache<TSys> {
    pub fn new(sys: TSys) -> Self {
        Self {
            inner: HashMap::new(),
            sys,
        }
    }
}

impl<TSys: EnvCacheSys> EnvCache for DefaultEnvCache<TSys> {
    fn insert(&mut self, path: PathBuf, vars: Option<HashMap<String, String>>) {
        let key = key(path, &self.sys);

        self.inner.insert(key, vars);
    }

    fn is_cached(&self, path: &Path) -> bool {
        let k = key(path.to_path_buf(), &self.sys);
        let value = if let Some(value) = self.inner.get(&k) {
            value
        } else {
            return false;
        };

        if value.is_some() {
            return true;
        } else {
            for p in path.ancestors() {
                let key = key(p.to_path_buf(), &self.sys);

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

    fn get(&self, path: &Path) -> Option<&HashMap<String, String>> {
        let k = key(path.to_path_buf(), &self.sys);
        let value = self.inner.get(&k)?;

        if let Some(value) = value {
            return Some(value);
        } else {
            for p in path.ancestors() {
                let key = key(p.to_path_buf(), &self.sys);

                match self.inner.get(&key) {
                    Some(Some(value)) => return Some(value),
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
        let k = key(path.to_path_buf(), &self.sys);
        self.inner.remove(&k);
    }

    fn clear_all(&mut self) {
        self.inner.clear();
    }
}

fn key(path: PathBuf, sys: &impl EnvCacheSys) -> PathBuf {
    sys.fs_canonicalize(&path).unwrap_or(path)
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

        cache.insert(
            key.to_path_buf(),
            Some(env![
                "ROOT_ENV" => "root",
            ]),
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
        cache.insert(
            key.to_path_buf(),
            Some(env![
                "ROOT_ENV" => "root",
            ]),
        );

        assert!(cache.is_cached(key,), "Should be cached");
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

        assert_eq!(cache.get(key,), Some(&val), "Should be cached");
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

        assert_eq!(cache.get(child_key,), Some(&val), "Should be cached");
    }
}
