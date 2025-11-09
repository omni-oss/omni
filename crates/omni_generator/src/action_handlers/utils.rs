use either::Left;
use omni_generator_configurations::OverwriteConfiguration;
use std::path::{self, Path, PathBuf};

use maps::{UnorderedMap, unordered_map};
use omni_prompt::configuration::{
    BasePromptConfiguration, ConfirmPromptConfiguration, PromptConfiguration,
    PromptingConfiguration,
};
use path_clean::clean;
use strum::{EnumDiscriminants, IntoDiscriminant};
use value_bag::OwnedValueBag;

use crate::{
    GeneratorSys,
    error::{Error, ErrorInner},
};

pub fn get_tera_context(
    context_values: &UnorderedMap<String, OwnedValueBag>,
) -> tera::Context {
    let mut context = tera::Context::new();

    for (key, value) in context_values.iter() {
        context.insert(key, value);
    }

    context
}

pub fn resolve_output_path(
    output_dir: &Path,
    target: Option<&Path>,
    base_path: &Path,
    template_path: &Path,
) -> Result<PathBuf, ResolveOutputPathError> {
    if let Some(target) = target {
        if target.is_absolute() {
            return Err(ResolveOutputPathErrorInner::TargetIsAbsolute {
                target: target.to_path_buf(),
            })?;
        }

        let target_absolute = path::absolute(output_dir.join(target))?;
        if !target_absolute.starts_with(output_dir) {
            return Err(
                ResolveOutputPathErrorInner::TargetIsOutsideOutputDir {
                    target: target_absolute,
                    output_dir: output_dir.to_path_buf(),
                },
            )?;
        }
    }

    let output_dir = if let Some(target) = target {
        clean(output_dir.join(target))
    } else {
        clean(output_dir)
    };
    let base_path = clean(base_path);
    let template_path = clean(template_path);

    Ok(if template_path.starts_with(&base_path) {
        output_dir.join(
            template_path
                .strip_prefix(&base_path)
                .expect("should strip prefix successfully"),
        )
    } else {
        output_dir.join(template_path)
    })
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ResolveOutputPathError(ResolveOutputPathErrorInner);

impl ResolveOutputPathError {
    #[allow(unused)]
    pub fn kind(&self) -> ResolveOutputPathErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ResolveOutputPathErrorInner>> From<T> for ResolveOutputPathError {
    fn from(value: T) -> Self {
        let value = value.into();

        Self(value)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(ResolveOutputPathErrorKind))]
#[error(transparent)]
pub(crate) enum ResolveOutputPathErrorInner {
    #[error("target should be relative, absoulate target is passed: {target}")]
    TargetIsAbsolute { target: PathBuf },

    #[error(
        "target should be resolved to be inside output dir, target is outside: {target}, output dir: {output_dir}"
    )]
    TargetIsOutsideOutputDir {
        target: PathBuf,
        output_dir: PathBuf,
    },

    #[error(transparent)]
    GenericIo(#[from] std::io::Error),
}

pub async fn ensure_dir_exists(
    dir: &Path,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    if !sys.fs_exists_async(dir).await? {
        sys.fs_create_dir_all_async(dir).await?;
    }

    if !sys.fs_is_dir_async(dir).await? {
        return Err(ErrorInner::new_path_exists_but_not_dir(dir))?;
    }

    Ok(())
}

pub async fn should_overwrite(
    path: &Path,
    overwrite: Option<OverwriteConfiguration>,
    sys: &impl GeneratorSys,
) -> Result<bool, Error> {
    if let Some(overwrite) = overwrite {
        match overwrite {
            OverwriteConfiguration::Prompt => {
                // will be handled by the next lines
            }
            OverwriteConfiguration::Always => return Ok(true),
            OverwriteConfiguration::Never => return Ok(false),
        }
    }

    let is_dir = sys.fs_is_dir_async(path).await?;

    let prompt_cfg = PromptConfiguration::new_confirm(
        ConfirmPromptConfiguration::new(
            BasePromptConfiguration::new(
                "overwrite_path",
                if is_dir {
                    format!(
                        "Directory already exists at path: {path:?}. Delete it and all of its contents?"
                    )
                } else {
                    format!("File already exists at path: {path:?}. Overwrite?")
                },
                None,
            ),
            Some(Left(true)),
        ),
    );

    let cfg = PromptingConfiguration::default();

    let result =
        omni_prompt::prompt_one(&prompt_cfg, None, &unordered_map!(), &cfg)?
            .expect("should have value");

    let bool_result = result.by_ref().to_bool().expect("should be bool");

    Ok(bool_result)
}

pub async fn overwrite(
    output_path: &Path,
    overwrite: Option<OverwriteConfiguration>,
    sys: &impl GeneratorSys,
) -> Result<Option<bool>, Error> {
    if sys.fs_exists_async(&output_path).await? {
        let overwrite = should_overwrite(&output_path, overwrite, sys).await?;
        let output_path_d = output_path.display();
        if overwrite {
            if sys.fs_is_dir_async(&output_path).await? {
                trace::info!(
                    "Removing directory and it's contents at path {}",
                    output_path_d
                );
                sys.fs_remove_dir_all_async(&output_path).await?;
            } else {
                trace::info!("Overwriting path at {}", output_path_d);
            }

            return Ok(Some(true));
        } else {
            return Ok(Some(false));
        }
    }

    return Ok(None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_output_path() {
        use std::path::PathBuf;

        let output_dir = PathBuf::from("/output");
        let target = Some(PathBuf::from("target"));
        let base_path = PathBuf::from("/template/files");
        let template_path = PathBuf::from("/template/files/file");

        let resolved_path = resolve_output_path(
            &output_dir,
            target.as_deref(),
            &base_path,
            &template_path,
        )
        .unwrap();

        assert_eq!(resolved_path, PathBuf::from("/output/target/file"));
    }
}
