//! Gating for tests that depend on the OS, filesystem, kernel or privileges
//! (reflink, hardlink, symlink, OS sandboxes). RFC 0027, section 10.
//!
//! A gated test must never pass because it silently did not run. Call
//! [`gate`] (or [`require`], which probes a directory) at the top of the test:
//!
//! - capability available: returns `true`, run the body;
//! - unavailable and not required: prints a stable `SKIPPED[...]` marker and
//!   returns `false`, so the test returns early;
//! - unavailable and listed in `OMNI_REQUIRE_CAPABILITIES`: panics with
//!   `REQUIRED CAPABILITY UNAVAILABLE: ...`, so a CI lane that is supposed to
//!   prove the capability goes red instead of green.
//!
//! `OMNI_REQUIRE_CAPABILITIES` is a comma-separated list of capability names,
//! or `all`. `OMNI_REQUIRE_OS_SANDBOX` (set to anything) is kept as an alias
//! for `os-sandbox`.
#![allow(clippy::disallowed_macros)]

use std::{
    fmt::Display,
    fs,
    path::{Path, PathBuf},
};

/// Env var naming the capabilities a CI lane must prove.
pub const REQUIRE_ENV: &str = "OMNI_REQUIRE_CAPABILITIES";
/// Legacy alias for requiring `os-sandbox`.
pub const REQUIRE_OS_SANDBOX_ENV: &str = "OMNI_REQUIRE_OS_SANDBOX";
/// Directory (on a reflink-capable filesystem) for reflink scratch space.
pub const REFLINK_DIR_ENV: &str = "OMNI_TEST_REFLINK_DIR";
/// Prefix of the line printed when a gated test is skipped.
pub const SKIPPED_PREFIX: &str = "SKIPPED[";
/// Prefix of the panic message when a required capability is missing.
pub const REQUIRED_PREFIX: &str = "REQUIRED CAPABILITY UNAVAILABLE: ";

/// A filesystem capability that can be probed on a directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
pub enum Capability {
    Reflink,
    Hardlink,
    Symlink,
}

impl Capability {
    pub fn name(self) -> &'static str {
        self.into()
    }
}

/// Which capabilities the current run must prove.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Requirements {
    all: bool,
    names: Vec<String>,
}

impl Requirements {
    pub fn parse(list: Option<&str>, os_sandbox_alias: bool) -> Self {
        let mut requirements = Self::default();
        for name in list.unwrap_or_default().split(',') {
            let name = name.trim().to_ascii_lowercase();
            match name.as_str() {
                "" => {}
                "all" => requirements.all = true,
                _ => requirements.names.push(name),
            }
        }
        if os_sandbox_alias {
            requirements.names.push("os-sandbox".to_string());
        }
        requirements
    }

    pub fn from_env() -> Self {
        Self::parse(
            std::env::var(REQUIRE_ENV).ok().as_deref(),
            std::env::var_os(REQUIRE_OS_SANDBOX_ENV).is_some(),
        )
    }

    pub fn requires(&self, name: &str) -> bool {
        self.all || self.names.iter().any(|n| n == name)
    }
}

/// Decide whether a gated test body should run.
///
/// `detail` describes the environment (OS, filesystem, directory, probe error)
/// and is part of both the skip marker and the failure message.
pub fn gate(name: &str, available: bool, detail: impl Display) -> bool {
    decide(
        name,
        available,
        &detail.to_string(),
        &Requirements::from_env(),
    )
}

fn decide(
    name: &str,
    available: bool,
    detail: &str,
    requirements: &Requirements,
) -> bool {
    if available {
        return true;
    }
    if requirements.requires(name) {
        panic!(
            "{REQUIRED_PREFIX}{name} ({detail}); this lane lists it in \
             {REQUIRE_ENV}"
        );
    }
    eprintln!("{SKIPPED_PREFIX}capability={name} {detail}]");
    false
}

/// Probe `capability` in `dir` and gate on the result.
pub fn require(capability: Capability, dir: &Path) -> bool {
    let probe = probe(capability, dir);
    let detail = format!(
        "os={} fs={} dir={} probe={}",
        std::env::consts::OS,
        filesystem_type(dir),
        dir.display(),
        match &probe {
            Ok(()) => "ok".to_string(),
            Err(error) => format!("{:?}: {error}", error.kind()),
        }
    );
    gate(capability.name(), probe.is_ok(), detail)
}

