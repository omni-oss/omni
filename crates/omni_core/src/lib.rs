#![allow(clippy::redundant_field_names)]

omni_codes::prefixes!("project", "task");

mod core_types;
#[cfg(test)]
mod diagnostic_tests;
mod extension_graph;
mod project_graph;
mod task_execution_graph;

pub use core_types::*;
pub use extension_graph::*;
pub use project_graph::*;
pub use task_execution_graph::*;
