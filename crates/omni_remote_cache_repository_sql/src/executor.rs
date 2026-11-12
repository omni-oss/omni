use std::time::Duration;

use async_trait::async_trait;
use futures::TryStreamExt as _;
use omni_remote_cache_repository_query::{QueryError, QueryExecutor};
use sqlx::{AnyPool, Row as _, any::AnyPoolOptions};

#[derive(Clone)]
pub struct SqlxExecutor {
    pool: AnyPool,
}

impl SqlxExecutor {
    /// Connects to `url`. The scheme (`sqlite:`, `postgres:`, `mysql:`)
    /// selects the driver, which must be enabled by a cargo feature.
    pub async fn connect(
        url: &str,
        max_connections: u32,
        acquire_timeout: Duration,
    ) -> Result<Self, QueryError> {
        sqlx::any::install_default_drivers();
        let pool = AnyPoolOptions::new()
            .max_connections(max_connections)
            .acquire_timeout(acquire_timeout)
            .connect(url)
            .await
            .map_err(|e| QueryError::Driver(e.to_string()))?;
        Ok(Self { pool })
    }

    pub fn from_pool(pool: AnyPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl QueryExecutor for SqlxExecutor {
    async fn exists(
        &self,
        sql: &str,
        params: &[&str],
    ) -> Result<bool, QueryError> {
        // `sql` comes from operator configuration, never from a request, so
        // it is audited here, in the only place a query string is built.
        // Request values reach the database only through `bind`.
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()));
        for p in params {
            query = query.bind((*p).to_owned());
        }

        let mut rows = query.fetch(&self.pool);
        let first = rows
            .try_next()
            .await
            .map_err(|e| QueryError::Driver(e.to_string()))?;
        let Some(row) = first else {
            return Ok(false);
        };
        if rows
            .try_next()
            .await
            .map_err(|e| QueryError::Driver(e.to_string()))?
            .is_some()
        {
            return Err(QueryError::Shape(
                "expected at most one row".to_string(),
            ));
        }

        match row.try_get::<i64, _>("found") {
            Ok(v) => Ok(v != 0),
            Err(_) => row.try_get::<bool, _>("found").map_err(|e| {
                QueryError::Shape(format!(
                    "`found` must be a boolean or an integer: {e}"
                ))
            }),
        }
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;

    async fn db() -> SqlxExecutor {
        let e =
            SqlxExecutor::connect("sqlite::memory:", 1, Duration::from_secs(5))
                .await
                .unwrap();
        for ddl in [
            "CREATE TABLE t(code TEXT, deleted INTEGER, label TEXT)",
            "INSERT INTO t VALUES ('a',0,'x'),('b',1,'y'),('a',0,'dup')",
            "INSERT INTO t VALUES ('n',0,NULL)",
        ] {
            sqlx::query(sqlx::AssertSqlSafe(ddl.to_string()))
                .execute(&e.pool)
                .await
                .unwrap();
        }
        e
    }

    #[tokio::test]
    async fn found_accepts_integer_and_boolean() {
        let e = db().await;
        let int =
            "SELECT 1 AS found FROM t WHERE code = ? AND deleted = 0 LIMIT 1";
        assert!(e.exists(int, &["a"]).await.unwrap());
        assert!(!e.exists(int, &["b"]).await.unwrap());
        assert!(!e.exists(int, &["zzz"]).await.unwrap());

        let boolean =
            "SELECT (deleted = 0) AS found FROM t WHERE code = ?1 LIMIT 1";
        assert!(e.exists(boolean, &["a"]).await.unwrap());
        assert!(!e.exists(boolean, &["b"]).await.unwrap());
    }

    #[tokio::test]
    async fn wrong_shapes_are_errors() {
        let e = db().await;
        let r = e.exists("SELECT 1 AS nope", &[]).await;
        assert!(matches!(r, Err(QueryError::Shape(_))), "{r:?}");

        let null = e.exists("SELECT NULL AS found", &[]).await;
        assert!(matches!(null, Err(QueryError::Shape(_))), "{null:?}");

        let text = e.exists("SELECT 'yes' AS found", &[]).await;
        assert!(matches!(text, Err(QueryError::Shape(_))), "{text:?}");

        let many = e
            .exists("SELECT 1 AS found FROM t WHERE code = ?", &["a"])
            .await;
        assert!(matches!(many, Err(QueryError::Shape(_))), "{many:?}");
    }

    #[tokio::test]
    async fn driver_errors_are_reported() {
        let e = db().await;
        let r = e.exists("SELECT 1 AS found FROM missing_table", &[]).await;
        assert!(matches!(r, Err(QueryError::Driver(_))), "{r:?}");
    }

    #[tokio::test]
    async fn injection_strings_are_inert_bound_values() {
        let e = db().await;
        let q = "SELECT 1 AS found FROM t WHERE code = ? LIMIT 1";
        assert!(!e.exists(q, &["a' OR '1'='1"]).await.unwrap());
        assert!(!e.exists(q, &["a'; DROP TABLE t; --"]).await.unwrap());
        assert!(e.exists(q, &["a"]).await.unwrap(), "table still exists");
    }
}

/// Server-database tests run only when a URL is provided, for example
/// `TEST_POSTGRES_URL=postgres://postgres:p@127.0.0.1:5432/postgres`.
#[cfg(test)]
mod server_tests {
    use super::*;

    async fn check(url_var: &str, int_q: &str, bool_q: &str) {
        let Ok(url) = std::env::var(url_var) else {
            return;
        };
        let e = SqlxExecutor::connect(&url, 1, Duration::from_secs(10))
            .await
            .unwrap();
        assert!(e.exists(int_q, &["a"]).await.unwrap());
        assert!(!e.exists(int_q, &["zzz"]).await.unwrap());
        assert!(e.exists(bool_q, &["a"]).await.unwrap());
    }

    #[tokio::test]
    async fn postgres() {
        check(
            "TEST_POSTGRES_URL",
            "SELECT 1::bigint AS found WHERE $1 = 'a'",
            "SELECT ($1 = 'a') AS found",
        )
        .await;
    }

    #[tokio::test]
    async fn mysql() {
        check(
            "TEST_MYSQL_URL",
            "SELECT CAST(1 AS SIGNED) AS found FROM DUAL WHERE ? = 'a'",
            "SELECT (? = 'a') AS found",
        )
        .await;
    }
}
