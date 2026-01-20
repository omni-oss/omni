use async_trait::async_trait;
use bridge_rpc::{
    server::{request::Request, response::PendingResponse},
    service::{Service, ServiceContext},
    service_error::ServiceError,
};
use derive_new::new;

use crate::HandlerError;

#[derive(new)]
pub struct HandlerContext {
    pub request: Request,
    pub response: PendingResponse,
}

#[async_trait]
pub trait Handler: Send + Sync + 'static {
    async fn run(&self, context: HandlerContext) -> Result<(), HandlerError>;
}

#[async_trait]
impl<
    TFuture: Future<Output = Result<(), HandlerError>> + Send + Sync + 'static,
    TFn: Fn(HandlerContext) -> TFuture + Send + Sync + 'static,
> Handler for TFn
{
    async fn run(&self, context: HandlerContext) -> Result<(), HandlerError> {
        self(context).await
    }
}

#[derive(new)]
pub struct HandlerService<T: Handler> {
    handler: T,
}

#[async_trait]
impl<T: Handler> Service for HandlerService<T> {
    async fn run(
        &self,
        context: ServiceContext,
    ) -> Result<(), bridge_rpc::service_error::ServiceError> {
        self.handler
            .run(HandlerContext::new(context.request, context.response))
            .await
            .map_err(|e| ServiceError::custom_error(e))
    }
}
