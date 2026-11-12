//! Cloudflare D1 `QueryExecutor` and the Workers `HostRuntime`. Empty on
//! native targets, where `omni_remote_cache_repository_sql` is used.
#![cfg(target_arch = "wasm32")]

use std::time::Duration;

use async_trait::async_trait;
use omni_remote_cache_core::hooks::{BoxTask, HostRuntime};
use omni_remote_cache_repository_query::{QueryError, QueryExecutor};
use send_wrapper::SendWrapper;
use serde_json::Value;
use worker::{
    Context, D1Database, Delay, send::SendFuture, wasm_bindgen::JsValue,
};

/// D1 handles are not `Send`. Workers are single-threaded, so they are held
/// in a `SendWrapper` and their futures in a `SendFuture`.
pub struct D1Executor {
    db: SendWrapper<D1Database>,
}

impl D1Executor {
    pub fn new(db: D1Database) -> Self {
        Self {
            db: SendWrapper::new(db),
        }
    }

    async fn rows(
        &self,
        sql: &str,
        params: &[&str],
    ) -> Result<Vec<Value>, QueryError> {
        SendFuture::new(async {
            // `sql` is operator configuration and is sent verbatim. Request
            // values reach D1 only as bound parameters.
            let args: Vec<JsValue> =
                params.iter().map(|p| JsValue::from_str(p)).collect();
            let statement = self
                .db
                .prepare(sql)
                .bind(&args)
                .map_err(|e| QueryError::Driver(e.to_string()))?;
            statement
                .all()
                .await
                .map_err(|e| QueryError::Driver(e.to_string()))?
                .results::<Value>()
                .map_err(|e| QueryError::Driver(e.to_string()))
        })
        .await
    }

    async fn single_row(
        &self,
        sql: &str,
        params: &[&str],
    ) -> Result<Option<Value>, QueryError> {
        let mut rows = self.rows(sql, params).await?;
        if rows.len() > 1 {
            return Err(QueryError::Shape(
                "expected at most one row".to_string(),
            ));
        }
        Ok(rows.pop())
    }
}

fn found(value: &Value) -> Result<bool, QueryError> {
    match value.get("found") {
        Some(Value::Bool(b)) => Ok(*b),
        Some(Value::Number(n)) if n.is_i64() || n.is_u64() => {
            Ok(n.as_i64() != Some(0) && n.as_u64() != Some(0))
        }
        Some(Value::Number(n))
            if n.as_f64().is_some_and(|f| f.fract() == 0.0) =>
        {
            Ok(n.as_f64() != Some(0.0))
        }
        other => Err(QueryError::Shape(format!(
            "`found` must be a boolean or an integer, got {other:?}"
        ))),
    }
}

#[async_trait]
impl QueryExecutor for D1Executor {
    async fn exists(
        &self,
        sql: &str,
        params: &[&str],
    ) -> Result<bool, QueryError> {
        match self.single_row(sql, params).await? {
            Some(row) => found(&row),
            None => Ok(false),
        }
    }

    async fn veto_reason(
        &self,
        sql: &str,
        params: &[&str],
    ) -> Result<Option<String>, QueryError> {
        match self.single_row(sql, params).await? {
            None => Ok(None),
            Some(row) => match row.get("veto_reason") {
                Some(Value::String(s)) => Ok(Some(s.clone())),
                Some(Value::Null) => Ok(None),
                other => Err(QueryError::Shape(format!(
                    "`veto_reason` must be text or NULL, got {other:?}"
                ))),
            },
        }
    }

    async fn execute(
        &self,
        sql: &str,
        params: &[&str],
    ) -> Result<(), QueryError> {
        self.rows(sql, params).await.map(|_| ())
    }
}

/// Runs `after` hooks past the response with `waitUntil`.
pub struct WaitUntilRuntime {
    ctx: SendWrapper<Context>,
}

impl WaitUntilRuntime {
    pub fn new(ctx: Context) -> Self {
        Self {
            ctx: SendWrapper::new(ctx),
        }
    }
}

impl HostRuntime for WaitUntilRuntime {
    fn spawn_detached(&self, task: BoxTask) {
        self.ctx.wait_until(task);
    }

    fn sleep(&self, duration: Duration) -> BoxTask {
        Box::pin(SendFuture::new(Delay::from(duration)))
    }
}
