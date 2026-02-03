use derive_new::new;
#[cfg(feature = "serde")]
use omni_serde_validators::tera_expr::validate_tera_expr;
use strum::{EnumIs, EnumTryAs};

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, EnumIs, EnumTryAs, new,
)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", serde(untagged))]
#[cfg_attr(feature = "schemars", schemars(untagged))]
pub enum TeraExprBoolean {
    Boolean(bool),
    Expr(
        #[cfg_attr(
            feature = "serde",
            serde(deserialize_with = "validate_tera_expr")
        )]
        String,
    ),
}

impl From<bool> for TeraExprBoolean {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

impl From<String> for TeraExprBoolean {
    fn from(value: String) -> Self {
        Self::Expr(value)
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
