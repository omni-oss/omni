#[cfg(feature = "serde")]
use derive_new::new;

#[cfg(feature = "serde")]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "schemars", schemars(transparent))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, new)]
pub struct TeraExpr(
    #[cfg_attr(
        feature = "serde",
        serde(
            deserialize_with = "omni_serde_validators::tera_expr::validate_tera_expr"
        )
    )]
    #[new(into)]
    String,
);

impl TeraExpr {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for TeraExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<TeraExpr> for String {
    fn from(value: TeraExpr) -> Self {
        value.0
    }
}

impl From<String> for TeraExpr {
    fn from(value: String) -> Self {
        TeraExpr(value)
    }
}

impl From<&str> for TeraExpr {
    fn from(value: &str) -> Self {
        TeraExpr(value.to_string())
    }
}

#[cfg(feature = "merge")]
impl merge::Merge for TeraExpr {
    fn merge(&mut self, other: Self) {
        *self = other;
    }
}
