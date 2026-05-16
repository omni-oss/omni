use derive_new::new;
use garde::Validate;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    JsonSchema,
    Validate,
    new,
)]
#[serde(tag = "source", rename_all = "kebab-case")]
#[garde(allow_unvalidated)]
pub enum GeneratorSourceConfiguration {
    Local(LocalGeneratorSourceConfiguration),
    Git(GitGeneratorSourceConfiguration),
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    JsonSchema,
    Validate,
    new,
)]
#[garde(allow_unvalidated)]
pub struct LocalGeneratorSourceConfiguration {
    #[new(into)]
    pub path: String,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    JsonSchema,
    Validate,
    new,
)]
#[garde(allow_unvalidated)]
pub struct GitGeneratorSourceConfiguration {
    #[new(into)]
    pub uri: Url,

    #[new(into)]
    pub rev: String,
}
