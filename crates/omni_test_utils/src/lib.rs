//! Host-side benchmark workspace generation.
//!
//! The pure, filesystem-free logic (config surface, seeded graph, task-graph
//! edges, workspace model, and omni-config rendering) lives in
//! [`omni_workspace_gen`] and is re-exported here so existing consumers keep
//! importing `HarnessConfig`, `DependencyStrategy`, etc. from `omni_test_utils`.
//! This crate adds the host concerns: writing the rendered omni files plus the
//! neutral base (launcher scripts + `src/**` content tree) to disk.

mod content;
mod harness;
mod launcher;
pub mod presets;

pub use harness::*;
pub use launcher::*;

pub use omni_workspace_gen::*;

pub(crate) use content::write_content_tree;
