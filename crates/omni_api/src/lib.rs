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
        DataView, ForwardedInputs, GeneratorInfo, GeneratorInputKind,
        GeneratorInputSpec, GeneratorInspectNode, GeneratorInspectResponse,
        GeneratorListResponse, GeneratorRunRequest, GeneratorRunResponse,
        GeneratorTargetSpec, GeneratorValidateInputRequest,
        GeneratorValidateInputResponse, InputCondition, InputDefault,
        InputFieldError, InputOption, InputValidator, InspectViewKind,
        StaticInputDefault, SubGeneratorRef, SubGeneratorValidationResult,
        WidgetView,
    },
    hash::HashResponse,
    task::{TaskRunFilters, TaskRunRequest, TaskRunResponse},
};

pub use operations::config_schema::handle_config_schema;
