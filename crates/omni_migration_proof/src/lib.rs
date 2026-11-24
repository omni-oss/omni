//! Disposable contract-proof harness for RFC 0032 (slice 1).
//!
//! Test-only prototype of the shared migration planner/runner and two
//! fixtures. Nothing in the workspace depends on it; delete the crate to undo.

pub mod core;
pub mod fs;
pub mod index;
pub mod lockfile;
