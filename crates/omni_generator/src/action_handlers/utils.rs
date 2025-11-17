use either::Left;
use omni_generator_configurations::{OmniPath, OverwriteConfiguration, Root};
use std::{
    borrow::Cow,
    path::{self, Path, PathBuf},
};

use maps::{UnorderedMap, unordered_map};
use omni_prompt::configuration::{
    BasePromptConfiguration, ConfirmPromptConfiguration, PromptConfiguration,
    PromptingConfiguration, TextPromptConfiguration,
    ValidatedPromptConfiguration,
};
use path_clean::clean;
use strum::{EnumDiscriminants, IntoDiscriminant};

use crate::{
    GeneratorSys,
    action_handlers::HandlerContext,
    error::{Error, ErrorInner},
    utils::expand_json_value,
};

pub fn resolve_output_path(
    output_dir: &Path,
    target: Option<&Path>,
    base_path: &Path,
    template_path: &Path,
    flatten: bool,
) -> Result<PathBuf, ResolveOutputPathError> {
    if let Some(target) = target {
        validate_target(output_dir, target)?;
    }

    let output_dir = if let Some(target) = target {
        clean(output_dir.join(target))
    } else {
        clean(output_dir)
    };
    let base_path = clean(base_path);
    let template_path = clean(template_path);

    let template_path = if flatten {
        Path::new(template_path.file_name().expect("should have file name"))
    } else {
        &template_path
    };

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

pub async fn get_target_dir<'a>(
    target_name: &str,
    target_overrides: &'a UnorderedMap<String, OmniPath>,
    generator_targets: &'a UnorderedMap<String, OmniPath>,
    output_dir: &Path,
    _sys: &impl GeneratorSys,
) -> Result<Cow<'a, OmniPath>, Error> {
    let target = target_overrides
        .get(target_name)
        .or_else(|| generator_targets.get(target_name));

    if let Some(target) = target {
        validate_target(output_dir, target.unresolved_path())?;
        return Ok(Cow::Borrowed(target));
    }

    let prompt_cfg =
        PromptConfiguration::new_text(TextPromptConfiguration::new(
            ValidatedPromptConfiguration::new(
                BasePromptConfiguration::new(
                    target_name,
                    format!("Directory for target {}:", target_name),
                    None,
                ),
                [],
            ),
            None,
        ));

    let cfg = PromptingConfiguration::default();

    loop {
        let result = omni_prompt::prompt_one(
            &prompt_cfg,
            None,
            &unordered_map!(),
            &cfg,
        )?
        .expect("should have value");

        let path_str = result.by_ref().to_str().expect("should be string");
        let path = Path::new(&path_str as &str);

        if let Err(err) = validate_target(output_dir, path) {
            trace::error!("invalid target dir: {}", err);
        }

        break Ok(Cow::Owned(OmniPath::new(path)));
    }
}

pub async fn get_target_file<'a>(
    target_name: &str,
    ctx: &HandlerContext<'a>,
    sys: &impl GeneratorSys,
) -> Result<Cow<'a, Path>, Error> {
    let base = enum_map::enum_map! {
        Root::Workspace => ctx.workspace_dir,
        Root::Output => ctx.output_path,
    };
    let target = ctx
        .generator_targets
        .get(target_name)
        .or_else(|| ctx.target_overrides.get(target_name))
        .map(|p| p.resolve(&base));
    let target = if let Some(target) = target {
        target
    } else {
        Cow::Owned(prompt_target_file(target_name, ctx, sys).await?)
    };

    Ok(target)
}

pub async fn prompt_target_file(
    target_name: &str,
    ctx: &HandlerContext<'_>,
    sys: &impl GeneratorSys,
) -> Result<PathBuf, Error> {
    let prompt_cfg =
        PromptConfiguration::new_text(TextPromptConfiguration::new(
            ValidatedPromptConfiguration::new(
                BasePromptConfiguration::new(
                    target_name,
                    format!("Directory for target {}:", target_name),
                    None,
                ),
                [],
            ),
            None,
        ));

    let cfg = PromptingConfiguration::default();

    let base = enum_map::enum_map! {
        Root::Workspace => ctx.workspace_dir,
        Root::Output => ctx.output_path,
    };

    loop {
        let result = omni_prompt::prompt_one(
            &prompt_cfg,
            None,
            &unordered_map!(),
            &cfg,
        )?
        .expect("should have value");

        let path_str = result.by_ref().to_str().expect("should be string");
        let path = if let Ok(path) = path_str.as_ref().parse::<OmniPath>() {
            path
        } else {
            trace::warn!("invalid path: {}", path_str);
            continue;
        };

        let resolved = path.resolve(&base);

        if !sys.fs_exists_async(resolved.as_ref()).await? {
            trace::warn!("file does not exist: {}", resolved.display());
        }

        break Ok(resolved.to_path_buf());
    }
}

pub fn validate_target(
    output_dir: &Path,
    target: &Path,
) -> Result<(), ResolveOutputPathError> {
    Ok({
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
    })
}

pub fn strip_extensions<'a, TStr: AsRef<str> + 'a>(
    path: &'a Path,
    exts: &'a [TStr],
) -> Cow<'a, Path> {
    if !exts.is_empty() {
        for check in exts {
            if let Some(ext) = path.extension()
                && *ext.to_string_lossy() == *check.as_ref()
            {
                return Cow::Owned(path.with_extension(""));
            }
        }
    }

    Cow::Borrowed(path)
}

