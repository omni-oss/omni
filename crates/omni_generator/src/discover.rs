use std::{borrow::Cow, path::Path, sync::LazyLock};

use omni_configuration_discovery::ConfigurationDiscovery;
use omni_generator_configurations::GeneratorConfiguration;
use tokio::task::JoinSet;

use crate::{GeneratorSys, error::Error};

static CONFIG_FILE_NAMES: LazyLock<Vec<String>> = LazyLock::new(|| {
    vec![
        "generator.omni.yaml".to_string(),
        "generator.omni.yml".to_string(),
        "generator.omni.json".to_string(),
        "generator.omni.toml".to_string(),
    ]
});

static IGNORE_FILE_NAMES: LazyLock<Vec<String>> =
    LazyLock::new(|| vec![".omniignore".to_string()]);

pub async fn discover<G: AsRef<str>>(
    root_dir: &Path,
    glob_patterns: &[G],
    sys: &impl GeneratorSys,
) -> Result<Vec<Cow<'static, GeneratorConfiguration>>, Error> {
    let discovery = ConfigurationDiscovery::new(
        root_dir,
        glob_patterns,
        CONFIG_FILE_NAMES.as_slice(),
        IGNORE_FILE_NAMES.as_slice(),
        "generator",
    );

    let files = discovery.discover().await?;

    let mut results = JoinSet::new();

    for file in files {
        let sys = sys.clone();
        results.spawn(async move {
            let mut conf: GeneratorConfiguration =
                omni_file_data_serde::read_async(file.as_path(), &sys).await?;

            conf.config_path = file;

            Ok::<_, Error>(Cow::Owned(conf))
        });
    }

    let mut configs = Vec::with_capacity(results.len());

    for result in results.join_all().await {
        configs.push(result?);
    }

    Ok(configs)
}

pub async fn discover_one_in_dir<D: AsRef<Path>>(
    dir: D,
    sys: &impl GeneratorSys,
) -> Result<Option<GeneratorConfiguration>, Error> {
    let discovery = ConfigurationDiscovery::new(
        dir.as_ref(),
        CONFIG_FILE_NAMES.as_slice(),
        CONFIG_FILE_NAMES.as_slice(),
        IGNORE_FILE_NAMES.as_slice(),
        "generator",
    );

    let files = discovery.discover().await?;

    for file in files {
        if sys.fs_exists_no_err_async(&file).await {
            let mut conf: GeneratorConfiguration =
                omni_file_data_serde::read_async(file.as_path(), sys).await?;

            conf.config_path = file;
            return Ok(Some(conf));
        }
    }

    Ok(None)
}
