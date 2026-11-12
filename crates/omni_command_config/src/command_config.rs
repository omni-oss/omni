use std::{borrow::Cow, collections::BTreeMap};

use strum::EnumIs;

/// One argv element together with how it must be interpreted.
///
/// Authored config strings are always [`CommandOperand::Expression`]; data
/// derived from elsewhere (script names, project names) must be a
/// [`CommandOperand::Literal`] so it is never Tera-rendered or env-expanded.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, EnumIs)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum CommandOperand {
    /// Used verbatim as one argument.
    Literal(String),
    /// Tera-rendered then env-expanded, as one argument.
    Expression(String),
    /// An authored expression with data bound to template variables. The
    /// bound values are substituted after rendering and env expansion, so
    /// they are never reinterpreted. Value-transforming filters apply to an
    /// opaque placeholder, not the value.
    Interpolated {
        expression: String,
        literals: BTreeMap<String, String>,
    },
}

impl CommandOperand {
    pub fn literal(value: impl Into<String>) -> Self {
        Self::Literal(value.into())
    }

    pub fn expression(value: impl Into<String>) -> Self {
        Self::Expression(value.into())
    }

    pub fn interpolated(
        expression: impl Into<String>,
        literals: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        Self::Interpolated {
            expression: expression.into(),
            literals: literals
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }

    /// Like [`Self::expression`], rejecting invalid Tera syntax.
    #[cfg(feature = "serde")]
    pub fn try_expression(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        omni_serde_validators::tera_expr::validate_str(&value)?;
        Ok(Self::Expression(value))
    }
}

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
    /// An explicit argv whose elements carry their interpretation mode. Not
    /// producible from config files; not part of the config schema.
    #[cfg_attr(feature = "serde", serde(skip))]
    #[cfg_attr(feature = "schemars", schemars(skip))]
    Operands(Vec<CommandOperand>),
}

impl CommandConfig {
    pub fn shell(command: impl Into<String>) -> Self {
        Self::Shell(command.into())
    }

    pub fn operands(items: impl IntoIterator<Item = CommandOperand>) -> Self {
        Self::Operands(items.into_iter().collect())
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
    /// - [`CommandConfig::Operands`] returns a JSON encoding that tags each
    ///   operand with its mode, so a literal and an equal-text expression
    ///   differ.
    pub fn canonical(&self) -> Cow<'_, str> {
        match self {
            CommandConfig::Shell(s) => Cow::Borrowed(s.as_str()),
            CommandConfig::Argv(items) => Cow::Owned(
                serde_json::to_string(items)
                    .expect("Vec<String> serialization is infallible"),
            ),
            CommandConfig::Operands(items) => Cow::Owned(
                serde_json::to_string(
                    &items.iter().map(operand_json).collect::<Vec<_>>(),
                )
                .expect("JSON value serialization is infallible"),
            ),
        }
    }
}

fn operand_json(op: &CommandOperand) -> serde_json::Value {
    use serde_json::json;
    match op {
        CommandOperand::Literal(s) => json!({"literal": s}),
        CommandOperand::Expression(s) => json!({"expression": s}),
        CommandOperand::Interpolated {
            expression,
            literals,
        } => {
            json!({"interpolated": {"expression": expression, "literals": literals}})
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_of_existing_forms_is_unchanged() {
        assert_eq!(
            CommandConfig::shell("echo $A {{ b }}").canonical(),
            "echo $A {{ b }}"
        );
        assert_eq!(
            CommandConfig::argv(["echo", "a b"]).canonical(),
            r#"["echo","a b"]"#
        );
    }

    #[test]
    fn canonical_differs_by_interpretation_mode() {
        let lit = CommandConfig::operands([CommandOperand::literal("$VAR")]);
        let expr =
            CommandConfig::operands([CommandOperand::expression("$VAR")]);
        let argv = CommandConfig::argv(["$VAR"]);
        assert_eq!(lit.canonical(), r#"[{"literal":"$VAR"}]"#);
        assert_eq!(expr.canonical(), r#"[{"expression":"$VAR"}]"#);
        assert_ne!(lit.canonical(), expr.canonical());
        assert_ne!(expr.canonical(), argv.canonical());
    }

    #[test]
    fn canonical_of_interpolated_is_stable() {
        let c = CommandConfig::operands([CommandOperand::interpolated(
            "{{ n }}",
            [("n", "v")],
        )]);
        assert_eq!(
            c.canonical(),
            r#"[{"interpolated":{"expression":"{{ n }}","literals":{"n":"v"}}}]"#
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn try_expression_validates() {
        assert!(CommandOperand::try_expression("{{ bad").is_err());
        assert!(CommandOperand::try_expression("{{ ok }}").is_ok());
    }
}
