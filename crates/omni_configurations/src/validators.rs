use std::borrow::Borrow;
use std::marker::PhantomData;

use lazy_regex::{Lazy, Regex, regex};
use omni_projection_configurations::{ProjectionExtra, ProjectionStrategy};
use serde_validate::{StaticValidator, declare_static_validator};
use sets::unordered_set;

use crate::{NoExtra, SourceConfig};

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
