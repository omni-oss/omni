use std::path::{Path, PathBuf};

use env::expand_into;
use env_loader::{
    EnvCache as _, EnvCacheExt as _, EnvLoaderError, EnvLoaderSys,
};
use maps::Map;
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
    pub override_vars: Option<&'a Map<String, String>>,
    pub inherit_env_vars: bool,
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

        let mut env_vars = maps::map!();

        if args.inherit_env_vars {
            let existing_env_vars = self.sys.env_vars();
            env_vars.extend(existing_env_vars);
        }

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
            &mut self.env_cache,
        )?;

        if let Some(override_vars) = args.override_vars {
            let mut override_vars = override_vars.clone();
            expand_into(&mut override_vars, &env);

            env.extend(override_vars);

            // replace the cache with the new env vars if there are overrides
            self.env_cache
                .insert_value(start_dir.to_path_buf(), env.clone());
        }

        Ok(env)
    }
}
