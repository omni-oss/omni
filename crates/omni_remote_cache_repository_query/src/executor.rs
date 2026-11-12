use async_trait::async_trait;

#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("query failed: {0}")]
    Driver(String),

    /// The query ran but did not return the documented shape.
    #[error("query returned the wrong shape: {0}")]
    Shape(String),
}

/// Runs an operator-supplied query and reads the `found` contract.
///
/// Implementations must send `sql` to the driver verbatim, bind `params` in
/// order as text, and never splice a parameter into the SQL text. The query
/// returns zero or one row. A row has a `found` column that is a boolean or
/// an integer (non-zero is true); any other type, including NULL, is
/// `QueryError::Shape`. More than one row is also a shape error. Zero rows
/// means `false`.
#[async_trait]
pub trait QueryExecutor: Send + Sync + 'static {
    async fn exists(
        &self,
        sql: &str,
        params: &[&str],
    ) -> Result<bool, QueryError>;

    /// Runs a hook veto query. Zero rows, or a NULL `veto_reason`, means
    /// allow. A row with a text `veto_reason` vetoes with that reason. More
    /// than one row, or a non-text reason, is `QueryError::Shape`.
    async fn veto_reason(
        &self,
        sql: &str,
        params: &[&str],
    ) -> Result<Option<String>, QueryError>;

    /// Runs a statement (for example an upsert of usage) and ignores any
    /// rows. Same verbatim-SQL and ordered-binding rules as `exists`.
    async fn execute(
        &self,
        sql: &str,
        params: &[&str],
    ) -> Result<(), QueryError>;

    /// Checks at startup, where the driver can describe the query, that it
    /// prepares, needs `params` parameters and returns `column`. Drivers
    /// that cannot describe a query keep this default, and a wrong query
    /// then fails on first use.
    async fn validate(
        &self,
        _sql: &str,
        _params: usize,
        _column: Option<&str>,
    ) -> Result<(), QueryError> {
        Ok(())
    }
}
