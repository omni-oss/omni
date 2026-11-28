#[cfg(feature = "schemars")]
use schemars::JsonSchema;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// A value is `0`, above its host ceiling, or inconsistent with another limit.
///
/// The message is the user-facing explanation (which key, and the bound); the
/// workspace load turns it into `workspace.invalid_containment`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LimitsError {
    #[error("`containment.limits.fs.read.{key}` must be at least 1")]
    Zero { key: &'static str },

    #[error(
        "`containment.limits.fs.read.{key}` is above the host ceiling of {ceiling}"
    )]
    AboveCeiling { key: &'static str, ceiling: u64 },

    #[error(
        "`containment.limits.fs.read.max_file_bytes` ({max_file_bytes}) is above `max_total_bytes` ({max_total_bytes}); a single read is bounded by both, so raise `max_total_bytes` too"
    )]
    FileAboveTotal {
        max_file_bytes: u64,
        max_total_bytes: u64,
    },
}

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;

/// The resolved file-read budgets, every field a concrete number.
///
/// Mirrors the fields of the input view's limits and carries the same
/// defaults (a test in the reader crate guards that they do not drift).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FsReadLimits {
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

impl FsReadLimits {
    /// The defaults applied when a key is absent.
    pub const DEFAULT: Self = Self {
        max_path_bytes: 4096,
        max_path_depth: 64,
        max_file_bytes: 8 * MIB,
        max_total_bytes: 16 * MIB,
        max_tracked_inputs: 4096,
        max_dir_entries: 10_000,
        max_total_dir_entries: 50_000,
        max_dir_scan_entries: 100_000,
        max_operations: 10_000,
        max_reported_changes: 100,
        max_observations_per_path: 16,
    };

    /// The host-owned ceilings a configured value may not exceed. Path bytes
    /// and depth can only be lowered.
    pub const CEILING: Self = Self {
        max_path_bytes: 4096,
        max_path_depth: 64,
        max_file_bytes: 64 * MIB,
        max_total_bytes: 256 * MIB,
        max_tracked_inputs: 32_768,
        max_dir_entries: 100_000,
        max_total_dir_entries: 500_000,
        max_dir_scan_entries: 1_000_000,
        max_operations: 100_000,
        max_reported_changes: 1_000,
        max_observations_per_path: 64,
    };
}

impl Default for FsReadLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

macro_rules! limits_configuration {
    ($( $field:ident : $ty:ty ),+ $(,)?) => {
        /// The `containment.limits.fs.read` block. Every key is optional; an
        /// absent key means the host default. Values must be between 1 and the
        /// host ceiling; that is checked by [`FsReadLimitsConfiguration::resolve`]
        /// (the configuration loader's parse errors name only the key path, so
        /// the bound check runs after the parse, where the message can name the
        /// key and the ceiling).
        #[derive(Debug, Clone, Default, PartialEq, Eq)]
        #[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
        #[cfg_attr(feature = "schemars", derive(JsonSchema))]
        #[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
        pub struct FsReadLimitsConfiguration {
            $(
                #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "Option::is_none"))]
                #[cfg_attr(feature = "schemars", schemars(range(min = 1, max = FsReadLimits::CEILING.$field)))]
                pub $field: Option<$ty>,
            )+
        }

        impl FsReadLimitsConfiguration {
            /// Check every bound and the cross-field rule, then fill the
            /// defaults.
            pub fn resolve(&self) -> Result<FsReadLimits, LimitsError> {
                let mut out = FsReadLimits::DEFAULT;
                $(
                    if let Some(value) = self.$field {
                        if value == 0 {
                            return Err(LimitsError::Zero { key: stringify!($field) });
                        }
                        if value > FsReadLimits::CEILING.$field {
                            return Err(LimitsError::AboveCeiling {
                                key: stringify!($field),
                                ceiling: FsReadLimits::CEILING.$field as u64,
                            });
                        }
                        out.$field = value;
                    }
                )+
                if out.max_file_bytes > out.max_total_bytes {
                    return Err(LimitsError::FileAboveTotal {
                        max_file_bytes: out.max_file_bytes,
                        max_total_bytes: out.max_total_bytes,
                    });
                }
                Ok(out)
            }
        }

    };
}

limits_configuration! {
    max_path_bytes: usize,
    max_path_depth: usize,
    max_file_bytes: u64,
    max_total_bytes: u64,
    max_tracked_inputs: usize,
    max_dir_entries: usize,
    max_total_dir_entries: usize,
    max_dir_scan_entries: usize,
    max_operations: usize,
    max_reported_changes: usize,
    max_observations_per_path: usize,
}

/// The `containment.limits.fs` block.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schemars", derive(JsonSchema))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct FsLimits {
    /// Budgets for reading files and directories.
    #[cfg_attr(feature = "serde", serde(default))]
    pub read: FsReadLimitsConfiguration,
}

/// The `containment.limits` block: host-owned budgets, set only here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schemars", derive(JsonSchema))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct ContainmentLimits {
    #[cfg_attr(feature = "serde", serde(default))]
    pub fs: FsLimits,
}

