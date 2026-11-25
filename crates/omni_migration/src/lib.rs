//! Shared planner and runner for pre-v1 store migrations (RFC 0032).
//!
//! The contract is storage-agnostic: it names no file, path or format. A
//! store supplies a [`MigrationStore`] adapter and a [`Catalog`]; [`build_plan`]
//! produces a sealed [`Plan`] and [`execute`] runs it under the adapter's
//! coordination. Nothing in the workspace depends on this crate yet.
//!
//! The crate is synchronous and has no async runtime dependency. Cancellation
//! is cooperative between steps.

mod access;
mod chain;
mod error;
mod plan;
mod run;
mod types;

pub use access::{Check, gate_access};
pub use chain::{Chain, StepId, Transition};
pub use error::Error;
pub use plan::{Catalog, Plan, PlannedStep, Step, build_plan, check_backend};
pub use run::{
    AuthorizedStep, GuardedObservation, MigrationSession, MigrationStore, Run,
    StepOutcome, execute,
};
pub use types::{
    BackendSupport, CatalogId, ChainBinding, ChainName, Direction, Policy,
    Revision, Stamp, StepCapabilities, StoreId, StoreState, Version,
};
