use std::{collections::HashMap, path::Path};

mod error;
mod sys;

pub use error::*;
pub use sys::*;

pub struct EnvConfig<'a> {
    /// Marker for the root file which tells the loader will consider as the topmost dir
    /// to look for env files.
    pub root_file: Option<&'a str>,
    /// The deepest dir to start looking for env files. If not specified, the current dir will be used.
    pub start_dir: Option<&'a str>,
    /// The env files to load. Values from later files will override earlier ones.
    pub env_files: &'a [&'a str],
    /// Filter out env files that don't have the specified env vars.
    pub matcher: Option<&'a HashMap<String, String>>,
    /// Extra env vars to use when parsing env files.
    pub extra_envs: Option<&'a HashMap<String, String>>,
}

pub fn load<TSys>(
    config: &EnvConfig,
    sys: TSys,
) -> Result<HashMap<String, String>, EnvLoaderError>
where
    TSys: EnvLoaderSys,
{
    let cwd = sys
        .env_current_dir()
        .map_err(|_| EnvLoaderErrorRepr::CantLoadCurrentDir)?;

    let start_dir = config
        .start_dir
        .unwrap_or_else(|| cwd.to_str().expect("Can't convert cwd to str"));

    let start_dir = Path::new(start_dir);

    if !sys.fs_exists(start_dir)? {
        return Err(EnvLoaderErrorRepr::PathDoesNotExist(
            start_dir.to_string_lossy().to_string(),
        )
        .into());
    }

    let start_dir = sys.fs_canonicalize(start_dir)?;

    // Walk up the current dir to find the root dir
    let abs_root_dir = config.root_file.map(|root_file| {
        for p in cwd.ancestors() {
            let f = p.join(root_file);

            if f.exists() && f.is_file() {
                return Some(
                    sys.fs_canonicalize(f.parent().expect("Can't get parent"))
                        .expect("Can't retrieve absolute path"),
                );
            }
        }

        None
    });

    if let Some(Some(root_dir)) = abs_root_dir.as_ref() {
        trace::debug!("Root dir: {:?}", root_dir);
    }

    let mut files = vec![];
    for dir in start_dir.ancestors().collect::<Vec<_>>().into_iter() {
        trace::debug!("Looking for env files in dir: {:?}", dir);
        for env_file in config.env_files.iter().rev() {
            let env_file = dir.join(env_file);

            trace::debug!("Checking env file: {:?}", env_file);

            if sys.fs_exists(&env_file)? && sys.fs_is_file(&env_file)? {
                files.push(env_file);
            }
        }

        let Some(Some(root_dir)) = abs_root_dir.as_ref() else {
            continue;
        };
        if dir == root_dir {
            trace::debug!("Reached root dir");
            // We've reached topmost dir
            break;
        }
    }

    let mut env = HashMap::new();

    env.extend(config.extra_envs.unwrap_or(&HashMap::new()).clone());

    trace::debug!("Loading env files: {:?}", files);

    for file in files.iter().rev() {
        let contents = sys.fs_read_to_string(file).map_err(|_| {
            EnvLoaderErrorRepr::CantReadFile(file.to_string_lossy().to_string())
        })?;

        let parsed = env::parse(
            &contents,
            &env::ParseConfig {
                expand: true,
                extra_envs: Some(&env),
            },
        )
        .map_err(EnvLoaderErrorRepr::CantParseEnv)?;

        if let Some(matcher) = &config.matcher
            && matcher
                .iter()
                .all(|(k, v)| parsed.get(k).map(|s| s == v).unwrap_or(false))
        {
            continue;
        }

        env.extend(parsed);
    }

    trace::debug!("Loaded env vars: {:?}", env);

    Ok(env)
}

#[cfg(test)]
mod tests {
    use system_traits::{
        EnvSetCurrentDir, FsCreateDirAll, FsWrite, impls::InMemorySysSync,
    };

    use super::*;

    fn create_sys() -> impl EnvLoaderSys {
        let sys = InMemorySysSync::default();

        sys.fs_create_dir_all("/root/nested/project")
            .expect("Can't create root dir");

        sys.fs_write("/root/.env", include_str!("../test_fixtures/.env.root"))
            .expect("Can't write root env file");
        sys.fs_write(
            "/root/.env.local",
            include_str!("../test_fixtures/.env.root.local"),
        )
        .expect("Can't write root local env file");

        sys.fs_write(
            "/root/nested/.env",
            include_str!("../test_fixtures/.env.nested"),
        )
        .expect("Can't write nested env file");
        sys.fs_write(
            "/root/nested/.env.local",
            include_str!("../test_fixtures/.env.nested.local"),
        )
        .expect("Can't write nested local env file");
        sys.fs_write(
            "/root/nested/project/.env",
            include_str!("../test_fixtures/.env.project"),
        )
        .expect("Can't write project env file");
        sys.fs_write(
            "/root/nested/project/.env.local",
            include_str!("../test_fixtures/.env.project.local"),
        )
        .expect("Can't write project local env file");
        sys.env_set_current_dir("/root/nested/project")
            .expect("Can't set current dir");

        sys
    }

    #[test]
    fn test_load_order() {
        let config = EnvConfig {
            env_files: &[".env", ".env.local"],
            extra_envs: None,
            matcher: None,
            root_file: Some("/root"),
            start_dir: Some("/root/nested/project"),
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
