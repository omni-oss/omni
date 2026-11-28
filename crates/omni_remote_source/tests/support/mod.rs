//! Helpers shared by the crate's integration tests. Each test binary includes
//! this module, so not every binary uses every helper.
#![allow(dead_code)]

#[cfg(unix)]
pub mod unix {
    use std::{os::unix::fs::PermissionsExt, path::Path};

    /// Set a path's permission bits. Only for tests that inject failures.
    pub fn set_mode(path: &Path, mode: u32) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .unwrap();
    }

    /// Root ignores directory permissions, so the failure cannot be injected.
    pub fn permissions_are_enforced(dir: &Path) -> bool {
        let probe = dir.join("probe");
        std::fs::create_dir(&probe).unwrap();
        set_mode(&probe, 0o555);
        let enforced = std::fs::write(probe.join("f"), b"").is_err();
        set_mode(&probe, 0o755);
        std::fs::remove_dir_all(&probe).ok();
        enforced
    }
}
