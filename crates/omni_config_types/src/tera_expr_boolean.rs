use derive_new::new;
#[cfg(feature = "serde")]
use strum::{EnumIs, EnumTryAs};

use crate::TeraExpr;

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, EnumIs, EnumTryAs, new,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", serde(untagged))]
#[cfg_attr(feature = "schemars", schemars(untagged))]
pub enum TeraExprBoolean {
    Expr(#[new(into)] TeraExpr),
    Boolean(#[new(into)] bool),
}

#[cfg(feature = "serde")]
impl<'a> serde::Deserialize<'a> for TeraExprBoolean {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'a>,
    {
        use omni_serde_validators::tera_expr::validate_str;
        serde_untagged::UntaggedEnumVisitor::new()
            .bool(|v| Ok(TeraExprBoolean::Boolean(v)))
            .string(|v| {
                validate_str(&v).map_err(serde::de::Error::custom)?;
                Ok(TeraExprBoolean::Expr(TeraExpr::new(v)))
            })
            .deserialize(deserializer)
    }
}

impl From<bool> for TeraExprBoolean {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

impl From<String> for TeraExprBoolean {
    fn from(value: String) -> Self {
        Self::Expr(TeraExpr::new(value))
    }
}

impl Default for TeraExprBoolean {
    fn default() -> Self {
        Self::Boolean(false)
    }
}

#[cfg(feature = "merge")]
impl merge::Merge for TeraExprBoolean {
    fn merge(&mut self, other: Self) {
        *self = other;
    }
}
