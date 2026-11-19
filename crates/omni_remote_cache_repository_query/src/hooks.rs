use std::sync::Arc;

use async_trait::async_trait;
use omni_remote_cache_core::hooks::{
    ArtifactHooks, HookContext, HookDecision, HookError, HookOutcome,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::QueryExecutor;

/// Number of parameters bound to the `before` query.
pub const BEFORE_PARAMS: usize = 11;

/// Number of parameters bound to the `after` query.
pub const AFTER_PARAMS: usize = 13;

/// Optional operator SQL for the hooks backend.
///
/// **Append-only contract:** a parameter's position never changes once it has
/// shipped, and new facts are only added at the end, so a query written for an
/// older list keeps its meaning.
///
/// Parameters are bound by position, as text. Every position that existed
/// before RFC 0030 keeps its meaning, and the new ones are appended, so a query
/// written for the older, shorter lists means what it did (`caller_id` is the
/// value that was called `api_key_id`). Postgres, MySQL and D1 require a query
/// to use exactly the bound count, so there an older query fails at startup
/// until it is extended or lists the new parameters; SQLite accepts a query that
/// uses fewer.
///
/// `before` (11): `$1 op, $2 tenant, $3 org, $4 ws, $5 env, $6 digest,
/// $7 caller_id, $8 credential_kind, $9 principal, $10 namespace, $11 scope`.
///
/// `after` (13): `$1 op, $2 tenant, $3 org, $4 ws, $5 env, $6 digest,
/// $7 outcome, $8 caller_id, $9 credential_kind, $10 principal, $11 namespace,
/// $12 scope, $13 sha256`.
///
/// `op` is one of `get`, `get_many`, `head`, `head_many`, `put` and `delete`.
/// `digest` is empty for list operations. `credential_kind` is `api_key` for an
/// API key. `namespace` is `shared` (or `isolated`). `scope` is empty when there
/// is none. `outcome` is `hit`, `miss`, `stored`, `already_exists`, `deleted` or
/// `failed`. `sha256` is the artifact's hash for a `stored` outcome and for a
/// `hit` on one artifact, otherwise empty.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema,
)]
pub struct HookQueriesConfiguration {
    /// Returns a row with a text `veto_reason` to veto, otherwise zero rows
    /// or NULL. Runs before the operation.
    #[serde(default)]
    pub before: Option<String>,

    /// A statement that records usage, for example an upsert of
    /// `last_read_at`. Runs after the operation, off the request path.
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
                params: BEFORE_PARAMS,
                column: Some("veto_reason"),
            });
        }
        if let Some(sql) = &self.after {
            specs.push(crate::QuerySpec {
                name: "hooks.after",
                sql,
                params: AFTER_PARAMS,
                column: None,
            });
        }
        specs
    }
}

/// `ArtifactHooks` backed by operator queries. An external service can read
/// what `after` records to prune old or unused entries.
pub struct QueryHooks {
    before_executor: Arc<dyn QueryExecutor>,
    after_executor: Arc<dyn QueryExecutor>,
    queries: HookQueriesConfiguration,
}

