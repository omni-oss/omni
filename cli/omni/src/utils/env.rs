use std::{collections::HashMap, ffi::OsString};

use env_loader::{EnvLoaderError, EnvLoaderSys};
use system_traits::EnvVars;

pub type EnvVarsMap = HashMap<String, String>;
pub type EnvVarsMapOs = HashMap<OsString, OsString>;

pub fn get_envs_at_start_dir(
    start_dir: &str,
    env_root_dir_marker: &str,
    env_files: &[&str],
    sys: impl EnvLoaderSys + EnvVars + Clone,
) -> Result<(EnvVarsMap, EnvVarsMapOs), EnvLoaderError> {
    let v = sys.env_vars();

    let mut env_vars = HashMap::new();
    let mut env_vars_os = HashMap::new();

    env_vars.extend(v);

    let config = env_loader::EnvConfig {
        root_file: Some(env_root_dir_marker),
        start_dir: Some(start_dir),
        env_files,
        extra_envs: Some(&env_vars),
        matcher: None,
    };

    let env = env_loader::load(&config, sys.clone())?;
    let env_os = env
        .iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect::<HashMap<_, _>>();
    env_vars.extend(env);

    env_vars_os.extend(env_os);

    Ok((env_vars, env_vars_os))
}
