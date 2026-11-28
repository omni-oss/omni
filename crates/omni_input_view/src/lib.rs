//! Captured, containment-checked input view (RFC 0024 F10).
//!
//! F10.2a is the per-OS beneath-root open primitive and the D9 name check;
//! F10.2b adds the tracked [`InputView`] (read, exists, list, limits,
//! evidence). F10.2c adds the reject-only [`SymlinkPolicy`] and F10.3 the final-pass
//! [`InputView::revalidate`]. The first consumer is `omni_contained_read`
//! (F10.4: contained script reads, opt-in); a discovery or plugin host that
//! uses the evidence and `revalidate` is still to come.
//!
//! Containment is only claimed where it is enforced and proven:
//!
//! - Linux: one `openat2` with `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS |
//!   RESOLVE_NO_MAGICLINKS | RESOLVE_NO_XDEV`. No fallback: if the syscall is
//!   unavailable the result is [`InputError::ContainmentUnsupported`].
//! - Windows: a one-component walk over retained handles that rejects every
//!   reparse point, then checks the final path of the handle.
//! - Everything else, including macOS: [`InputError::ContainmentUnsupported`]
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
pub use error::{
    BudgetKind, ContainmentViolation, EntryKind, InputError, NameProblem,
};
pub use evidence::{Evidence, FileIdentity, Observation, PathEvidence};
pub use limits::InputLimits;
pub use name::check_name;
pub use view::{
    Change, ChangeKind, Exists, InputView, InputViewOptions, InputsChanged,
    List, Listing, Read, Revalidation, SymlinkPolicy,
};
