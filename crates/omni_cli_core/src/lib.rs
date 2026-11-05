#![allow(clippy::redundant_field_names)]
// CLI presenter crate: renders user-facing text from events and command results.
#![allow(clippy::disallowed_macros)]

pub mod build;
pub mod commands;
pub mod configurations;
pub mod constants;
pub mod context;
pub mod core;
pub mod executor;
pub mod notices;
pub mod subscriber;
pub mod task_output_capture;
pub mod utils;
