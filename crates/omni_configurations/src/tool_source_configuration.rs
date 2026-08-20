use derive_new::new;
use garde::Validate;
use omni_config_types::SingleOrMany;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::Url;

/// A registered source of tool manifests, mirroring
/// [`GeneratorSourceConfiguration`](crate::GeneratorSourceConfiguration).
///
/// The serialized shape is intentionally identical to the generator source so
/// the two can be unified behind one shared type in a future revision without
/// a wire-format change.
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
#[serde(tag = "source", rename_all = "kebab-case", deny_unknown_fields)]
#[garde(allow_unvalidated)]
pub enum ToolSourceConfiguration {
    Local(LocalToolSourceConfiguration),
    // v2: remote `git` tool sources are reserved but not yet resolved. When
    // implemented, tools loaded from a `git` source receive a stricter
    // read-only capability floor (network/process/env/fs-write denied) at
    // `require-floor` strictness, distinct from the local `@workspace/**` floor.
    Git(GitToolSourceConfiguration),
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
#[serde(deny_unknown_fields)]
#[garde(allow_unvalidated)]
pub struct LocalToolSourceConfiguration {
    #[new(into)]
    pub path: SingleOrMany<String>,
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
#[serde(deny_unknown_fields)]
#[garde(allow_unvalidated)]
pub struct GitToolSourceConfiguration {
    #[new(into)]
    pub uri: Url,

    #[new(into)]
    pub rev: String,
}
