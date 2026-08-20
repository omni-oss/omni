use system_traits::{FsReadAsync, auto_impl};

/// Aggregate system-handle bound for the tool subsystem's read-only needs:
/// discovery and manifest loading. Cloneable so handles can be duplicated
/// across the concurrent discovery tasks.
#[auto_impl]
pub trait ToolSys: Clone + Send + Sync + 'static + FsReadAsync {}
