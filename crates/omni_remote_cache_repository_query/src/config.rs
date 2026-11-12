use std::{sync::Arc, time::Duration};

use omni_remote_cache_core::{
    caching::{CachingConfiguration, CachingProvider},
    hooks::{Background, FailurePolicy, HookSettings},
    providers::DependencyProvider,
};
use serde::{Deserialize, Serialize};

use crate::{
    ApiKeyHashConfiguration, HookQueriesConfiguration, QueriesConfiguration,
    QueryDependencyProvider, QueryExecutor, QueryHooks,
};

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct HooksConfiguration {
    #[serde(default)]
    pub queries: HookQueriesConfiguration,
    /// Defaults to 2000.
    #[serde(default)]
    pub before_timeout_ms: Option<u64>,
    /// Defaults to 10000.
    #[serde(default)]
    pub after_timeout_ms: Option<u64>,
    /// What a failing or timed-out `before` hook does. Defaults to
    /// `fail_closed`.
    #[serde(default)]
    pub before_failure_policy: Option<FailurePolicy>,
}

impl HooksConfiguration {
    pub fn settings(&self) -> HookSettings {
        let d = HookSettings::default();
        HookSettings {
            before_timeout: self
                .before_timeout_ms
                .map_or(d.before_timeout, Duration::from_millis),
            after_timeout: self
                .after_timeout_ms
                .map_or(d.after_timeout, Duration::from_millis),
            before_policy: self
                .before_failure_policy
                .unwrap_or(d.before_policy),
        }
    }
}

/// Everything an operator configures for query-driven repositories. The
/// driver connection itself (SQL url or D1 binding) is configured by the
/// host.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct RepositoryConfiguration {
    pub queries: QueriesConfiguration,
    #[serde(default)]
    pub api_key_hash: ApiKeyHashConfiguration,
    #[serde(default)]
    pub hooks: HooksConfiguration,
    /// Absent means no caching.
    #[serde(default)]
    pub caching: Option<CachingConfiguration>,
}

impl RepositoryConfiguration {
    pub fn from_json(json: &str) -> eyre::Result<Self> {
        Ok(serde_json::from_str(json)?)
    }

    /// Builds the provider, with the optional caching decorator on top.
    pub fn build_provider(
        self,
        executor: Arc<dyn QueryExecutor>,
        background: Option<Arc<dyn Background>>,
    ) -> eyre::Result<Arc<dyn DependencyProvider>> {
        let hooks = Arc::new(QueryHooks::new(
            executor.clone(),
            self.hooks.queries.clone(),
        ));
        let mut provider = QueryDependencyProvider::new(
            executor,
            self.queries,
            self.api_key_hash,
        )?
        .with_hooks(hooks, self.hooks.settings());
        if let Some(bg) = background {
            provider = provider.with_background(bg);
        }
        let provider: Arc<dyn DependencyProvider> = Arc::new(provider);
        Ok(match &self.caching {
            Some(c) => Arc::new(CachingProvider::new(provider, c)),
            None => provider,
        })
    }
}
