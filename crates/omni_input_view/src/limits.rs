/// Host-owned budgets (maintainer-accepted defaults, adjustable later). The
/// checks that use them land in F10.2b; plugins cannot raise them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputLimits {
    pub max_path_bytes: usize,
    pub max_path_depth: usize,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
    pub max_tracked_inputs: usize,
    pub max_dir_entries: usize,
    pub max_total_dir_entries: usize,
    pub max_dir_scan_entries: usize,
    pub max_operations: usize,
    pub max_reported_changes: usize,
    pub max_observations_per_path: usize,
}

impl Default for InputLimits {
    fn default() -> Self {
        Self {
            max_path_bytes: 4096,
            max_path_depth: 64,
            max_file_bytes: 8 * 1024 * 1024,
            max_total_bytes: 16 * 1024 * 1024,
            max_tracked_inputs: 4096,
            max_dir_entries: 10_000,
            max_total_dir_entries: 50_000,
            max_dir_scan_entries: 100_000,
            max_operations: 10_000,
            max_reported_changes: 100,
            max_observations_per_path: 16,
        }
    }
}
