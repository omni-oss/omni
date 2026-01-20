use async_trait::async_trait;
use bridge_rpc::{
    service::{Service, ServiceContext},
    service_error::ServiceError,
};
use futures::{FutureExt, future::BoxFuture};

type DynServiceFn = Box<
    dyn Fn(ServiceContext) -> BoxFuture<'static, Result<(), ServiceError>>
        + Send
        + Sync,
>;

pub struct ServiceFn(DynServiceFn);

#[async_trait]
impl Service for ServiceFn {
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        (self.0)(context).await
    }
}

pub fn service_fn<TFn, TFut>(f: TFn) -> ServiceFn
where
    TFn: Fn(ServiceContext) -> TFut + Send + Sync + 'static,
    TFut: Future<Output = Result<(), ServiceError>> + Send + Sync + 'static,
{
    ServiceFn(Box::new(move |context| f(context).boxed()))
}
