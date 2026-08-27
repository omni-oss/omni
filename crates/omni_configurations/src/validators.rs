use std::borrow::Borrow;
use std::marker::PhantomData;

use lazy_regex::{Lazy, Regex, regex};
use serde_validate::{StaticValidator, declare_static_validator};
use sets::unordered_set;

use crate::{NoExtra, ProjectionExtra, ProjectionStrategy, SourceConfig};

/// Compile-time label distinguishing source kinds in validation error messages.
pub trait SourceKindLabel {
    const LABEL: &'static str;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GeneratorLabel;
impl SourceKindLabel for GeneratorLabel {
    const LABEL: &'static str = "generator";
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ToolLabel;
impl SourceKindLabel for ToolLabel {
    const LABEL: &'static str = "tool";
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ProjectionLabel;
impl SourceKindLabel for ProjectionLabel {
    const LABEL: &'static str = "projection";
}

#[derive(Debug, Clone, Copy, Default)]
struct SourcesValidator<E, L>(PhantomData<(E, L)>);

impl<E, L, T> StaticValidator<T> for SourcesValidator<E, L>
where
    T: Borrow<Vec<SourceConfig<E>>>,
    L: SourceKindLabel,
{
    fn validate_static(value: &T) -> Result<(), String> {
        let value = value.borrow();
        let mut encountered_uri = unordered_set!();

        for item in value {
            match item {
                SourceConfig::Local(_) => {
                    // do nothing with local sources
                }
                SourceConfig::Git(git) => {
                    if !encountered_uri.insert(git.uri.as_str()) {
                        return Err(format!(
                            "Duplicate {label} source git uri found: {}\nEach {label} source git uri must be unique",
                            git.uri,
                            label = L::LABEL,
                        ));
                    }
                }
            }
        }

        Ok(())
    }
}

declare_static_validator!(
    SourcesValidator<NoExtra, GeneratorLabel>,
    Vec<SourceConfig<NoExtra>>,
    validate_generator_sources,
    option_validate_generator_sources,
);

declare_static_validator!(
    SourcesValidator<NoExtra, ToolLabel>,
    Vec<SourceConfig<NoExtra>>,
    validate_tool_sources,
    option_validate_tool_sources,
);

#[derive(Debug, Clone, Copy, Default)]
struct ProjectionSourcesValidator;

impl<T: Borrow<Vec<SourceConfig<ProjectionExtra>>>> StaticValidator<T>
    for ProjectionSourcesValidator
{
    fn validate_static(value: &T) -> Result<(), String> {
        let sources = value.borrow();

        // Reuse the shared git-uri dedup for projection sources.
        SourcesValidator::<ProjectionExtra, ProjectionLabel>::validate_static(
            sources,
        )?;

        let mut encountered_id = unordered_set!();
        for source in sources {
            let extra = match source {
                SourceConfig::Local(local) => &local.extra,
                SourceConfig::Git(git) => &git.extra,
            };

            if !encountered_id.insert(extra.id.as_str()) {
                return Err(format!(
                    "Duplicate projection source id found: {}\nEach projection source id must be unique",
                    extra.id
                ));
            }

            for projection in &extra.projections {
                if projection.strategy == ProjectionStrategy::Namespaced
                    && !projection.rules.is_empty()
                {
                    return Err(format!(
                        "Projection source '{}' uses the `namespaced` strategy with `rules`\nThe `namespaced` strategy links the whole source and does not accept `rules`",
                        extra.id
                    ));
                }
            }
        }

        Ok(())
    }
}

declare_static_validator!(
    ProjectionSourcesValidator,
    Vec<SourceConfig<ProjectionExtra>>,
    validate_projection_sources,
    option_validate_projection_sources,
);

#[derive(Debug, Clone, Copy, Default)]
struct ProjectionIdValidator;

static PROJECTION_ID_SEGMENT_REGEX: &Lazy<Regex> =
    regex!(r"^@?[A-Za-z0-9](?:[A-Za-z0-9._-]*[A-Za-z0-9])?$");

fn is_windows_reserved_name(segment: &str) -> bool {
    let stem = segment.split('.').next().unwrap_or(segment);
    let stem = stem.to_ascii_uppercase();

    match stem.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" => true,
        _ => {
            let is_numbered = |prefix: &str| {
                stem.strip_prefix(prefix).is_some_and(|n| {
                    n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9')
                })
            };
            is_numbered("COM") || is_numbered("LPT")
        }
    }
}

impl<T: Borrow<String>> StaticValidator<T> for ProjectionIdValidator {
    fn validate_static(value: &T) -> Result<(), String> {
        let id = value.borrow();

        if id.is_empty() {
            return Err("projection id must not be empty".to_string());
        }
        if id.contains('\\') {
            return Err(format!(
                "projection id must use `/` as the only path separator: {id}"
            ));
        }
        if id.starts_with('/') {
            return Err(format!("projection id must be relative: {id}"));
        }

        for segment in id.split('/') {
            if segment.is_empty() {
                return Err(format!(
                    "projection id must not contain empty segments: {id}"
                ));
            }
            if segment == "." || segment == ".." {
                return Err(format!(
                    "projection id must not contain `.` or `..` segments: {id}"
                ));
            }
            if !PROJECTION_ID_SEGMENT_REGEX.is_match(segment) {
                return Err(format!(
                    "invalid projection id segment `{segment}` in: {id}"
                ));
            }
            if is_windows_reserved_name(segment) {
                return Err(format!(
                    "projection id segment `{segment}` is a reserved device name: {id}"
                ));
            }
        }

        Ok(())
    }
}

declare_static_validator!(
    ProjectionIdValidator,
    String,
    validate_projection_id,
    option_validate_projection_id,
);

#[derive(Debug, Clone, Copy, Default)]
#[allow(unused)]
struct SourceNameValidator;

#[allow(unused)]
static SOURCE_NAME_REGEX: &Lazy<Regex> =
    regex!(r"^(?:@[a-zA-Z0-9._-]+/)?[a-zA-Z0-9._-]+$");

impl<T: Borrow<String>> StaticValidator<T> for SourceNameValidator {
    fn validate_static(value: &T) -> Result<(), String> {
        let value = value.borrow();

        if !SOURCE_NAME_REGEX.is_match(value) {
            return Err(format!("Invalid source name format: {value}"));
        }

        Ok(())
    }
}

declare_static_validator!(
    SourceNameValidator,
    String,
    validate_source_name,
    option_validate_source_name
);
