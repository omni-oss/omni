use derive_new::new;
use either::Either;
use strum::{EnumIs, EnumTryAs};

use crate::TeraExpr;

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, EnumIs, EnumTryAs, new,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", serde(untagged))]
#[cfg_attr(feature = "schemars", schemars(untagged))]
pub enum MaybeExpr<L> {
    Expr(#[new(into)] TeraExpr),
    Value(#[new(into)] L),
}

#[cfg(feature = "serde")]
impl<'a, T: for<'de> serde::Deserialize<'de>> serde::Deserialize<'a>
    for MaybeExpr<T>
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'a>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;

        match value {
            serde_json::Value::String(s) => {
                // Validate the string as a Tera expression
                omni_serde_validators::tera_expr::validate_str(&s)
                    .map_err(serde::de::Error::custom)?;
                Ok(MaybeExpr::new_expr(s))
            }
            value => Ok(serde_path_to_error::deserialize(value)
                .map(MaybeExpr::Value)
                .map_err(serde::de::Error::custom)?),
        }
    }
}

impl<T: std::fmt::Display> std::fmt::Display for MaybeExpr<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MaybeExpr::Value(v) => v.fmt(f),
            MaybeExpr::Expr(e) => e.fmt(f),
        }
    }
}

impl<L> MaybeExpr<L> {
    #[inline(always)]
    pub fn into_either(self) -> Either<L, TeraExpr> {
        match self {
            MaybeExpr::Value(v) => Either::Left(v),
            MaybeExpr::Expr(e) => Either::Right(e),
        }
    }
}

impl<L> From<MaybeExpr<L>> for Either<L, TeraExpr> {
    #[inline(always)]
    fn from(v: MaybeExpr<L>) -> Self {
        v.into_either()
    }
}

impl<L> From<Either<L, TeraExpr>> for MaybeExpr<L> {
    #[inline(always)]
    fn from(e: Either<L, TeraExpr>) -> Self {
        match e {
            Either::Left(v) => MaybeExpr::Value(v),
            Either::Right(s) => MaybeExpr::Expr(s),
        }
    }
}

// Strings are always treated as Tera expressions (never overlap with concrete
// bool/i64/f64 variants below).
impl<L> From<String> for MaybeExpr<L> {
    #[inline(always)]
    fn from(s: String) -> Self {
        MaybeExpr::new_expr(s)
    }
}

impl<L> From<&str> for MaybeExpr<L> {
    #[inline(always)]
    fn from(s: &str) -> Self {
        MaybeExpr::new_expr(s)
    }
}

impl From<bool> for MaybeExpr<bool> {
    #[inline(always)]
    fn from(v: bool) -> Self {
        MaybeExpr::new_value(v)
    }
}

impl From<f64> for MaybeExpr<f64> {
    #[inline(always)]
    fn from(v: f64) -> Self {
        MaybeExpr::new_value(v)
    }
}

impl From<i64> for MaybeExpr<i64> {
    #[inline(always)]
    fn from(v: i64) -> Self {
        MaybeExpr::new_value(v)
    }
}

impl From<i32> for MaybeExpr<i64> {
    #[inline(always)]
    fn from(v: i32) -> Self {
        MaybeExpr::new_value(v as i64)
    }
}
