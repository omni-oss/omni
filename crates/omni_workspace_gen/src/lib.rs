//! Pure, filesystem-free core for benchmark workspace generation.
//!
//! Owns the single copy of the harness config surface, the seeded
//! dependency-graph generator, deterministic naming, and the task-graph edge
//! rules. Later modules add the serializable workspace model and pure renderers
//! that turn that model into file *contents* (never touching the filesystem),
//! so the same core can run natively for `omni_bench` and via wasm for
//! `task-bench`.

mod config;
mod graph;
mod model;
mod render;

pub use config::*;
pub use graph::*;
pub use model::*;
pub use render::*;
