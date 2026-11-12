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
}