impl QueryHooks {
    /// `before` runs on the request path, so give it the same executor as
    /// authorization. `after` runs off it and can use a separate one, so slow
    /// usage writes cannot take connections from either.
    pub fn new(
        before_executor: Arc<dyn QueryExecutor>,
        after_executor: Arc<dyn QueryExecutor>,
        queries: HookQueriesConfiguration,
    ) -> Self {
        Self {
            before_executor,
            after_executor,
            queries,
        }
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
        let namespace = ctx.namespace.to_string();
        let scope = ctx.scope.as_deref().unwrap_or("");
        for digest in digests(ctx) {
            let reason = self
                .before_executor
                .veto_reason(
                    sql,
                    &[
                        &op,
                        &ctx.tenant,
                        &ctx.org,
                        &ctx.ws,
                        &ctx.env,
                        digest,
                        &ctx.caller_id,
                        ctx.credential_kind.as_str(),
                        &ctx.principal,
                        &namespace,
                        scope,
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
        let namespace = ctx.namespace.to_string();
        let scope = ctx.scope.as_deref().unwrap_or("");
        let sha256 = outcome.sha256.as_deref().unwrap_or("");
        let outcome: &'static str = outcome.kind.into();
        for digest in digests(ctx) {
            self.after_executor
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
                        &ctx.caller_id,
                        ctx.credential_kind.as_str(),
                        &ctx.principal,
                        &namespace,
                        scope,
                        sha256,
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

    use omni_remote_cache_core::{
        hooks::{ArtifactOp, NamespaceKind, OutcomeKind},
        security::CredentialKind,
    };

    use super::*;
    use crate::QueryError;

    #[derive(Default)]
    struct Fake {
        veto: Option<String>,
        calls: Mutex<Vec<(String, Vec<String>)>>,
    }

    #[async_trait]
    impl QueryExecutor for Fake {
        async fn api_key_identity(
            &self,
            _: &str,
            _: &[&str],
        ) -> Result<Option<crate::ApiKeyIdentity>, QueryError> {
            unreachable!("hook tests never authenticate")
        }

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
            caller_id: "kid".into(),
            credential_kind: CredentialKind::ApiKey,
            principal: "owner".into(),
            namespace: NamespaceKind::Shared,
            scope: None,
        }
    }

    fn hooks(f: Arc<Fake>) -> QueryHooks {
        QueryHooks::new(
            f.clone(),
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
            [
                "get", "t", "o", "w", "e", "d1", "kid", "api_key", "owner",
                "shared", ""
            ]
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
                    sha256: None,
                },
            )
            .await
            .unwrap();
        let calls = f.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(
            calls[1].1,
            [
                "get", "t", "o", "w", "e", "d2", "hit", "kid", "api_key",
                "owner", "shared", "", ""
            ]
        );
    }

    #[tokio::test]
    async fn the_new_facts_are_bound_after_the_ones_that_already_existed() {
        let mut ctx = ctx(ArtifactOp::Put, &["d1"]);
        ctx.credential_kind = CredentialKind::Custom("oidc".into());
        ctx.namespace = NamespaceKind::Isolated;
        ctx.scope = Some("pr-7".into());
        let f = Arc::new(Fake::default());
        let h = hooks(f.clone());
        h.before(&ctx).await.unwrap();
        h.after(
            &ctx,
            &HookOutcome {
                kind: OutcomeKind::Stored,
                sha256: Some("ab".repeat(32)),
            },
        )
        .await
        .unwrap();
        let calls = f.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            [
                "put", "t", "o", "w", "e", "d1", "kid", "oidc", "owner",
                "isolated", "pr-7"
            ]
        );
        assert_eq!(
            calls[1].1,
            [
                "put",
                "t",
                "o",
                "w",
                "e",
                "d1",
                "stored",
                "kid",
                "oidc",
                "owner",
                "isolated",
                "pr-7",
                &"ab".repeat(32)
            ]
        );
    }

    /// A query written before RFC 0030 binds the first seven (`before`) or
    /// eight (`after`) parameters. Keeping those positions means an older
    /// query that is still accepted (SQLite takes fewer) records the same
    /// things as before, instead of silently reading a new fact as an old one.
    #[tokio::test]
    async fn the_older_parameter_lists_are_a_prefix_of_the_new_ones() {
        let f = Arc::new(Fake::default());
        let h = hooks(f.clone());
        let ctx = ctx(ArtifactOp::Delete, &["d1"]);
        h.before(&ctx).await.unwrap();
        h.after(
            &ctx,
            &HookOutcome {
                kind: OutcomeKind::Deleted,
                sha256: None,
            },
        )
        .await
        .unwrap();
        let calls = f.calls.lock().unwrap();
        assert_eq!(
            calls[0].1[..7],
            ["delete", "t", "o", "w", "e", "d1", "kid"],
            "op, tenant, org, ws, env, digest, api_key_id"
        );
        assert_eq!(
            calls[1].1[..8],
            ["delete", "t", "o", "w", "e", "d1", "deleted", "kid"],
            "op, tenant, org, ws, env, digest, outcome, api_key_id"
        );
    }

    #[test]
    fn the_declared_counts_are_what_is_bound() {
        assert_eq!((BEFORE_PARAMS, AFTER_PARAMS), (11, 13));
        let queries = HookQueriesConfiguration {
            before: Some("B".into()),
            after: Some("A".into()),
        };
        let counts: Vec<_> = queries
            .specs()
            .iter()
            .map(|spec| (spec.name, spec.params))
            .collect();
        assert_eq!(counts, [("hooks.before", 11), ("hooks.after", 13)]);
    }

    #[tokio::test]
    async fn unset_queries_are_noops() {
        let f = Arc::new(Fake::default());
        let h = QueryHooks::new(f.clone(), f.clone(), Default::default());
        assert_eq!(
            h.before(&ctx(ArtifactOp::Put, &["d"])).await.unwrap(),
            HookDecision::Allow
        );
        assert!(f.calls.lock().unwrap().is_empty());
    }
}
