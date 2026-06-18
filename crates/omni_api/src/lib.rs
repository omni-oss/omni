#![allow(async_fn_in_trait)]

mod api;
pub mod error;
pub mod operations;
mod setup_guard;

pub use api::{OmniApi, OmniApiBuilder};
pub use error::OmniApiError;

// Re-export commonly used types at the crate root.
pub use operations::{
    cache::{
        CachePruneRequest, CachePruneResponse, CacheRemoteSetupRequest,
        CacheStatsRequest,
    },
    config_schema::{ConfigSchemaResponse, SchemaKind},
    env::{EnvRequest, EnvResponse},
    exec::{ExecRequest, ExecResponse},
    generator::{
        GeneratorInfo, GeneratorListResponse, GeneratorRunRequest,
        GeneratorRunResponse,
    },
    hash::HashResponse,
    run::{RunFilters, RunRequest, RunResponse},
};

pub use operations::config_schema::handle_config_schema;
