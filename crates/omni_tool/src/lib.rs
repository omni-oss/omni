//! The tool subsystem: discovery and execution of `tool.omni.*` manifests.

pub mod error;

mod bridge_runner;
mod discover;
mod pipeline;
mod run;
mod sys;

pub use bridge_runner::LazyToolRunner;
pub use discover::discover;
pub use run::{
    EXEC_TOOL_PATH, ExecToolPayload, ToolEnforcement, ToolRunner, run_named,
};
pub use sys::ToolSys;