/// The workspace `containment` block: how the host bounds what code it does
/// not fully trust can read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schemars", derive(JsonSchema))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct ContainmentConfiguration {
    #[cfg_attr(feature = "serde", serde(default))]
    pub limits: ContainmentLimits,
}

impl ContainmentConfiguration {
    /// The resolved file-read limits (defaults filled in).
    pub fn fs_read_limits(&self) -> Result<FsReadLimits, LimitsError> {
        self.limits.fs.read.resolve()
    }
}

/// What a run carries when contained reads are enabled: the resolved limits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContainedReads {
    pub limits: FsReadLimits,
}

#[cfg(all(test, feature = "serde"))]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<ContainmentConfiguration, String> {
        serde_json::from_str(json).map_err(|e| e.to_string())
    }

    /// Parse, then check the bounds the way the context load does.
    fn resolve(json: &str) -> Result<FsReadLimits, String> {
        parse(json)?.fs_read_limits().map_err(|e| e.to_string())
    }

    #[test]
    fn empty_block_resolves_to_the_defaults() {
        assert_eq!(resolve("{}").unwrap(), FsReadLimits::DEFAULT);
    }

    #[test]
    fn each_key_overrides_its_default() {
        let limits = resolve(
            r#"{"limits":{"fs":{"read":{"max_file_bytes":1024,"max_operations":5}}}}"#,
        )
        .unwrap();
        assert_eq!(limits.max_file_bytes, 1024);
        assert_eq!(limits.max_operations, 5);
        assert_eq!(limits.max_path_bytes, FsReadLimits::DEFAULT.max_path_bytes);
    }

    #[test]
    fn zero_is_rejected_and_names_the_key() {
        let err = resolve(r#"{"limits":{"fs":{"read":{"max_path_depth":0}}}}"#)
            .unwrap_err();
        assert!(err.contains("max_path_depth"), "{err}");
    }

    #[test]
    fn above_the_ceiling_is_rejected_and_names_the_ceiling() {
        let err = resolve(
            r#"{"limits":{"fs":{"read":{"max_file_bytes":67108865,"max_total_bytes":268435456}}}}"#,
        )
        .unwrap_err();
        assert!(err.contains("max_file_bytes"), "{err}");
        assert!(err.contains("67108864"), "{err}");
        let err =
            resolve(r#"{"limits":{"fs":{"read":{"max_path_bytes":4097}}}}"#)
                .unwrap_err();
        assert!(err.contains("4096"), "{err}");
    }

    #[test]
    fn a_file_limit_above_the_effective_total_is_rejected() {
        // 32 MiB is within the file ceiling but above the 16 MiB default total.
        let err = resolve(
            r#"{"limits":{"fs":{"read":{"max_file_bytes":33554432}}}}"#,
        )
        .unwrap_err();
        assert!(err.contains("max_total_bytes"), "{err}");
        let ok = resolve(
            r#"{"limits":{"fs":{"read":{"max_file_bytes":33554432,"max_total_bytes":67108864}}}}"#,
        )
        .unwrap();
        assert_eq!(ok.max_file_bytes, 33554432);
    }

    #[test]
    fn unknown_keys_and_misplaced_limits_are_rejected() {
        assert!(parse(r#"{"other":1}"#).is_err());
        assert!(parse(r#"{"limits":{"other":1}}"#).is_err());
        assert!(parse(r#"{"limits":{"fs":{"other":1}}}"#).is_err());
        assert!(parse(r#"{"limits":{"fs":{"read":{"other":1}}}}"#).is_err());
        // a limit directly under `limits`, `fs` or `containment`
        assert!(parse(r#"{"max_file_bytes":1}"#).is_err());
        assert!(parse(r#"{"limits":{"max_file_bytes":1}}"#).is_err());
        assert!(parse(r#"{"limits":{"fs":{"max_file_bytes":1}}}"#).is_err());
    }

    #[test]
    fn defaults_never_exceed_ceilings() {
        let d = FsReadLimits::DEFAULT;
        let c = FsReadLimits::CEILING;
        assert!(d.max_path_bytes <= c.max_path_bytes);
        assert!(d.max_path_depth <= c.max_path_depth);
        assert!(d.max_file_bytes <= c.max_file_bytes);
        assert!(d.max_total_bytes <= c.max_total_bytes);
        assert!(d.max_tracked_inputs <= c.max_tracked_inputs);
        assert!(d.max_dir_entries <= c.max_dir_entries);
        assert!(d.max_total_dir_entries <= c.max_total_dir_entries);
        assert!(d.max_dir_scan_entries <= c.max_dir_scan_entries);
        assert!(d.max_operations <= c.max_operations);
        assert!(d.max_reported_changes <= c.max_reported_changes);
        assert!(d.max_observations_per_path <= c.max_observations_per_path);
        assert!(d.max_file_bytes <= d.max_total_bytes);
    }

    #[test]
    fn serialization_omits_unset_keys() {
        let config = parse("{}").unwrap();
        let json = serde_json::to_string(&config).unwrap();
        assert_eq!(json, r#"{"limits":{"fs":{"read":{}}}}"#);
    }
}
