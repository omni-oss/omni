use std::path::Path;

use serde::de::DeserializeOwned;
use system_traits::FsReadAsync;

pub async fn load_config<
    'a,
    'b,
    TConfig,
    TPath,
    TSys: FsReadAsync + Send + Sync,
>(
    path: TPath,
    sys: TSys,
) -> eyre::Result<TConfig>
where
    TConfig: DeserializeOwned,
    TPath: Into<&'a Path>,
{
    let path: &'a Path = path.into();
    let ext = path.extension().unwrap_or_default();
    let content = sys.fs_read_to_string_async(path).await?;

    match ext.to_string_lossy().as_ref() {
        "yaml" | "yml" => Ok(serde_yml::from_str(&content)?),
        "json" => Ok(serde_json::from_str(&content)?),
        "toml" => Ok(toml::from_str(&content)?),
        _ => {
            eyre::bail!(
                "Unsupported file extension for project configuration file {:?}",
                path
            )
        }
    }
}

pub fn is_valid_config_extension(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("yaml") | Some("yml") | Some("json") | Some("toml")
    )
}
