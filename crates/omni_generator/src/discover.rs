use std::{borrow::Cow, path::Path, sync::LazyLock};

use omni_configuration_discovery::ConfigurationDiscovery;
use omni_generator_configurations::GeneratorConfiguration;
use tokio::task::JoinSet;

use crate::{
    GeneratorSys,
    error::{Error, ErrorInner},
};

static CONFIG_FILE_NAMES: LazyLock<Vec<String>> = LazyLock::new(|| {
    omni_constants::config_file_names(omni_constants::GENERATOR_OMNI)
});

static IGNORE_FILE_NAMES: LazyLock<Vec<String>> =
    LazyLock::new(|| vec![omni_constants::OMNI_IGNORE.to_string()]);

#[allow(clippy::result_large_err)]
pub async fn discover<G: AsRef<str>>(
    root_dir: &Path,
    glob_patterns: &[G],
    sys: &impl GeneratorSys,
) -> Result<Vec<Cow<'static, GeneratorConfiguration>>, Error> {
    discover_with(root_dir, glob_patterns, sys, true).await
}

/// Like [`discover`], for a self-contained source tree such as a fetched git
/// checkout. Ignore files in directories above `root_dir` are not applied, so
/// the enclosing workspace's rules (which hide the source store) cannot hide
/// the source's own generators. The source's own ignore files still apply.
#[allow(clippy::result_large_err)]
pub async fn discover_in_source<G: AsRef<str>>(
    root_dir: &Path,
    glob_patterns: &[G],
    sys: &impl GeneratorSys,
) -> Result<Vec<Cow<'static, GeneratorConfiguration>>, Error> {
    discover_with(root_dir, glob_patterns, sys, false).await
}

#[allow(clippy::result_large_err)]
async fn discover_with<G: AsRef<str>>(
    root_dir: &Path,
    glob_patterns: &[G],
    sys: &impl GeneratorSys,
    parent_ignores: bool,
) -> Result<Vec<Cow<'static, GeneratorConfiguration>>, Error> {
    let no_exclude: &[G] = &[];
    let discovery = ConfigurationDiscovery::new(
        root_dir,
        glob_patterns,
        no_exclude,
        CONFIG_FILE_NAMES.as_slice(),
        IGNORE_FILE_NAMES.as_slice(),
        "generator",
    )
    .with_parent_ignores(parent_ignores);

    let files = discovery.discover().await?;

    let mut results = JoinSet::new();

    for file in files {
        let sys = sys.clone();
        results.spawn(async move {
            let mut conf: GeneratorConfiguration =
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

#[allow(clippy::result_large_err)]
pub async fn discover_one_in_dir<D: AsRef<Path>>(
    dir: D,
    sys: &impl GeneratorSys,
) -> Result<Option<GeneratorConfiguration>, Error> {
    let no_exclude: &[String] = &[];
    let discovery = ConfigurationDiscovery::new(
        dir.as_ref(),
        CONFIG_FILE_NAMES.as_slice(),
        no_exclude,
        CONFIG_FILE_NAMES.as_slice(),
        IGNORE_FILE_NAMES.as_slice(),
        "generator",
    );

    let files = discovery.discover().await?;

    for file in files {
        if sys.fs_exists_no_err_async(&file).await {
            let mut conf: GeneratorConfiguration =
                omni_file_data_serde::read_async(file.as_path(), sys)
                    .await
                    .map_err(|e| ErrorInner::LoadConfig {
                        path: file.to_path_buf(),
                        inner: e,
                    })?;

            conf.config_path = file;
            return Ok(Some(conf));
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use system_traits::impls::RealSys;
    use tempfile::TempDir;

    use super::*;

    /// `<root>/store/source/<manifest>` with an ignore file in `<root>` hiding
    /// `store/`, as the synced `/.omni/sources/**` rule does. Returns the
    /// source dir.
    fn source_under_ignored_store(root: &Path) -> std::path::PathBuf {
        let source = root.join("store/source");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join("generator.omni.yaml"),
            "name: remote-gen\n",
        )
        .unwrap();
        std::fs::write(root.join(".omniignore"), "/store/**\n").unwrap();
        source
    }

    #[tokio::test]
    async fn discover_applies_ancestor_ignore_files() {
        let dir = TempDir::new().unwrap();
        let source = source_under_ignored_store(dir.path());

        let found = discover(&source, &["**"], &RealSys).await.unwrap();

        assert!(found.is_empty(), "the workspace rule hides it: {found:?}");
    }

    #[tokio::test]
    async fn discover_in_source_skips_ancestor_ignore_files() {
        let dir = TempDir::new().unwrap();
        let source = source_under_ignored_store(dir.path());

        let found = discover_in_source(&source, &["**"], &RealSys)
            .await
            .unwrap();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "remote-gen");
    }

    #[tokio::test]
    async fn discover_in_source_keeps_the_sources_own_ignore_files() {
        let dir = TempDir::new().unwrap();
        let source = source_under_ignored_store(dir.path());
        std::fs::write(source.join(".omniignore"), "generator.omni.yaml\n")
            .unwrap();

        let found = discover_in_source(&source, &["**"], &RealSys)
            .await
            .unwrap();

        assert!(found.is_empty(), "the source's own rule applies: {found:?}");
    }
}
