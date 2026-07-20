//! [`RunnerPool`]: a lazily-spawned, keyed cache of [`BridgeServiceRunner`]s.
//!
//! This is a pure mechanism with no notion of capabilities, policies, or system
//! handles. A caller decides what a *key* means (e.g. `(runtime, policy
//! fingerprint)` for the generator subsystem) and supplies a **factory** that
//! knows how to spawn the runner for a key that is not cached yet. Everything
//! subsystem-specific — building a [`SpawnPolicy`](crate::SpawnPolicy), wrapping
//! a system handle in a broker, registering RPC services — lives inside that
//! factory closure, so the pool itself stays dependency-free and reusable by
//! every subsystem that drives JS/TS scripts through the bridge.
//!
//! ## Concurrency
//!
//! [`get_or_try_init`](RunnerPool::get_or_try_init) holds the pool lock across
//! the factory's `await`. This deliberately serializes spawns: two concurrent
//! callers requesting the same (or different) key never race to spawn duplicate
//! processes. Subsystems whose actions run sequentially (generators today) pay
//! nothing for this; the guarantee matters only under concurrent use.

use std::{collections::HashMap, future::Future, hash::Hash, sync::Arc};

use bridge_rpc_core::service::Service;
use bridge_rpc_router::Router;
use tokio::sync::Mutex;

use crate::BridgeServiceRunner;

/// A shared, lazily-spawned set of [`BridgeServiceRunner`]s keyed by an
/// arbitrary caller-defined `K`.
///
/// `S` is the service exposed *to* the JS process (the reverse RPC direction);
/// it defaults to [`Router`], matching [`BridgeServiceRunner`].
pub struct RunnerPool<K, S: Service = Router> {
    runners: Mutex<HashMap<K, Arc<BridgeServiceRunner<S>>>>,
}

impl<K, S: Service> Default for RunnerPool<K, S> {
    fn default() -> Self {
        Self {
            runners: Mutex::new(HashMap::new()),
        }
    }
}

impl<K, S: Service> std::fmt::Debug for RunnerPool<K, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunnerPool").finish_non_exhaustive()
    }
}

impl<K, S> RunnerPool<K, S>
where
    K: Eq + Hash + Clone + Send + Sync,
    S: Service,
{
    /// Creates an empty pool. Runners are spawned on first use via
    /// [`get_or_try_init`](Self::get_or_try_init).
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the cached runner for `key`, or builds one with `factory` and
    /// caches it before returning.
    ///
    /// The factory is only invoked on a cache miss. Its error type `E` is
    /// caller-defined, so the pool imposes no error vocabulary. The pool lock is
    /// held across the spawn (see the module docs), so a failed spawn leaves the
    /// key uncached and a subsequent call will retry.
    pub async fn get_or_try_init<Fut, E>(
        &self,
        key: K,
        factory: impl FnOnce() -> Fut,
    ) -> Result<Arc<BridgeServiceRunner<S>>, E>
    where
        Fut: Future<Output = Result<BridgeServiceRunner<S>, E>> + Send,
    {
        let mut runners = self.runners.lock().await;
        if let Some(runner) = runners.get(&key) {
            return Ok(runner.clone());
        }

        let runner = Arc::new(factory().await?);
        runners.insert(key, runner.clone());
        Ok(runner)
    }

    /// Shuts down every runner that was started, emptying the pool.
    /// Best-effort: individual shutdown failures are ignored.
    pub async fn shutdown(&self) {
        let runners = {
            let mut guard = self.runners.lock().await;
            std::mem::take(&mut *guard)
        };
        for (_, runner) in runners {
            let _ = runner.shutdown().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    // A failed spawn must leave the key uncached so a later call retries,
    // rather than caching (and repeatedly returning) a phantom failure. The
    // success/dedup path is covered by the live spawn e2e in `omni_generator`.
    #[tokio::test]
    async fn factory_error_is_propagated_and_not_cached() {
        let pool: RunnerPool<u32> = RunnerPool::new();
        let calls = AtomicUsize::new(0);

        let first = pool
            .get_or_try_init(1, || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err::<BridgeServiceRunner, _>("boom".to_string())
            })
            .await;
        assert_eq!(first.err().as_deref(), Some("boom"));

        let second = pool
            .get_or_try_init(1, || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err::<BridgeServiceRunner, _>("boom".to_string())
            })
            .await;
        assert!(second.is_err());

        // The factory ran on both calls: the error was never cached.
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
