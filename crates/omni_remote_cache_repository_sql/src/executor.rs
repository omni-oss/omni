use std::time::Duration;

use async_trait::async_trait;
use futures::TryStreamExt as _;
use omni_remote_cache_repository_query::{QueryError, QueryExecutor};
use sqlx::{AnyPool, Either, Row as _, any::AnyPoolOptions};

#[derive(Clone)]
pub struct SqlxExecutor {
    pool: AnyPool,
    /// SQLite reports the highest parameter index a query uses and accepts
    /// extra bound values, so fewer is fine there.
    lenient_params: bool,
    query_timeout: Duration,
}

const DEFAULT_QUERY_TIMEOUT: Duration = Duration::from_secs(10);

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
        let mut executor = Self::from_pool(pool);
        executor.lenient_params = url.starts_with("sqlite");
        Ok(executor)
    }

    pub fn from_pool(pool: AnyPool) -> Self {
        Self {
            pool,
            lenient_params: false,
            query_timeout: DEFAULT_QUERY_TIMEOUT,
        }
    }

    /// Bounds each statement, so a stalled database cannot hold pool
    /// connections indefinitely. `acquire_timeout` only bounds acquiring.
    pub fn with_query_timeout(mut self, timeout: Duration) -> Self {
        self.query_timeout = timeout;
        self
    }

    async fn bounded<T>(
        &self,
        fut: impl std::future::Future<Output = Result<T, QueryError>>,
    ) -> Result<T, QueryError> {
        tokio::time::timeout(self.query_timeout, fut)
            .await
            .map_err(|_| {
                QueryError::Driver(format!(
                    "query timed out after {:?}",
                    self.query_timeout
                ))
            })?
    }
}

fn driver(e: sqlx::Error) -> QueryError {
    QueryError::Driver(e.to_string())
}

impl SqlxExecutor {
    // `sql` comes from operator configuration, never from a request, so it
    // is audited here, in the only place a query string is built. Request
    // values reach the database only through `bind`.
    fn build<'q>(
        sql: &str,
        params: &[&str],
    ) -> sqlx::query::Query<'q, sqlx::Any, sqlx::any::AnyArguments> {
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()));
        for p in params {
            query = query.bind((*p).to_owned());
        }
        query
    }

    /// Returns the single row, `None` for zero rows, or a shape error for
    /// more than one.
    async fn single_row(
        &self,
        sql: &str,
        params: &[&str],
    ) -> Result<Option<sqlx::any::AnyRow>, QueryError> {
        let mut rows = Self::build(sql, params).fetch(&self.pool);
        let Some(row) = rows.try_next().await.map_err(driver)? else {
            return Ok(None);
        };
        if rows.try_next().await.map_err(driver)?.is_some() {
            return Err(QueryError::Shape(
                "expected at most one row".to_string(),
            ));
        }
        Ok(Some(row))
    }
}

#[async_trait]
impl QueryExecutor for SqlxExecutor {
    async fn exists(
        &self,
        sql: &str,
        params: &[&str],
    ) -> Result<bool, QueryError> {
        let Some(row) = self.bounded(self.single_row(sql, params)).await?
        else {
            return Ok(false);
        };
        match row.try_get::<i64, _>("found") {
            Ok(v) => Ok(v != 0),
            Err(_) => row.try_get::<bool, _>("found").map_err(|e| {
                QueryError::Shape(format!(
                    "`found` must be a boolean or an integer: {e}"
                ))
            }),
        }
    }

    async fn veto_reason(
        &self,
        sql: &str,
        params: &[&str],
    ) -> Result<Option<String>, QueryError> {
        let Some(row) = self.bounded(self.single_row(sql, params)).await?
        else {
            return Ok(None);
        };
        row.try_get::<Option<String>, _>("veto_reason")
            .map_err(|e| {
                QueryError::Shape(format!(
                    "`veto_reason` must be text or NULL: {e}"
                ))
            })
    }