pub async fn get_output_path<'a, TExt: AsRef<str>>(
    target_name: Option<&'a str>,
    expected_output_path: &'a Path,
    base_path: Option<&'a Path>,
    ctx: &HandlerContext<'a>,
    strip_extensions: &'a [TExt],
    flatten: bool,
    sys: &impl GeneratorSys,
) -> Result<PathBuf, Error> {
    let target = if let Some(target_name) = target_name {
        Some(
            get_target_dir(
                target_name,
                &ctx.target_overrides,
                &ctx.generator_targets,
                ctx.output_path,
                sys,
            )
            .await?,
        )
    } else {
        None
    };

    let bases = enum_map::enum_map! {
        Root::Output => ctx.output_path,
        Root::Workspace => ctx.workspace_dir,
    };

    let target = target.map(|p| {
        let resolved = p.as_ref().resolve(&bases);

        if resolved.starts_with(ctx.output_path) {
            resolved
                .strip_prefix(ctx.output_path)
                .expect("should remove output dir prefix")
                .to_path_buf()
        } else {
            resolved.to_path_buf()
        }
    });

    let output_path = resolve_output_path(
        ctx.output_path,
        target.as_deref(),
        base_path.unwrap_or(ctx.generator_dir),
        &expected_output_path,
        flatten,
    )?;

    Ok(if !strip_extensions.is_empty() {
        self::strip_extensions(&output_path, strip_extensions).to_path_buf()
    } else {
        output_path
    })
}

pub fn augment_tera_context(
    tera_ctx: &tera::Context,
    data: Option<&UnorderedMap<String, serde_json::Value>>,
    _ctx: &HandlerContext<'_>,
) -> Result<tera::Context, Error> {
    let mut new_ctx = tera_ctx.clone();

    if let Some(data) = data {
        add_data_internal(&mut new_ctx, data)?;
    }

    Ok(new_ctx)
}

#[allow(unused)]
pub fn add_data(
    tera_ctx: &tera::Context,
    data: &UnorderedMap<String, serde_json::Value>,
) -> Result<tera::Context, Error> {
    let mut tera_ctx_with_data = tera_ctx.clone();

    add_data_internal(&mut tera_ctx_with_data, data)?;

    Ok(tera_ctx_with_data)
}

fn add_data_internal(
    tera_ctx: &mut tera::Context,
    data: &UnorderedMap<String, serde_json::Value>,
) -> Result<(), Error> {
    let mut expanded_data = unordered_map!(cap: data.len());

    for (key, value) in data {
        expanded_data
            .insert(key.clone(), expand_json_value(tera_ctx, &key, value)?);
    }

    tera_ctx.insert("data", &expanded_data);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn test_resolve_output_path() {
        let output_dir = PathBuf::from(if cfg!(windows) {
            "D:\\output"
        } else {
            "/output"
        });
        let target = Some(PathBuf::from("target"));
        let base_path = PathBuf::from(if cfg!(windows) {
            "D:\\template\\files"
        } else {
            "/template/files"
        });
        let template_path = PathBuf::from(if cfg!(windows) {
            "D:\\template\\files\\file"
        } else {
            "/template/files/file"
        });

        let resolved_path = resolve_output_path(
            &output_dir,
            target.as_deref(),
            &base_path,
            &template_path,
            false,
        )
        .unwrap();

        assert_eq!(
            resolved_path,
            PathBuf::from(if cfg!(windows) {
                "D:\\output\\target\\file"
            } else {
                "/output/target/file"
            })
        );
    }

    #[test]
    fn test_resolve_output_path_with_flatten() {
        let output_dir = PathBuf::from(if cfg!(windows) {
            "D:\\output"
        } else {
            "/output"
        });
        let target = Some(PathBuf::from("target"));
        let base_path = PathBuf::from(if cfg!(windows) {
            "D:\\template\\files"
        } else {
            "/template/files"
        });
        let template_path = PathBuf::from(if cfg!(windows) {
            "D:\\template\\files\\file\\file.txt"
        } else {
            "/template/files/file/file.txt"
        });

        let resolved_path = resolve_output_path(
            &output_dir,
            target.as_deref(),
            &base_path,
            &template_path,
            true,
        )
        .unwrap();

        assert_eq!(
            resolved_path,
            PathBuf::from(if cfg!(windows) {
                "D:\\output\\target\\file.txt"
            } else {
                "/output/target/file.txt"
            })
        );
    }

    #[test]
    fn test_strip_extensions() {
        assert_eq!(
            strip_extensions(Path::new("file.txt"), &["txt"]),
            PathBuf::from("file")
        );
        assert_eq!(
            strip_extensions(Path::new("file.txt"), &["txt", "txt2"]),
            PathBuf::from("file")
        );
        assert_eq!(
            strip_extensions(Path::new("file.txt.txt2"), &["txt"]),
            PathBuf::from("file.txt.txt2")
        );
        assert_eq!(
            strip_extensions(Path::new("file.txt.txt2"), &["txt", "txt2"]),
            PathBuf::from("file.txt")
        );
    }
}
