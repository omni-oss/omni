pub use omni_constants::{
    OMNI_CACHE_DIR as CACHE_DIR, OMNI_DIR, OMNI_IGNORE,
    OMNI_LOCKS_DIR as LOCKS_DIR, OMNI_SCRATCH_DIR as SCRATCH_DIR,
    OMNI_SOURCES_DIR as SOURCES_DIR, OMNI_TRACE_DIR as TRACE_DIR, PROJECT_OMNI,
    REMOTE_CACHE_OMNI, SUPPORTED_CONFIG_EXTS as SUPPORTED_EXTENSIONS,
    WORKSPACE_OMNI,
};
// pub const WORKSPACE_DIR_VAR: &str = "WORKSPACE_DIR";
// pub const PROJECT_DIR_VAR: &str = "PROJECT_DIR";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_constants_match_their_pre_migration_literals() {
        assert_eq!(OMNI_DIR, ".omni");
        assert_eq!(CACHE_DIR, ".omni/cache");
        assert_eq!(TRACE_DIR, ".omni/trace");
        assert_eq!(SCRATCH_DIR, ".omni/scratch");
        assert_eq!(LOCKS_DIR, ".omni/locks");
        assert_eq!(SOURCES_DIR, ".omni/sources");
    }
}
