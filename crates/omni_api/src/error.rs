use thiserror::Error;

#[derive(Debug, Error)]
pub enum OmniApiError {
    #[error("failed to initialize workspace context: {0}")]
    ContextInit(#[from] omni_context::ContextError),

    #[error("omni_setup initialization failed: {0}")]
    SetupInit(#[source] eyre::Report),
}
