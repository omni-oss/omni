//! Configuration for how the host bounds access to the filesystem.
//!
//! This crate holds only configuration shapes and the host-owned numbers that
//! constrain them: the `containment` block of the workspace configuration,
//! the defaults and ceilings of the file-read budgets, and their load-time
//! validation. It has no filesystem or policy code and no dependency on the
//! input view; the reader that enforces the limits converts [`FsReadLimits`]
//! into its own type.
//!
//! ```yaml
//! containment:
//!   limits:
//!     fs:
//!       read:
//!         max_file_bytes: 4194304
//! ```
//!
//! The limits are host-owned: only the workspace configuration can set them,
//! a value of `0` or above its ceiling is a load error (never clamped), and
//! nothing else in the configuration can raise them.

mod limits;

pub use limits::{
    ContainedReads, ContainmentConfiguration, ContainmentLimits, FsLimits,
    FsReadLimits, FsReadLimitsConfiguration, LimitsError,
};
