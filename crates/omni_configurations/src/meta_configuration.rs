use config_utils::{DictConfig, DynValue};
use derive_new::new;
use garde::Validate;
use merge::Merge;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Deserialize,
    Serialize,
    JsonSchema,
    Clone,
    Debug,
    PartialEq,
    Merge,
    Validate,
    Default,
    new,
)]
#[garde(allow_unvalidated)]
#[serde(transparent)]
pub struct MetaConfiguration(pub DictConfig<DynValue>);

impl MetaConfiguration {
    /// Builds the CEL context for a meta filter: every top-level key is a
    /// variable, and the whole map is also available as `meta` (unless a key
    /// is itself named `meta`).
    pub fn to_expression_context(
        &self,
    ) -> Result<
        omni_expressions::Context<'static, 'static>,
        omni_expressions::Error,
    > {
        let mut ctx = omni_expressions::Context::default();
        ctx.add_map_with_root(
            META_VARIABLE,
            self.0
                .iter()
                .map(|(k, v)| (k.as_str(), v.clone().into_json())),
        )?;
        Ok(ctx)
    }

    pub fn empty_expression_context() -> Result<
        omni_expressions::Context<'static, 'static>,
        omni_expressions::Error,
    > {
        Self::default().to_expression_context()
    }
}

pub const META_VARIABLE: &str = "meta";

#[cfg(test)]
mod tests {
    use super::*;
    use config_utils::DynValue;

    fn meta(entries: &[(&str, DynValue)]) -> MetaConfiguration {
        MetaConfiguration::new(DictConfig::value(
            entries
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect::<maps::Map<_, _>>(),
        ))
    }

    fn eval(m: &MetaConfiguration, expr: &str) -> bool {
        omni_expressions::parse(expr)
            .unwrap()
            .evaluate_bool(&m.to_expression_context().unwrap())
            .unwrap()
    }

    #[test]
    fn exposes_keys_and_meta() {
        let m = meta(&[("kind", DynValue::new_string("e2e"))]);
        assert!(eval(&m, "kind == 'e2e'"));
        assert!(eval(&m, "meta.kind == 'e2e'"));
        assert!(!eval(&m, "has(meta.other)"));
    }

    #[test]
    fn empty_meta_has_empty_map() {
        let m = MetaConfiguration::default();
        assert!(!eval(&m, "has(meta.kind)"));
    }

    #[test]
    fn task_key_named_meta_wins() {
        let m = meta(&[("meta", DynValue::new_integer(1))]);
        assert!(eval(&m, "meta == 1"));
    }
}
