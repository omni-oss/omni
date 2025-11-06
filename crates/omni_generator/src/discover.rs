use std::path::Path;

use omni_configuration_discovery::ConfigurationDiscovery;
use omni_generator_configurations::GeneratorConfiguration;
use tokio::task::JoinSet;

use crate::{GeneratorSys, error::Error};

pub async fn discover(
    root_dir: &Path,
    glob_patterns: &[String],
    sys: &impl GeneratorSys,
) -> Result<Vec<GeneratorConfiguration>, Error> {
    let config_file_names = [
        "generator.omni.yaml".to_string(),
        "generator.omni.yml".to_string(),
        "generator.omni.json".to_string(),
        "generator.omni.toml".to_string(),
    ];

    let ignore_filenames = [".omniignore".to_string()];

    let discovery = ConfigurationDiscovery::new(
        root_dir,
        glob_patterns,
        config_file_names.as_slice(),
        ignore_filenames.as_slice(),
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
