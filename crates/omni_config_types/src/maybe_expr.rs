use derive_new::new;
use either::Either;
use omni_serde_validators::tera_expr::validate_tera_expr;
use strum::{EnumIs, EnumTryAs};

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, EnumIs, EnumTryAs, new,
)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", serde(untagged))]
#[cfg_attr(feature = "schemars", schemars(untagged))]
pub enum MaybeExpr<L> {
    Value(#[new(into)] L),
    Expr(
        #[cfg_attr(
            feature = "serde",
            serde(deserialize_with = "validate_tera_expr")
        )]
        #[new(into)]
        String,
    ),
}

impl<T: std::fmt::Display> std::fmt::Display for MaybeExpr<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MaybeExpr::Value(v) => write!(f, "{}", v),
            MaybeExpr::Expr(e) => write!(f, "{}", e),
        }
    }
}

impl<L> MaybeExpr<L> {
    #[inline(always)]
    pub fn into_either(self) -> Either<L, String> {
        match self {
            MaybeExpr::Value(v) => Either::Left(v),
            MaybeExpr::Expr(e) => Either::Right(e),
        }
    }
}

impl<L> From<MaybeExpr<L>> for Either<L, String> {
    #[inline(always)]
    fn from(v: MaybeExpr<L>) -> Self {
        v.into_either()
    }
}

impl<L> From<Either<L, String>> for MaybeExpr<L> {
    #[inline(always)]
    fn from(e: Either<L, String>) -> Self {
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
        MaybeExpr::Expr(s)
    }
}

impl<L> From<&str> for MaybeExpr<L> {
    #[inline(always)]
    fn from(s: &str) -> Self {
        MaybeExpr::Expr(s.to_string())
    }
}

impl From<bool> for MaybeExpr<bool> {
    #[inline(always)]
    fn from(v: bool) -> Self {
        MaybeExpr::Value(v)
    }
}

impl From<f64> for MaybeExpr<f64> {
    #[inline(always)]
    fn from(v: f64) -> Self {
        MaybeExpr::Value(v)
    }
}

impl From<i64> for MaybeExpr<i64> {
    #[inline(always)]
    fn from(v: i64) -> Self {
        MaybeExpr::Value(v)
    }
}

impl From<i32> for MaybeExpr<i64> {
    #[inline(always)]
    fn from(v: i32) -> Self {
        MaybeExpr::Value(v as i64)
    }
}