    async fn validate(
        &self,
        sql: &str,
        params: usize,
        column: Option<&str>,
    ) -> Result<(), QueryError> {
        use sqlx::{
            Column as _, Executor as _, SqlSafeStr as _, Statement as _,
        };

        // Operator configuration, never request data (see `build`).
        let statement = self
            .bounded(async {
                self.pool
                    .prepare(sqlx::AssertSqlSafe(sql.to_owned()).into_sql_str())
                    .await
                    .map_err(driver)
            })
            .await?;

        let needed = statement.parameters().map(|p| match p {
            Either::Left(types) => types.len(),
            Either::Right(count) => count,
        });
        if let Some(needed) = needed {
            let ok = if self.lenient_params {
                needed <= params
            } else {
                needed == params
            };
            if !ok {
                return Err(QueryError::Shape(format!(
                    "the query uses {needed} parameter(s) but {params} are bound"
                )));
            }
        }
        if let Some(column) = column
            && !statement
                .columns()
                .iter()
                .any(|c| c.name().eq_ignore_ascii_case(column))
        {
            return Err(QueryError::Shape(format!(
                "the query must return a `{column}` column"
            )));
        }
        Ok(())
    }

    async fn execute(
        &self,
        sql: &str,
        params: &[&str],
    ) -> Result<(), QueryError> {
        self.bounded(async {
            Self::build(sql, params)
                .execute(&self.pool)
                .await
                .map(|_| ())
                .map_err(driver)
        })
        .await
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
    async fn veto_reason_and_execute() {
        let e = db().await;
        let q = "SELECT label AS veto_reason FROM t WHERE code = ? AND deleted = 0 AND label <> 'dup'";
        assert_eq!(e.veto_reason(q, &["a"]).await.unwrap(), Some("x".into()));
        assert_eq!(e.veto_reason(q, &["zzz"]).await.unwrap(), None);
        assert_eq!(
            e.veto_reason(q, &["n"]).await.unwrap(),
            None,
            "NULL allows"
        );
        let many = e
            .veto_reason(
                "SELECT label AS veto_reason FROM t WHERE code = ?",
                &["a"],
            )
            .await;
        assert!(matches!(many, Err(QueryError::Shape(_))));

        e.execute("INSERT INTO t VALUES (?, 0, ?)", &["new", "l"])
            .await
            .unwrap();
        assert!(
            e.exists("SELECT 1 AS found FROM t WHERE code = ?", &["new"])
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn statements_are_bounded_by_the_query_timeout() {
        let e = db().await.with_query_timeout(Duration::from_millis(50));
        // A recursive CTE that runs far longer than the deadline.
        let r = e
            .exists(
                "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 2000000000) SELECT (MAX(x) > 0) AS found FROM c",
                &[],
            )
            .await;
        assert!(
            matches!(&r, Err(QueryError::Driver(m)) if m.contains("timed out")),
            "{r:?}"
        );
    }

    #[tokio::test]
    async fn startup_validation_describes_queries() {
        let e = db().await;
        let ok = "SELECT 1 AS found FROM t WHERE code = ?1 AND label = ?2";
        e.validate(ok, 2, Some("found")).await.unwrap();
        // SQLite accepts extra bound values
        e.validate("SELECT 1 AS found WHERE ?1 = 'a'", 2, Some("found"))
            .await
            .unwrap();

        let missing = e.validate("SELECT 1 AS nope", 0, Some("found")).await;
        assert!(matches!(missing, Err(QueryError::Shape(_))), "{missing:?}");
        let more = e.validate(ok, 1, Some("found")).await;
        assert!(matches!(more, Err(QueryError::Shape(_))), "{more:?}");
        let bad = e.validate("SELEC nonsense", 0, None).await;
        assert!(matches!(bad, Err(QueryError::Driver(_))), "{bad:?}");
        e.validate("INSERT INTO t VALUES (?1, 0, ?2)", 2, None)
            .await
            .unwrap();
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

        // startup validation describes the query on a real server too
        e.validate(int_q, 1, Some("found")).await.unwrap();
        let wrong_count = e.validate(int_q, 2, Some("found")).await;
        assert!(
            matches!(wrong_count, Err(QueryError::Shape(_))),
            "{wrong_count:?}"
        );
        let wrong_col = e.validate(int_q, 1, Some("veto_reason")).await;
        assert!(
            matches!(wrong_col, Err(QueryError::Shape(_))),
            "{wrong_col:?}"
        );
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
