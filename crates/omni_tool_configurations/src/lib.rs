//! Configuration types for the tool subsystem: the `tool.omni.yaml` manifest
//! ([`ToolConfiguration`]), its execution backend ([`ToolBackend`]), and the
//! [`Tool`] capability profile.
mod tool_backend;
mod tool_configuration;
mod tool_profile;

pub use tool_backend::*;
pub use tool_configuration::*;
pub use tool_profile::*;
