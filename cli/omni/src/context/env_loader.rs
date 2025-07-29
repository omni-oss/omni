use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use env_loader::{
    EnvCache as _, EnvCacheExt as _, EnvLoaderError, EnvLoaderSys,
};
use system_traits::{EnvCurrentDir, EnvVars, auto_impl};

use crate::utils::env::EnvVarsMap;

#[auto_impl]
pub trait EnvCacheSys: EnvLoaderSys + EnvVars + Clone + EnvCurrentDir {}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EnvLoader<T: EnvCacheSys> {
    env_cache: env_loader::DefaultEnvCache<T>,
    sys: T,
    root_dir_marker: PathBuf,
    env_files: Vec<PathBuf>,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct GetVarsArgs<'a> {
    pub start_dir: Option<&'a Path>,
    pub env_files: Option<&'a [&'a Path]>,
    pub override_vars: Option<&'a HashMap<String, String>>,
}

impl<T: EnvCacheSys> EnvLoader<T> {
    pub fn new(
        sys: T,
        root_dir_marker: PathBuf,
        env_files: Vec<PathBuf>,
    ) -> Self {
        Self {
            env_cache: env_loader::DefaultEnvCache::new(sys.clone()),
            sys,
            root_dir_marker,
            env_files,
        }
    }

    pub fn get_cached(&self, path: &Path) -> Option<&EnvVarsMap> {
        self.env_cache.get(path)
    }

    pub fn get(
        &mut self,
        args: &GetVarsArgs,
    ) -> Result<EnvVarsMap, EnvLoaderError> {
        let cwd = self.sys.env_current_dir()?;
        let start_dir = args.start_dir.unwrap_or(&cwd);

        if let Some(cached) = self.env_cache.get(start_dir) {
            let mut env = cached.clone();
            if let Some(override_vars) = args.override_vars {
                env.extend(override_vars.clone());
            }
            return Ok(env);
        }

        let v = self.sys.env_vars();

        let mut env_vars = HashMap::new();

        env_vars.extend(v);

        let env_files = args
            .env_files
            .map(|s| s.iter().map(Path::new).collect::<Vec<_>>())
            .unwrap_or_else(|| {
                self.env_files.iter().map(Path::new).collect::<Vec<_>>()
            });

        let config = env_loader::EnvConfig {
            root_file: Some(&self.root_dir_marker),
            start_dir: Some(start_dir),
            env_files: Some(&env_files),
            extra_envs: Some(&env_vars),
            matcher: None,
        };

        let mut env = env_loader::load_with_caching(
            &config,
            self.sys.clone(),
            Some(&mut self.env_cache),
        )?;

        if let Some(override_vars) = args.override_vars {
            env.extend(override_vars.clone());
            // replace the cache with the new env vars if there are overrides
            self.env_cache
                .insert_value(start_dir.to_path_buf(), env.clone());
        }

        Ok(env)
    }
}
