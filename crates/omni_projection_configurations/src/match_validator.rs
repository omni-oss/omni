use std::borrow::Borrow;

use omni_config_types::SingleOrMany;
use serde_validate::{StaticValidator, declare_static_validator};

#[derive(Debug, Clone, Copy, Default)]
struct MatchPatternsValidator;

impl<T: Borrow<SingleOrMany<String>>> StaticValidator<T>
    for MatchPatternsValidator
{
    fn validate_static(value: &T) -> Result<(), String> {
        let patterns = value.borrow();

        if let Some(items) = patterns.as_slice()
            && items.is_empty()
        {
            return Err("match/scope list must not be empty".to_string());
        }

        let mut has_include = false;
        for pattern in patterns.iter() {
            if pattern.trim().is_empty() {
                return Err(
                    "match/scope pattern must not be empty or whitespace"
                        .to_string(),
                );
            }
            if omni_glob::is_include(pattern) {
                has_include = true;
            }
        }

        if !has_include {
            return Err(
                "match/scope must contain at least one include pattern; \
                 an exclude-only list matches nothing"
                    .to_string(),
            );
        }

        Ok(())
    }
}

declare_static_validator!(
    MatchPatternsValidator,
    SingleOrMany<String>,
    validate_match_patterns,
    option_validate_match_patterns,
);
