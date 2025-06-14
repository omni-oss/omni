use std::{
    collections::HashMap,
    env::current_dir,
    fs,
    path::{Path, absolute},
};

mod error;

pub use error::*;

pub struct EnvConfig<'a> {
    /// Marker for the root file which tells the loader will consider as the topmost dir
    /// to look for env files.
    pub root_file: Option<&'a str>,
    /// The deepest dir to start looking for env files. If not specified, the current dir will be used.
    pub start_dir: Option<&'a str>,
    /// The env files to load. Values from later files will override earlier ones.
    pub env_files: &'a [&'a str],
    pub matcher: Option<&'a HashMap<String, String>>,
    /// Extra env vars to use when parsing env files.
    pub extra_envs: Option<&'a HashMap<String, String>>,
}

pub fn load(
    config: &EnvConfig,
) -> Result<HashMap<String, String>, EnvLoaderError> {
    let cwd =
        current_dir().map_err(|_| EnvLoaderErrorRepr::CantLoadCurrentDir)?;

    let start_dir = config
        .start_dir
        .unwrap_or_else(|| cwd.to_str().expect("Can't convert cwd to str"));

    let start_dir = Path::new(start_dir);

    if !start_dir.exists() {
        return Err(EnvLoaderErrorRepr::PathDoesNotExist(
            start_dir.to_string_lossy().to_string(),
        )
        .into());
    }

    let start_dir =
        absolute(start_dir).expect("Can't convert start dir to absolute");

    // Walk up the current dir to find the root dir
    let abs_root_dir = config.root_file.map(|root_file| {
        for p in cwd.ancestors() {
            let f = p.join(root_file);

            if f.exists() && f.is_file() {
                return Some(
                    absolute(f.parent().expect("Can't get parent"))
                        .expect("Can't retrieve absolute path"),
                );
            }
        }

        None
    });

    if let Some(Some(root_dir)) = abs_root_dir.as_ref() {
        tracing::debug!("Root dir: {:?}", root_dir);
    }

    let mut files = vec![];
    for dir in start_dir.ancestors().collect::<Vec<_>>().into_iter() {
        tracing::debug!("Looking for env files in dir: {:?}", dir);
        for env_file in config.env_files {
            let env_file = dir.join(env_file);

            tracing::debug!("Checking env file: {:?}", env_file);

            if env_file.exists() && env_file.is_file() {
                files.push(env_file);
            }
        }

        let Some(Some(root_dir)) = abs_root_dir.as_ref() else {
            continue;
        };
        if dir == root_dir {
            tracing::debug!("Reached root dir");
            // We've reached topmost dir
            break;
        }
    }

    let mut env = HashMap::new();

    env.extend(config.extra_envs.unwrap_or(&HashMap::new()).clone());

    tracing::debug!("Loading env files: {:?}", files);

    for file in files.iter() {
        let contents = fs::read_to_string(file).map_err(|_| {
            EnvLoaderErrorRepr::CantReadFile(file.to_string_lossy().to_string())
        })?;

        let parsed = env::parse(
            &contents,
            &env::ParseConfig {
                expand: true,
                extra_envs: Some(&env),
            },
        )
        .map_err(EnvLoaderErrorRepr::ParseEnvError)?;

        if let Some(matcher) = &config.matcher {
            if matcher
                .iter()
                .all(|(k, v)| parsed.get(k).map(|s| s == v).unwrap_or(false))
            {
                continue;
            }
        }

        env.extend(parsed);
    }

    tracing::debug!("Loaded env vars: {:?}", env);

    Ok(env)
}
