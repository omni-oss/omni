use std::{borrow::Cow, path::Path, sync::LazyLock};

use omni_configuration_discovery::ConfigurationDiscovery;
use omni_tool_configurations::ToolConfiguration;
use tokio::task::JoinSet;

use crate::{
    ToolSys,
    error::{Error, ErrorInner},
};

static CONFIG_FILE_NAMES: LazyLock<Vec<String>> = LazyLock::new(|| {
    vec![
        "tool.omni.yaml".to_string(),
        "tool.omni.yml".to_string(),
        "tool.omni.json".to_string(),
        "tool.omni.toml".to_string(),
    ]
});

static IGNORE_FILE_NAMES: LazyLock<Vec<String>> =
    LazyLock::new(|| vec![".omniignore".to_string()]);

/// Discover every `tool.omni.*` manifest under `root_dir` matching any of
/// `glob_patterns`, deserializing each into a [`ToolConfiguration`] with its
/// `config_path` set to the manifest's location.
pub async fn discover<G: AsRef<str>>(
    root_dir: &Path,
    glob_patterns: &[G],
    sys: &impl ToolSys,
) -> Result<Vec<Cow<'static, ToolConfiguration>>, Error> {
    let discovery = ConfigurationDiscovery::new(
        root_dir,
        glob_patterns,
        CONFIG_FILE_NAMES.as_slice(),
        IGNORE_FILE_NAMES.as_slice(),
        "tool",
    );

    let files = discovery.discover().await?;

    let mut results = JoinSet::new();

    for file in files {
        let sys = sys.clone();
        results.spawn(async move {
            let mut conf: ToolConfiguration =
                omni_file_data_serde::read_async(file.as_path(), &sys)
                    .await
                    .map_err(|e| ErrorInner::LoadConfig {
                        path: file.to_path_buf(),
                        inner: e,
                    })?;

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

#[cfg(test)]
mod tests {
    use std::fs;

    use system_traits::impls::RealSys;
    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    async fn discovers_tool_manifests_by_glob() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        let tool_a = root.join("tools/a");
        let tool_b = root.join("tools/b");
        fs::create_dir_all(&tool_a).unwrap();
        fs::create_dir_all(&tool_b).unwrap();

        fs::write(
            tool_a.join("tool.omni.yaml"),
            "type: js\nname: tool-a\nentrypoint: ./index.mjs\n",
        )
        .unwrap();
        fs::write(
            tool_b.join("tool.omni.yaml"),
            "type: js\nname: tool-b\nentrypoint: ./index.mjs\n",
        )
        .unwrap();
        // A non-manifest file must be ignored.
        fs::write(tool_a.join("index.mjs"), "export default () => {}").unwrap();

        let sys = RealSys::default();
        let mut names = discover(root, &["tools/**"], &sys)
            .await
            .expect("discovery succeeds")
            .into_iter()
            .map(|c| c.name.clone())
            .collect::<Vec<_>>();
        names.sort();

        assert_eq!(names, vec!["tool-a".to_string(), "tool-b".to_string()]);
    }

    #[tokio::test]
    async fn sets_config_path_on_discovered_manifest() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let tool_dir = root.join("tools/only");
        fs::create_dir_all(&tool_dir).unwrap();
        let manifest = tool_dir.join("tool.omni.yaml");
        fs::write(&manifest, "type: js\nname: only\nentrypoint: ./index.mjs\n")
            .unwrap();

        let sys = RealSys::default();
        let configs = discover(root, &["tools/**"], &sys).await.unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].config_path, manifest);
    }
}