/// Try the capability for real in `dir`.
pub fn probe(capability: Capability, dir: &Path) -> std::io::Result<()> {
    let scratch = tempfile::tempdir_in(dir)?;
    let source = scratch.path().join("source");
    let target = scratch.path().join("target");
    fs::write(&source, b"probe")?;
    match capability {
        Capability::Reflink => {
            reflink_copy::reflink(&source, &target)?;
            // A clone must be independent: editing it leaves the source alone.
            fs::write(&target, b"changed")?;
            if fs::read(&source)? != b"probe" {
                return Err(std::io::Error::other(
                    "reflink target shares data with its source",
                ));
            }
            Ok(())
        }
        Capability::Hardlink => fs::hard_link(&source, &target),
        Capability::Symlink => {
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&source, &target)
            }
            #[cfg(windows)]
            {
                std::os::windows::fs::symlink_file(&source, &target)
            }
        }
    }
}

/// A scratch directory for tests of `capability`. Reflink tests use
/// `OMNI_TEST_REFLINK_DIR` when set (default runners' temp dirs are not on a
/// reflink filesystem); everything else uses the system temp directory.
pub fn scratch_dir(capability: Capability) -> tempfile::TempDir {
    if capability == Capability::Reflink
        && let Some(base) = std::env::var_os(REFLINK_DIR_ENV)
    {
        let base = PathBuf::from(base);
        fs::create_dir_all(&base).expect("create OMNI_TEST_REFLINK_DIR");
        return tempfile::tempdir_in(base).expect("scratch dir");
    }
    tempfile::tempdir().expect("scratch dir")
}

/// Best-effort filesystem type of the volume holding `dir`, for diagnostics.
pub fn filesystem_type(dir: &Path) -> String {
    #[cfg(target_os = "linux")]
    {
        let Ok(dir) = fs::canonicalize(dir) else {
            return "unknown".into();
        };
        let Ok(mounts) = fs::read_to_string("/proc/self/mounts") else {
            return "unknown".into();
        };
        let mut best: Option<(usize, &str)> = None;
        for line in mounts.lines() {
            let mut fields = line.split_whitespace();
            let (_, Some(mount), Some(kind)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            if dir.starts_with(mount)
                && best.is_none_or(|(len, _)| mount.len() >= len)
            {
                best = Some((mount.len(), kind));
            }
        }
        best.map_or_else(|| "unknown".into(), |(_, kind)| kind.to_string())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = dir;
        "unknown".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requirements_parse_lists_all_and_the_legacy_alias() {
        let r = Requirements::parse(Some("reflink, Symlink ,,"), false);
        assert!(r.requires("reflink"));
        assert!(r.requires("symlink"));
        assert!(!r.requires("hardlink"));

        let all = Requirements::parse(Some("all"), false);
        assert!(all.requires("anything"));

        let alias = Requirements::parse(None, true);
        assert!(alias.requires("os-sandbox"));
        assert!(!alias.requires("reflink"));

        assert!(!Requirements::parse(None, false).requires("reflink"));
    }

    #[test]
    fn available_runs_and_unrequired_unavailable_skips() {
        let none = Requirements::default();
        assert!(decide("reflink", true, "d", &none));
        assert!(!decide("reflink", false, "d", &none));
    }

    #[test]
    #[should_panic(expected = "REQUIRED CAPABILITY UNAVAILABLE: reflink")]
    fn required_but_unavailable_panics_with_the_marker() {
        let r = Requirements::parse(Some("reflink"), false);
        decide("reflink", false, "fs=ext4 dir=/x", &r);
    }

    #[test]
    #[should_panic(expected = "fs=ext4 dir=/x")]
    fn the_failure_names_the_environment() {
        let r = Requirements::parse(Some("all"), false);
        decide("symlink", false, "fs=ext4 dir=/x", &r);
    }

    #[test]
    fn probes_report_real_capabilities_of_the_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        // Hard links work on every filesystem tests run on.
        probe(Capability::Hardlink, dir.path()).expect("hardlink");
        assert!(!filesystem_type(dir.path()).is_empty());
    }

    #[test]
    fn probing_a_missing_directory_is_an_error() {
        let missing = Path::new("/definitely/not/here");
        assert!(probe(Capability::Hardlink, missing).is_err());
    }
}
