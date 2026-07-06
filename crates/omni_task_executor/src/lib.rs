mod batch_executor;
mod cache_manager;
mod cache_store_provider;
mod config;
mod execution_plan_provider;
mod executor;
mod force;

mod on_failure;
mod pipeline;
mod result;
mod serde_impls;
mod sys;
mod task_context_provider;
mod utils;

pub use config::*;
pub use executor::*;
pub use force::*;
pub use omni_execution_plan::Call;
pub use on_failure::*;
pub use result::*;
pub use sys::*;

/// Internal pipeline components exposed for the crate's own benchmarks
/// (`benches/`). Enabled via the `bench-support` feature; not part of the
/// stable public API.
#[cfg(feature = "bench-support")]
pub mod bench_support {
    pub use crate::cache_manager::{
        CacheManager, CacheManagerBuilder, TaskResultContext,
    };
    pub use crate::cache_store_provider::{
        CacheStoreProvider, ContextCacheStoreProvider,
    };
    pub use crate::execution_plan_provider::ContextExecutionPlanProvider;
    pub use crate::task_context_provider::DefaultTaskContextProvider;
}
