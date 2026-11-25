//! Captured, containment-checked input view (RFC 0024 F10).
//!
//! F10.2a is the per-OS beneath-root open primitive and the D9 name check;
//! F10.2b adds the tracked [`InputView`] (read, exists, list, limits,
//! evidence). Symlink policy and revalidation land in later slices; nothing
//! here is consumed yet.
//!
//! Containment is only claimed where it is enforced and proven:
//!
//! - Linux: one `openat2` with `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS |
//!   RESOLVE_NO_MAGICLINKS | RESOLVE_NO_XDEV`. No fallback: if the syscall is
//!   unavailable the result is [`ContainmentViolation::Unsupported`].
//! - Windows: a one-component walk over retained handles that rejects every
//!   reparse point, then checks the final path of the handle.
//! - Everything else, including macOS: [`ContainmentViolation::Unsupported`]
//!   without touching the filesystem.
#![deny(unsafe_code)]

mod auth;
mod cancel;
mod contain;
mod digest;
mod error;
mod evidence;
mod limits;
mod name;
mod view;

pub use auth::{
    AuthDecision, AuthError, DenyCause, DenyReason, InputAuthorizer,
};
pub use cancel::Cancellation;
pub use contain::{ContainedRoot, open_beneath};
pub use digest::{Blake3Digest, Digest, DigestSink};
pub use error::{BudgetKind, ContainmentViolation, EntryKind, InputError};
pub use evidence::{Evidence, FileIdentity, Observation, PathEvidence};
pub use limits::InputLimits;
pub use name::check_name;
pub use view::{Exists, InputView, InputViewOptions, List, Listing, Read};
