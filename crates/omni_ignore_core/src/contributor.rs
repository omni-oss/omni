use crate::pattern::IgnorePattern;

/// One named source's patterns, emitted in significant order.
pub struct Contribution {
    pub name: &'static str,
    pub patterns: Vec<IgnorePattern>,
}

/// A source of ignore patterns. `patterns` is async because gathering can need
/// IO (the projections contributor reads the ledger). Implementors capture what
/// they need at construction, so the trait stays object-safe and enabled
/// contributors can be held as `Vec<Box<dyn IgnoreContributor>>`.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait IgnoreContributor: Send + Sync {
    fn name(&self) -> &'static str;
    async fn patterns(&self) -> eyre::Result<Vec<IgnorePattern>>;
}
