use std::borrow::Borrow;
use std::marker::PhantomData;

use lazy_regex::{Lazy, Regex, regex};
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
