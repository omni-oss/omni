//! The subsystem-agnostic engine behind `omni ignore sync`: the pattern data
//! boundary, the block renderer, the fence parser, and the per-file patch. The
//! concrete pattern sources live in `omni_ignore_contributors`.

mod contributor;
mod error;
mod fence;
mod patch;
mod pattern;
mod render;

pub use contributor::{Contribution, IgnoreContributor};
pub use error::{FenceError, IgnoreError};
pub use fence::{Eol, FENCE_BEGIN, FENCE_END_PREFIX, is_fence_line};
pub use patch::{
    CheckOutcome, CleanOutcome, FileReport, SyncOutcome, check_files,
    clean_files, sync_files,
};
pub use pattern::IgnorePattern;
pub use render::render_block;

#[cfg(test)]
pub(crate) use contributor::MockIgnoreContributor;
