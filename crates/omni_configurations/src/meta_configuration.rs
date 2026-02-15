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
    pub fn into_expression_context(
        self,
    ) -> Result<omni_expressions::Context<'static>, omni_expressions::Error>
    {
        let mut ctx = omni_expressions::Context::default();

        for (k, v) in self.0.iter() {
            ctx.add_variable(k, v.clone().into_json())?;
        }

        Ok(ctx)
    }
}
