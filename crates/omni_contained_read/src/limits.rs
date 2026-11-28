use omni_containment_configurations::FsReadLimits;
use omni_input_view::InputLimits;

/// The view's limits for the configured (or default) file-read budgets.
pub fn input_limits(limits: &FsReadLimits) -> InputLimits {
    InputLimits {
        max_path_bytes: limits.max_path_bytes,
        max_path_depth: limits.max_path_depth,
        max_file_bytes: limits.max_file_bytes,
        max_total_bytes: limits.max_total_bytes,
        max_tracked_inputs: limits.max_tracked_inputs,
        max_dir_entries: limits.max_dir_entries,
        max_total_dir_entries: limits.max_total_dir_entries,
        max_dir_scan_entries: limits.max_dir_scan_entries,
        max_operations: limits.max_operations,
        max_reported_changes: limits.max_reported_changes,
        max_observations_per_path: limits.max_observations_per_path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_configured_defaults_equal_the_views_defaults() {
        // Guards the two copies of the defaults against drifting apart.
        assert_eq!(
            input_limits(&FsReadLimits::DEFAULT),
            InputLimits::default()
        );
    }

    #[test]
    fn every_field_is_carried() {
        let limits = FsReadLimits {
            max_path_bytes: 1,
            max_path_depth: 2,
            max_file_bytes: 3,
            max_total_bytes: 4,
            max_tracked_inputs: 5,
            max_dir_entries: 6,
            max_total_dir_entries: 7,
            max_dir_scan_entries: 8,
            max_operations: 9,
            max_reported_changes: 10,
            max_observations_per_path: 11,
        };
        let out = input_limits(&limits);
        assert_eq!(out.max_path_bytes, 1);
        assert_eq!(out.max_path_depth, 2);
        assert_eq!(out.max_file_bytes, 3);
        assert_eq!(out.max_total_bytes, 4);
        assert_eq!(out.max_tracked_inputs, 5);
        assert_eq!(out.max_dir_entries, 6);
        assert_eq!(out.max_total_dir_entries, 7);
        assert_eq!(out.max_dir_scan_entries, 8);
        assert_eq!(out.max_operations, 9);
        assert_eq!(out.max_reported_changes, 10);
        assert_eq!(out.max_observations_per_path, 11);
    }
}
