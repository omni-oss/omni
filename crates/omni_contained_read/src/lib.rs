//! Contained file reads for code omni does not fully trust (F10.4).
//!
//! This crate joins three existing pieces so a script's file read goes
//! through the input view while still obeying the capability policy and the
//! generator's transactional overlay:
//!
//! - [`SharedAuthorizer`] and [`CapabilityInputAuthorizer`] adapt the
//!   capability policy to the view's `InputAuthorizer`;
//! - [`ContainedReader`] implements `BaseFsReadAsync`: it authorizes, then
//!   reads a name inside the workspace through a fresh `InputView` (symlinks
//!   rejected, limits enforced before allocation) and hands any other path to
//!   the existing broker unchanged;
//! - [`ReadThrough`] is the seam for a sys that has state of its own (the
//!   generator overlay) to answer first.
//!
//! The containment itself lives in `omni_input_view`; nothing here opens a
//! file by path.
//!
//! The guarantee is deliberately narrow: a read of a name inside the
//! workspace that is served from disk is contained. A path that reaches the
//! workspace through an outside alias is refused when the alias is visible at
//! check time (best effort, racy like the broker). Reads of paths wholly
//! outside the workspace are the broker's, unchanged.
#![deny(unsafe_code)]

mod authorizer;
mod error;
mod limits;
mod reader;
mod resolve;

pub use authorizer::{CapabilityInputAuthorizer, SharedAuthorizer};
pub use error::to_io_error;
pub use limits::input_limits;
pub use reader::{ContainedReader, PlainReadThrough, ReadThrough, RealRead};
pub use resolve::resolve_real_path;
