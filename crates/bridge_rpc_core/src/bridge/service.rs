use std::sync::Arc;

use crate::{ClientHandle, bridge::service_error::ServiceError};

pub use super::service_error as error;
use async_trait::async_trait;
use derive_new::new;

use super::server::{request::Request, response::PendingResponse};

#[derive(new)]
pub struct ServiceContext {
    pub request: Request,
    pub response: PendingResponse,
    pub client: Arc<ClientHandle>,
}

impl ServiceContext {
    pub fn from_request_and_response(
        request: Request,
        response: PendingResponse,
    ) -> Self {
        Self {
            request,
            response,
            client: Arc::new(ClientHandle::dummy()),
        }
    }
}

#[async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait Service: Send + Sync + 'static {
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError>;
}
