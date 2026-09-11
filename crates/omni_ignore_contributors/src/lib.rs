//! The concrete pattern sources for `omni ignore sync`: a `projections`
//! contributor that reads the projection ledger and an `internal` contributor
//! that emits omni's own `.omni/` state.

mod internal;
mod projections;

pub use internal::InternalContributor;
pub use projections::ProjectionsContributor;
