use std::sync::Arc;

use async_trait::async_trait;
use omni_remote_cache_core::hooks::{
    ArtifactHooks, HookContext, HookDecision, HookError, HookOutcome,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::QueryExecutor;

/// Optional operator SQL for the hooks backend. Both params lists start with
/// `op, tenant, org, ws, env, digest`, where `digest` is empty for list
/// operations, and end with `api_key_id`, an opaque fingerprint of the key
/// (see `omni_remote_cache_core::hooks::api_key_id`). `op` is one of `get`, `get_many`, `head`, `head_many`, `put`
/// and `delete`.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema,
)]
pub struct HookQueriesConfiguration {
    /// Returns a row with a text `veto_reason` to veto, otherwise zero rows
    /// or NULL. Runs before the operation.
    /// params: op, tenant, org, ws, env, digest, api_key_id
    #[serde(default)]
    pub before: Option<String>,

    /// A statement that records usage, for example an upsert of
    /// `last_read_at`. Runs after the operation, off the request path.
    /// params: op, tenant, org, ws, env, digest, outcome, api_key_id
    /// (`hit`, `miss`, `stored`, `deleted`, `failed`)
    #[serde(default)]
    pub after: Option<String>,
}

impl HookQueriesConfiguration {
    pub fn specs(&self) -> Vec<crate::QuerySpec<'_>> {
        let mut specs = Vec::new();
        if let Some(sql) = &self.before {
            specs.push(crate::QuerySpec {
                name: "hooks.before",
                sql,
                params: 7,
                column: Some("veto_reason"),
            });
        }
        if let Some(sql) = &self.after {
            specs.push(crate::QuerySpec {
                name: "hooks.after",
                sql,
                params: 8,
                column: None,
            });
        }
        specs
    }
}

/// `ArtifactHooks` backed by operator queries. An external service can read
/// what `after` records to prune old or unused entries.
pub struct QueryHooks {
    executor: Arc<dyn QueryExecutor>,
    queries: HookQueriesConfiguration,
}

impl QueryHooks {
    pub fn new(
        executor: Arc<dyn QueryExecutor>,
        queries: HookQueriesConfiguration,
    ) -> Self {
        Self { executor, queries }
    }
}

fn digests(ctx: &HookContext) -> Vec<&str> {
    if ctx.digests.is_empty() {
        vec![""]
    } else {
        ctx.digests.iter().map(String::as_str).collect()
    }
}

#[async_trait]
impl ArtifactHooks for QueryHooks {
    async fn before(
        &self,
        ctx: &HookContext,
    ) -> Result<HookDecision, HookError> {
        let Some(sql) = &self.queries.before else {
            return Ok(HookDecision::Allow);
        };
        let op = ctx.op.to_string();
        for digest in digests(ctx) {
            let reason = self
                .executor
                .veto_reason(
                    sql,
                    &[
                        &op,
                        &ctx.tenant,
                        &ctx.org,
                        &ctx.ws,
                        &ctx.env,
                        digest,
                        &ctx.api_key_id,
                    ],
                )
                .await
                .map_err(|e| HookError(e.into()))?;
            if let Some(reason) = reason {
                return Ok(HookDecision::Veto { reason });
            }
        }
        Ok(HookDecision::Allow)
    }

    async fn after(
        &self,
        ctx: &HookContext,
        outcome: &HookOutcome,
    ) -> Result<(), HookError> {
        let Some(sql) = &self.queries.after else {
            return Ok(());
        };
        let op = ctx.op.to_string();
        let outcome: &'static str = outcome.kind.into();
        for digest in digests(ctx) {
            self.executor
                .execute(
                    sql,
                    &[
                        &op,
                        &ctx.tenant,
                        &ctx.org,
                        &ctx.ws,
                        &ctx.env,
                        digest,
                        outcome,
                        &ctx.api_key_id,
                    ],
                )
                .await
                .map_err(|e| HookError(e.into()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use omni_remote_cache_core::hooks::{ArtifactOp, OutcomeKind};

    use super::*;
    use crate::QueryError;

    #[derive(Default)]
    struct Fake {
        veto: Option<String>,
        calls: Mutex<Vec<(String, Vec<String>)>>,
    }

    #[async_trait]
    impl QueryExecutor for Fake {
        async fn exists(
            &self,
            _: &str,
            _: &[&str],
        ) -> Result<bool, QueryError> {
            Ok(true)
        }
        async fn veto_reason(
            &self,
            sql: &str,
            p: &[&str],
        ) -> Result<Option<String>, QueryError> {
            self.calls
                .lock()
                .unwrap()
                .push((sql.into(), p.iter().map(|s| s.to_string()).collect()));
            Ok(self.veto.clone())
        }
        async fn execute(
            &self,
            sql: &str,
            p: &[&str],
        ) -> Result<(), QueryError> {
            self.calls
                .lock()
                .unwrap()
                .push((sql.into(), p.iter().map(|s| s.to_string()).collect()));
            Ok(())
        }
    }

    fn ctx(op: ArtifactOp, digests: &[&str]) -> HookContext {
        HookContext {
            op,
            tenant: "t".into(),
            org: "o".into(),
            ws: "w".into(),
            env: "e".into(),
            digests: digests.iter().map(|d| d.to_string()).collect(),
            api_key_id: "kid".into(),
        }
    }

    fn hooks(f: Arc<Fake>) -> QueryHooks {
        QueryHooks::new(
            f,
            HookQueriesConfiguration {
                before: Some("B".into()),
                after: Some("A".into()),
            },
        )
    }

    #[tokio::test]
    async fn veto_and_allow() {
        let f = Arc::new(Fake {
            veto: Some("nope".into()),
            ..Default::default()
        });
        let d = hooks(f.clone())
            .before(&ctx(ArtifactOp::Get, &["d1"]))
            .await
            .unwrap();
        assert_eq!(
            d,
            HookDecision::Veto {
                reason: "nope".into()
            }
        );
        assert_eq!(
            f.calls.lock().unwrap()[0].1,
            ["get", "t", "o", "w", "e", "d1", "kid"]
        );

        let f = Arc::new(Fake::default());
        let d = hooks(f)
            .before(&ctx(ArtifactOp::GetMany, &[]))
            .await
            .unwrap();
        assert_eq!(d, HookDecision::Allow);
    }

    #[tokio::test]
    async fn after_records_each_digest_with_outcome() {
        let f = Arc::new(Fake::default());
        hooks(f.clone())
            .after(
                &ctx(ArtifactOp::Get, &["d1", "d2"]),
                &HookOutcome {
                    kind: OutcomeKind::Hit,
                },
            )
            .await
            .unwrap();
        let calls = f.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].1, ["get", "t", "o", "w", "e", "d2", "hit", "kid"]);
    }

    #[tokio::test]
    async fn unset_queries_are_noops() {
        let f = Arc::new(Fake::default());
        let h = QueryHooks::new(f.clone(), Default::default());
        assert_eq!(
            h.before(&ctx(ArtifactOp::Put, &["d"])).await.unwrap(),
            HookDecision::Allow
        );
        assert!(f.calls.lock().unwrap().is_empty());
    }
}
