use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use env::expand_into;
use env_loader::{EnvCache as _, EnvCacheExt, EnvLoaderError, EnvLoaderSys};
use maps::Map;
use system_traits::{EnvCurrentDir, EnvVars, auto_impl};

use crate::utils::EnvVarsMap;

#[auto_impl]
pub trait EnvCacheSys: EnvLoaderSys + EnvVars + Clone + EnvCurrentDir {}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub project_env_var_overrides: Option<&'a Map<String, String>>,
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

    pub fn get_cached(&self, path: &Path) -> Option<Arc<EnvVarsMap>> {
        self.env_cache.get(path).clone()
    }

    pub fn get(
        &mut self,
        args: &GetVarsArgs,
    ) -> Result<Arc<EnvVarsMap>, EnvLoaderError> {
        let cwd = self.sys.env_current_dir()?;
        let start_dir = args.start_dir.unwrap_or(&cwd);

        if let Some(cached) = self.env_cache.get(start_dir) {
            if let Some(override_vars) = args.project_env_var_overrides {
                let mut env = (*cached).clone();
                env.extend(override_vars.clone());
                return Ok(Arc::new(env));
            } else {
                return Ok(cached);
            }
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

        if let Some(override_vars) = args.project_env_var_overrides {
            let mut env = (*env_loader::load_with_caching(
                &config,
                self.sys.clone(),
                &mut self.env_cache,
            )?)
            .clone();
            let mut override_vars = override_vars.clone();
            expand_into(&mut override_vars, &env);

            env.extend(override_vars);

            let shared_env = Arc::new(env);

            // replace the cache with the new env vars if there are overrides
            self.env_cache
                .insert_shared(start_dir.to_path_buf(), shared_env.clone());

            Ok(shared_env)
        } else {
            let env = env_loader::load_with_caching(
                &config,
                self.sys.clone(),
                &mut self.env_cache,
            )?;

            Ok(env)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use system_traits::EnvSetCurrentDir as _;

    use super::*;
    use crate::{EnvLoader, test_fixture::*};

    const ENV: &str = "testing";

    #[tokio::test]
    pub async fn test_load_env_vars() {
        let root = Path::new("/root");
        let sys = mem_sys();

        setup_fixture(root, sys.clone());

        sys.env_set_current_dir(root.join("nested").join("project-1"))
            .expect("Can't set current dir");

        let mut env_loader = EnvLoader::new(
            sys.clone(),
            PathBuf::from("workspace.omni.yaml"),
            vec![
                PathBuf::from(".env"),
                PathBuf::from(".env.local"),
                PathBuf::from(format!(".env.{ENV}")),
                PathBuf::from(format!(".env.{ENV}.local")),
            ],
        );

        let env = env_loader
            .get(&GetVarsArgs::default())
            .expect("Can't load env vars");

        assert_eq!(
            env.get("SHARED_ENV").map(String::as_str),
            Some("root-local-nested-local-project-local")
        );
    }
}
