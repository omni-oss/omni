use std::{path::Path, sync::LazyLock};

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
    LazyLock::new(|| vec![".omnignore".to_string()]);

pub async fn discover<G: AsRef<str>>(
    root_dir: &Path,
    glob_patterns: &[G],
    sys: &impl GeneratorSys,
) -> Result<Vec<GeneratorConfiguration>, Error> {
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
            let mut conf = omni_file_data_serde::read_async::<
                GeneratorConfiguration,
                _,
                _,
            >(file.as_path(), &sys)
            .await?;

            conf.file = file;

            Ok::<_, Error>(conf)
        });
    }

    let mut configs = vec![];

    for result in results.join_all().await {
        configs.push(result?);
    }

    Ok(configs)
}
