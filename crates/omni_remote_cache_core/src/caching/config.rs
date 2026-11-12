use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One cache's settings. Caching is off unless a provider is wrapped with
/// `CachingProvider`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct CacheConfiguration {
    pub ttl_secs: u64,
    pub max_entries: usize,
    /// Cache `false` results for this many seconds. Absent means negative
    /// results are not cached, so a newly created entity is seen at once.
    #[serde(default)]
    pub negative_ttl_secs: Option<u64>,
}

impl CacheConfiguration {
    pub fn repositories_default() -> Self {
        Self {
            ttl_secs: 60,
            max_entries: 10_000,
            negative_ttl_secs: None,
        }
    }

    pub fn security_default() -> Self {
        Self {
            ttl_secs: 10,
            max_entries: 10_000,
            negative_ttl_secs: None,
        }
    }

    pub(crate) fn ttl(&self) -> Duration {
        Duration::from_secs(self.ttl_secs)
    }

    pub(crate) fn negative_ttl(&self) -> Option<Duration> {
        self.negative_ttl_secs.map(Duration::from_secs)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct CachingConfiguration {
    #[serde(default)]
    pub repositories: Option<CacheConfiguration>,
    #[serde(default)]
    pub security: Option<CacheConfiguration>,
}
