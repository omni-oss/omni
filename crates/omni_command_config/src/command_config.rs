use std::borrow::Cow;

use strum::EnumIs;

/// A task command, expressed either as a shell-style string (split into argv
/// exactly once via `shlex`) or as an explicit argv vector (never parsed).
///
/// Deserialization is untagged: a scalar becomes [`CommandConfig::Shell`], a
/// sequence becomes [`CommandConfig::Argv`]. Each string is validated as a Tera
/// expression at deserialize time.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, EnumIs)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", serde(untagged))]
#[cfg_attr(feature = "schemars", schemars(untagged))]
pub enum CommandConfig {
    /// A shell-style string, split into argv exactly once via `shlex`.
    Shell(String),
    /// An explicit argv; never parsed.
    Argv(Vec<String>),
}

impl CommandConfig {
    pub fn shell(command: impl Into<String>) -> Self {
        Self::Shell(command.into())
    }

    pub fn argv(items: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::Argv(items.into_iter().map(Into::into).collect())
    }

    /// A stable, raw (pre-Tera, pre-env-expansion) textual representation used
    /// for cache/hash digests.
    ///
    /// - [`CommandConfig::Shell`] returns the raw string unchanged
    ///   (`Cow::Borrowed`, zero-alloc) so shell-command cache hashes are
    ///   byte-identical to the pre-`CommandConfig` era.
    /// - [`CommandConfig::Argv`] returns a deterministic JSON encoding of the
    ///   raw vector (`Cow::Owned`).
    pub fn canonical(&self) -> Cow<'_, str> {
        match self {
            CommandConfig::Shell(s) => Cow::Borrowed(s.as_str()),
            CommandConfig::Argv(items) => Cow::Owned(
                serde_json::to_string(items)
                    .expect("Vec<String> serialization is infallible"),
            ),
        }
    }
}

impl From<String> for CommandConfig {
    fn from(value: String) -> Self {
        Self::Shell(value)
    }
}

impl From<&str> for CommandConfig {
    fn from(value: &str) -> Self {
        Self::Shell(value.to_string())
    }
}

impl From<Vec<String>> for CommandConfig {
    fn from(value: Vec<String>) -> Self {
        Self::Argv(value)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for CommandConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use omni_serde_validators::tera_expr::validate_str;
        serde_untagged::UntaggedEnumVisitor::new()
            .string(|v| {
                validate_str(&v).map_err(serde::de::Error::custom)?;
                Ok(CommandConfig::Shell(v.to_string()))
            })
            .seq(|s| {
                let items: Vec<String> = s.deserialize()?;
                for item in &items {
                    validate_str(item).map_err(serde::de::Error::custom)?;
                }
                Ok(CommandConfig::Argv(items))
            })
            .deserialize(deserializer)
    }
}

#[cfg(feature = "merge")]
impl merge::Merge for CommandConfig {
    fn merge(&mut self, other: Self) {
        *self = other;
    }
}
