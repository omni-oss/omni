use std::borrow::Borrow;

use lazy_regex::{Lazy, Regex, regex};
use serde_validate::{StaticValidator, declare_static_validator};
use sets::unordered_set;

use crate::GeneratorSourceConfiguration;

#[derive(Debug, Clone, Copy, Default)]
struct GeneratorSourcesValidator;

impl<T: Borrow<Vec<GeneratorSourceConfiguration>>> StaticValidator<T>
    for GeneratorSourcesValidator
{
    fn validate_static(value: &T) -> Result<(), String> {
        let value = value.borrow();
        let mut encountered_uri = unordered_set!();

        for item in value {
            match item {
                GeneratorSourceConfiguration::Local(_) => {
                    // do nothing with local sources
                }
                GeneratorSourceConfiguration::Git(git) => {
                    if !encountered_uri.insert(git.uri.as_str()) {
                        return Err(format!(
                            "Duplicate generator source git uri found: {}\nGenerator source git uri should be unique",
                            git.uri
                        ));
                    }
                }
            }
        }

        Ok(())
    }
}

declare_static_validator!(
    GeneratorSourcesValidator,
    Vec<GeneratorSourceConfiguration>,
    validate_generator_sources,
    option_validate_generator_sources,
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
