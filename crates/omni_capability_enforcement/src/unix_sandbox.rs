//! Helpers shared by the Unix [`Tier::OsSandbox`](crate::Tier::OsSandbox)
//! backends — Linux [`landlock_sandbox`](crate::landlock_sandbox) and macOS
//! [`seatbelt_sandbox`](crate::seatbelt_sandbox).
//!
//! Both confine a spawned JS runtime from a `pre_exec` hook and grant the same
//! universal `/dev` sink/source nodes plus the same "only operate on paths that
//! exist" guard, so those live here as one source of truth rather than being
//! duplicated (and drifting) per backend. Anything genuinely platform-specific
//! — Landlock's Linux loader/`/proc`/`/sys` *read* baseline, a Seatbelt SBPL
//! profile — stays in its own module.

// The Seatbelt backend is still a skeleton, so on macOS these helpers are not
// yet called; keep them here as the shared seam without a dead-code warning
// until that integration lands (Landlock exercises them on Linux today).
#![allow(dead_code)]

use std::path::PathBuf;

/// Safe pseudo-devices a confined child may read *and* write, identical on every
/// Unix. These are the universal sink/source device nodes with no persistence or
/// side effects beyond the calling process; granting them keeps ordinary
/// programs working (redirecting to `/dev/null`, reading randomness) without
/// widening access to real files. Callers should pass the result through
/// [`existing`] before handing paths to the kernel, since not every node is
/// present on every host.
pub(crate) fn writable_pseudo_devices() -> Vec<PathBuf> {
    [
        "/dev/null",
        "/dev/zero",
        "/dev/full",
        "/dev/random",
        "/dev/urandom",
        "/dev/tty",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect()
}

/// Keep only paths that exist. The Unix sandbox facilities open each granted
/// path (Landlock's `path_beneath_rules` does an `O_PATH` open; a Seatbelt
/// profile references it), and a missing one would fail the whole ruleset/
/// profile — so a non-existent baseline entry must be filtered out rather than
/// aborting confinement.
pub(crate) fn existing(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths.iter().filter(|p| p.exists()).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writable_pseudo_devices_are_all_under_dev() {
        for path in writable_pseudo_devices() {
            assert!(
                path.starts_with("/dev"),
                "{path:?} is not a /dev pseudo-device"
            );
        }
    }

    #[test]
    fn existing_filters_out_absent_paths() {
        let present = std::env::temp_dir();
        let absent = present.join("omni-unix-sandbox-does-not-exist-xyz");
        let filtered = existing(&[present.clone(), absent]);
        assert_eq!(filtered, vec![present]);
    }
}
