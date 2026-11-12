use std::{collections::HashMap, sync::Mutex, time::Duration};

#[cfg(target_arch = "wasm32")]
use time::OffsetDateTime;

use super::CacheConfiguration;

pub type Clock = fn() -> i128;

/// Monotonic where the platform has one, so a backward wall-clock step cannot
/// extend a cached authorization result. `Instant` panics on wasm32, which
/// falls back to the wall clock.
#[cfg(not(target_arch = "wasm32"))]
fn system_clock() -> i128 {
    use std::{sync::OnceLock, time::Instant};

    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_nanos() as i128
}

#[cfg(target_arch = "wasm32")]
fn system_clock() -> i128 {
    OffsetDateTime::now_utc().unix_timestamp_nanos()
}

struct Entry<V> {
    value: V,
    expires_at: i128,
}

/// A bounded TTL cache of lookups. Runtime-agnostic: no timers, the
/// clock is only read on access.
pub struct TtlCache<V = bool> {
    config: CacheConfiguration,
    clock: Clock,
    entries: Mutex<HashMap<String, Entry<V>>>,
}

impl<V: Clone> TtlCache<V> {
    pub fn new(config: CacheConfiguration) -> Self {
        Self::with_clock(config, system_clock)
    }

    pub fn with_clock(config: CacheConfiguration, clock: Clock) -> Self {
        Self {
            config,
            clock,
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry<V>>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get(&self, key: &str) -> Option<V> {
        let now = (self.clock)();
        let mut entries = self.lock();
        match entries.get(key) {
            Some(e) if e.expires_at > now => Some(e.value.clone()),
            Some(_) => {
                entries.remove(key);
                None
            }
            None => None,
        }
    }

    /// `positive` picks the TTL: negative results use the negative TTL, and
    /// are not cached when none is configured.
    pub fn put(&self, key: String, value: V, positive: bool) {
        let ttl: Duration = if positive {
            self.config.ttl()
        } else {
            match self.config.negative_ttl() {
                Some(t) => t,
                None => return,
            }
        };
        if ttl.is_zero() || self.config.max_entries == 0 {
            return;
        }
        let now = (self.clock)();
        let mut entries = self.lock();
        if entries.len() >= self.config.max_entries
            && !entries.contains_key(&key)
        {
            entries.retain(|_, e| e.expires_at > now);
            if entries.len() >= self.config.max_entries {
                // Still full: drop the entry closest to expiry.
                if let Some(oldest) = entries
                    .iter()
                    .min_by_key(|(_, e)| e.expires_at)
                    .map(|(k, _)| k.clone())
                {
                    entries.remove(&oldest);
                }
            }
        }
        entries.insert(
            key,
            Entry {
                value,
                expires_at: now + ttl.as_nanos() as i128,
            },
        );
    }

    /// Returns the cached value or computes, stores and returns it. Errors
    /// pass through and are never stored.
    pub async fn get_or_try<E>(
        &self,
        key: String,
        compute: impl std::future::Future<Output = Result<V, E>>,
        positive: impl Fn(&V) -> bool,
    ) -> Result<V, E> {
        if let Some(v) = self.get(&key) {
            return Ok(v);
        }
        let v = compute.await?;
        self.put(key, v.clone(), positive(&v));
        Ok(v)
    }
}

/// Builds a collision-free key from parts (length-prefixed).
pub fn cache_key(parts: &[&str]) -> String {
    let mut k = String::new();
    for p in parts {
        k.push_str(&p.len().to_string());
        k.push(':');
        k.push_str(p);
    }
    k
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

    use super::*;

    static NOW: AtomicI64 = AtomicI64::new(0);
    fn clock() -> i128 {
        NOW.load(Ordering::SeqCst) as i128 * 1_000_000_000
    }
    fn cfg(ttl: u64, max: usize, neg: Option<u64>) -> CacheConfiguration {
        CacheConfiguration {
            ttl_secs: ttl,
            max_entries: max,
            negative_ttl_secs: neg,
        }
    }

    // One test: the clock is a process-wide static.
    #[tokio::test]
    async fn ttl_size_errors_and_negative_results() {
        NOW.store(0, Ordering::SeqCst);
        let calls = AtomicUsize::new(0);
        let c = TtlCache::with_clock(cfg(10, 2, None), clock);
        let calls_ref = &calls;
        let call = |v: Result<bool, ()>| async move {
            calls_ref.fetch_add(1, Ordering::SeqCst);
            v
        };

        assert_eq!(
            c.get_or_try("a".into(), call(Ok(true)), |v| *v).await,
            Ok(true)
        );
        assert_eq!(
            c.get_or_try("a".into(), call(Ok(true)), |v| *v).await,
            Ok(true)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(c.len(), 1);

        // expiry: revocation is bounded by the TTL
        assert_eq!(c.get("a"), Some(true));
        NOW.store(10, Ordering::SeqCst);
        assert_eq!(c.get("a"), None);

        // negatives not cached by default
        c.put("n".into(), false, false);
        assert_eq!(c.get("n"), None);

        // errors never cached
        assert_eq!(
            c.get_or_try("e".into(), call(Err(())), |v| *v).await,
            Err(())
        );
        assert_eq!(c.get("e"), None);

        // max entries
        c.put("1".into(), true, true);
        c.put("2".into(), true, true);
        c.put("3".into(), true, true);
        assert_eq!(c.len(), 2);

        // negative ttl
        let c = TtlCache::with_clock(cfg(10, 5, Some(1)), clock);
        c.put("n".into(), false, false);
        assert_eq!(c.get("n"), Some(false));
        NOW.store(11, Ordering::SeqCst);
        assert_eq!(c.get("n"), None);
    }

    #[test]
    fn keys_do_not_collide() {
        assert_ne!(cache_key(&["a:b", "c"]), cache_key(&["a", "b:c"]));
    }
}
