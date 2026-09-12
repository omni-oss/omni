use omni_constants::{
    OMNI_CACHE_DIR, OMNI_DIR, OMNI_LOCKS_DIR, OMNI_SCRATCH_DIR,
    OMNI_SOURCES_DIR, OMNI_TRACE_DIR, REMOTE_CACHE_OMNI, SOURCE_LOCKFILE_NAME,
};
use omni_ignore_core::{IgnoreContributor, IgnorePattern};

/// Emits the ordered set of patterns for omni's own state under `.omni/`. The
/// patterns are built from the same `omni_constants` segments the subsystems use
/// to write those paths, so a rename moves the writer and the ignore pattern
/// together. The `sources/*/lock.json` keep-rule is emitted right after the base
/// it re-includes and must never be reordered ahead of it.
pub struct InternalContributor;

impl InternalContributor {
    fn lines() -> Vec<String> {
        let remote_cache = REMOTE_CACHE_OMNI.replace("{ext}", "*");
        vec![
            format!("/{OMNI_CACHE_DIR}/**"),
            format!("/{OMNI_LOCKS_DIR}/**"),
            format!("/{OMNI_DIR}/{remote_cache}"),
            format!("/{OMNI_SCRATCH_DIR}/**"),
            format!("/{OMNI_SOURCES_DIR}/*/**"),
            format!("!/{OMNI_SOURCES_DIR}/*/{SOURCE_LOCKFILE_NAME}"),
            format!("/{OMNI_TRACE_DIR}/**"),
        ]
    }
}

#[async_trait::async_trait]
impl IgnoreContributor for InternalContributor {
    fn name(&self) -> &'static str {
        "internal"
    }

    async fn patterns(&self) -> eyre::Result<Vec<IgnorePattern>> {
        Self::lines()
            .iter()
            .map(|line| IgnorePattern::raw(line).map_err(eyre::Report::from))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn emits_the_expected_ordered_patterns() {
        let patterns = InternalContributor.patterns().await.unwrap();
        let lines: Vec<&str> =
            patterns.iter().map(IgnorePattern::as_str).collect();

        assert_eq!(
            lines,
            vec![
                "/.omni/cache/**",
                "/.omni/locks/**",
                "/.omni/remote-cache.omni.*",
                "/.omni/scratch/**",
                "/.omni/sources/*/**",
                "!/.omni/sources/*/lock.json",
                "/.omni/trace/**",
            ]
        );
    }

    #[tokio::test]
    async fn excludes_the_remote_cache_service_directory() {
        let lines = InternalContributor::lines();
        assert!(!lines.iter().any(|l| l.contains("remote_cache")));
    }

    #[tokio::test]
    async fn keep_rule_follows_its_base_pattern() {
        let lines = InternalContributor::lines();
        let base = lines
            .iter()
            .position(|l| l == "/.omni/sources/*/**")
            .unwrap();
        let keep = lines
            .iter()
            .position(|l| l == "!/.omni/sources/*/lock.json")
            .unwrap();
        assert_eq!(keep, base + 1);
    }

    #[tokio::test]
    async fn patterns_are_derived_from_constants() {
        let lines = InternalContributor::lines();
        assert_eq!(lines[0], format!("/{OMNI_CACHE_DIR}/**"));
        assert_eq!(lines[4], format!("/{OMNI_SOURCES_DIR}/*/**"));
        assert_eq!(
            lines[5],
            format!("!/{OMNI_SOURCES_DIR}/*/{SOURCE_LOCKFILE_NAME}")
        );
    }
}
