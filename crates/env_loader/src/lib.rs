use std::path::{Path, PathBuf};

mod cache;
mod error;
mod sys;

#[cfg(test)]
pub mod test_utils;

pub use cache::*;
pub use error::*;
use maps::Map;
pub use sys::*;

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct EnvConfig<'a> {
    /// Marker for the root file which tells the loader will consider as the topmost dir
    /// to look for env files.
    pub root_file: Option<&'a Path>,
    /// The deepest dir to start looking for env files. If not specified, the current dir will be used.
    pub start_dir: Option<&'a Path>,
    /// The env files to load. Values from later files will override earlier ones.
    pub env_files: Option<&'a [&'a Path]>,
    /// Filter out env files that don't have the specified env vars.
    pub matcher: Option<&'a Map<String, String>>,
    /// Extra env vars to use when parsing env files.
    pub extra_envs: Option<&'a Map<String, String>>,
}

pub fn load<'a, TSys: EnvLoaderSys>(
    config: &'a EnvConfig<'a>,
    sys: TSys,
) -> Result<Map<String, String>, EnvLoaderError> {
    load_internal::<TSys, DefaultEnvCache<TSys>>(config, sys, None)
}

pub fn load_with_caching<'a, TSys, TCache>(
    config: &'a EnvConfig<'a>,
    sys: TSys,
    mut cache: &mut TCache,
) -> Result<Map<String, String>, EnvLoaderError>
where
    TSys: EnvLoaderSys,
    TCache: EnvCache,
{
    load_internal::<TSys, TCache>(config, sys, Some(&mut cache))
}

fn load_internal<'a, TSys, TCache>(
    config: &'a EnvConfig<'a>,
    sys: TSys,
    mut cache: Option<&mut TCache>,
) -> Result<Map<String, String>, EnvLoaderError>
where
    TSys: EnvLoaderSys,
    TCache: EnvCache,
{
    let cwd = sys
        .env_current_dir()
        .map_err(|_| EnvLoaderErrorInner::CantLoadCurrentDir)?;
    let start_dir = config.start_dir.unwrap_or(&cwd);

    if let Some(cached_ref) = cache.as_mut()
        && let Some(vars) = cached_ref.get(start_dir)
    {
        trace::debug!("Cache hit for start dir: {:?}", start_dir);
        let vars = vars.clone();
        return Ok(vars);
    }

    if !sys.fs_exists(start_dir)? {
        return Err(EnvLoaderErrorInner::PathDoesNotExist(
            start_dir.to_string_lossy().to_string(),
        )
        .into());
    }

    let start_dir = sys.fs_canonicalize(start_dir)?;

    // Walk up the current dir to find the root dir
    let abs_root_dir = config
        .root_file
        .map(|root_file| {
            for p in cwd.ancestors() {
                let root_file: PathBuf = root_file.into();
                let f = p.join(root_file);

                if f.exists() && f.is_file() {
                    return sys
                        .fs_canonicalize(f.parent().expect("Can't get parent"))
                        .expect("Can't retrieve absolute path");
                }
            }

            PathBuf::from("/")
        })
        .unwrap_or_else(|| PathBuf::from("/"));

    trace::debug!("Root dir: {:?}", abs_root_dir);

    let mut files = vec![];
    let default_envs = [Path::new(".env")];
    let env_files = config.env_files.unwrap_or(&default_envs);

    let mut env = maps::map!();

    env.extend(config.extra_envs.unwrap_or(&maps::map!()).clone());

    for dir in start_dir.ancestors().collect::<Vec<_>>() {
        trace::debug!("Looking for env files in dir: {:?}", dir);
        // If we've already processed this dir, don't process it again
        if let Some(cache_ref) = cache.as_mut()
            && let Some(vars) = cache_ref.get(dir)
        {
            trace::debug!("Cache hit for dir: {:?}, setting env vars", dir);

            env = vars.clone();
            break;
        }

        let mut to_process = vec![];
        for env_file in env_files.iter().rev() {
            let env_file = dir.join(env_file);

            trace::debug!("Checking env file: {:?}", env_file);

            if sys.fs_exists(&env_file)? && sys.fs_is_file(&env_file)? {
                to_process.push(env_file);
            }
        }

        files.push((dir, to_process));

        if dir == abs_root_dir {
            trace::debug!("Reached root dir");
            // We've reached topmost dir
            break;
        }
    }

    trace::debug!("Loading env files: {:#?}", files);

    // Process it in reverse order so that we can process the files in the same order as they were specified
    for (dir, files) in files.iter().rev() {
        trace::debug!("Processing dir: {:?}", dir);

        if files.is_empty() {
            if let Some(cache) = cache.as_mut() {
                if *dir == abs_root_dir {
                    cache.insert_value(dir.to_path_buf(), env.clone());
                } else {
                    cache.insert_none(dir.to_path_buf());
                }
            }

            continue;
        }

        if let Some(cache) = cache.as_mut()
            && let Some(vars) = cache.get(dir)
        {
            trace::debug!("Cache hit for dir: {:?}", dir);
            env.extend(vars.clone());
            continue;
        }

        for file in files.iter().rev() {
            let contents = sys.fs_read_to_string(file).map_err(|_| {
                EnvLoaderErrorInner::CantReadFile(
                    file.to_string_lossy().to_string(),
                )
            })?;

            let parsed = env::parse(
                &contents,
                &env::ParseConfig {
                    expand: true,
                    extra_envs: Some(&env),
                },
            )
            .map_err(EnvLoaderErrorInner::CantParseEnv)?;

            if let Some(matcher) = &config.matcher
                && matcher.iter().all(|(k, v)| {
                    parsed.get(k).map(|s| s == v).unwrap_or(false)
                })
            {
                continue;
            }
            env.extend(parsed);
        }
        if let Some(cache) = cache.as_mut() {
            cache.insert_value(dir.to_path_buf(), env.clone());
        }
    }

    // Commented out for now, as it's very slow
    // trace::debug!("Loaded env vars: {:?}", env);

    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::test_utils::create_sys;
    use super::*;

    #[test]
    fn test_load_order() {
        let config = EnvConfig {
            env_files: Some(&[Path::new(".env"), Path::new(".env.local")]),
            extra_envs: None,
            matcher: None,
            root_file: Some(Path::new("/root")),
            start_dir: Some(Path::new("/root/nested/project")),
            ..Default::default()
        };

        let sys = create_sys();
        let env = load(&config, sys).expect("Can't load env");

        assert_eq!(env.get("ROOT_ENV").map(|s| s.as_str()), Some("root"));
        assert_eq!(
            env.get("ROOT_LOCAL_ENV").map(|s| s.as_str()),
            Some("root-local")
        );
        assert_eq!(env.get("EMPTY_ENV").map(|s| s.as_str()), Some(""));
        assert_eq!(env.get("NESTED_ENV").map(|s| s.as_str()), Some("nested"));
        assert_eq!(
            env.get("NESTED_LOCAL_ENV").map(|s| s.as_str()),
            Some("nested-local")
        );
        assert_eq!(env.get("PROJECT_ENV").map(|s| s.as_str()), Some("project"));
        assert_eq!(
            env.get("PROJECT_LOCAL_ENV").map(|s| s.as_str()),
            Some("project-local")
        );
        assert_eq!(
            env.get("SHARED_ENV").map(|s| s.as_str()),
            Some("root-local-nested-local-project-local")
        );
    }
}
