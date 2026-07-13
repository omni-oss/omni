//! Process-wide memoization of compiled [`GlobSet`]s.
//!
//! Compiling a set of glob patterns is expensive: every pattern is parsed and
//! translated into a regex, and the combined set is compiled into a matching
//! automaton. Several hot paths (task input/output collection, project and
//! workspace hashing, task filtering) re-derive the *same* pattern sets on
//! every invocation, so the compilation cost is paid over and over for an
//! identical result.
//!
//! A compiled [`GlobSet`] is a pure function of its ordered pattern strings and
//! is immutable once built, which makes it trivially memoizable with no
//! invalidation. [`build_glob_set`] returns a shared, cached [`GlobSet`] keyed
//! by the exact ordered patterns.

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, RwLock},
};

use globset::{Glob, GlobSet, GlobSetBuilder};

/// Cache of compiled glob sets, keyed by the ordered list of pattern strings.
///
/// The key stores the patterns verbatim (rather than a hash) so lookups are
/// exact and can never return a set compiled from a different pattern list.
type GlobSetCache = HashMap<Vec<String>, Arc<GlobSet>>;

static GLOB_SET_CACHE: LazyLock<RwLock<GlobSetCache>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Build, or reuse a previously built, compiled [`GlobSet`] for `patterns`.
///
/// Patterns are compiled with globset's default [`Glob::new`] options, so any
/// caller that would otherwise build its set with `Glob::new` can share cache
/// entries. The returned set is shared behind an [`Arc`]; it is immutable and
/// safe to use concurrently.
///
/// The cache is keyed by the ordered pattern strings, so callers that pass the
/// same patterns in the same order get the same compiled set. An empty
/// `patterns` slice yields an empty set (one that matches nothing), matching
/// the behavior of building an empty [`GlobSetBuilder`].
pub fn build_glob_set<S: AsRef<str>>(
    patterns: &[S],
) -> Result<Arc<GlobSet>, globset::Error> {
    let key: Vec<String> =
        patterns.iter().map(|p| p.as_ref().to_owned()).collect();

    if let Some(hit) = read_cache().get(&key) {
        return Ok(Arc::clone(hit));
    }

    // Build outside the write lock so the (potentially slow) compilation never
    // blocks other lookups. Two threads racing on the same key simply compile
    // it twice; because identical inputs produce identical sets, whichever
    // insertion lands first wins and the other is discarded.
    let mut builder = GlobSetBuilder::new();
    for pattern in &key {
        builder.add(Glob::new(pattern)?);
    }
    let set = Arc::new(builder.build()?);

    let mut cache = write_cache();
    let entry = cache.entry(key).or_insert(set);
    Ok(Arc::clone(entry))
}

fn read_cache() -> std::sync::RwLockReadGuard<'static, GlobSetCache> {
    // The critical sections only touch the map (no user code runs while the
    // lock is held), so the lock cannot be poisoned in practice; recover the
    // guard defensively rather than propagating a panic.
    GLOB_SET_CACHE.read().unwrap_or_else(|e| e.into_inner())
}

fn write_cache() -> std::sync::RwLockWriteGuard<'static, GlobSetCache> {
    GLOB_SET_CACHE.write().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_matching_set() {
        let set = build_glob_set(&["src/**/*.rs", "*.toml"]).unwrap();

        assert!(set.is_match("src/a/b.rs"));
        assert!(set.is_match("Cargo.toml"));
        assert!(!set.is_match("src/a/b.txt"));
    }

    #[test]
    fn empty_patterns_match_nothing() {
        let set = build_glob_set::<&str>(&[]).unwrap();

        assert!(set.is_empty());
        assert!(!set.is_match("anything"));
    }

    #[test]
    fn identical_patterns_return_the_same_cached_instance() {
        let a = build_glob_set(&["a/*.rs", "b/*.rs"]).unwrap();
        let b = build_glob_set(&["a/*.rs", "b/*.rs"]).unwrap();

        // A cache hit returns the very same allocation, not just an equal one.
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn different_patterns_do_not_alias() {
        let a = build_glob_set(&["a/*.rs"]).unwrap();
        let b = build_glob_set(&["b/*.rs"]).unwrap();

        assert!(!Arc::ptr_eq(&a, &b));
        assert!(a.is_match("a/x.rs") && !a.is_match("b/x.rs"));
        assert!(b.is_match("b/x.rs") && !b.is_match("a/x.rs"));
    }

    #[test]
    fn order_is_significant_in_the_key() {
        let a = build_glob_set(&["a/*.rs", "b/*.rs"]).unwrap();
        let b = build_glob_set(&["b/*.rs", "a/*.rs"]).unwrap();

        // Different key ordering => distinct cache entries, but both must match
        // the same files.
        assert!(!Arc::ptr_eq(&a, &b));
        for f in ["a/x.rs", "b/y.rs"] {
            assert_eq!(a.is_match(f), b.is_match(f));
        }
    }

    #[test]
    fn accepts_owned_and_borrowed_pattern_types() {
        let owned: Vec<String> = vec!["*.md".to_string()];
        let set_owned = build_glob_set(&owned).unwrap();
        let set_borrowed = build_glob_set(&["*.md"]).unwrap();

        assert!(Arc::ptr_eq(&set_owned, &set_borrowed));
        assert!(set_owned.is_match("README.md"));
    }

    #[test]
    fn invalid_pattern_is_an_error() {
        // An unclosed character class is rejected by globset.
        assert!(build_glob_set(&["a[b"]).is_err());
    }
}
