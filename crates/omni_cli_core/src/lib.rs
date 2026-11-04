#![allow(clippy::redundant_field_names)]
// Presenter crate (RFC 0023): the only place user-facing text is written.
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
