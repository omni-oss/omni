use std::{collections::HashMap, fmt::Display};

use crate::{
    BoxStream, StreamError, StreamHandlerFn, Transport,
    bridge::RequestHandlerFn,
};

pub struct BridgeRpcBuilder<TTransport: Transport> {
    transport: TTransport,
    request_handlers: HashMap<String, RequestHandlerFn>,
    stream_handlers: HashMap<String, StreamHandlerFn>,
}

impl<TTransport: Transport> BridgeRpcBuilder<TTransport> {
    pub fn new(transport: TTransport) -> Self {
        Self {
            transport,
            request_handlers: HashMap::new(),
            stream_handlers: HashMap::new(),
        }
    }

    pub fn request_handler<TFn, TRequest, TResponse, TError, TFuture>(
        mut self,
        path: impl Into<String>,
        handler: TFn,
    ) -> Self
    where
        TFn: FnMut(TRequest) -> TFuture + Send + Clone + 'static,
        TRequest: for<'de> serde::Deserialize<'de>,
        TResponse: serde::Serialize,
        TError: Display,
        TFuture: Future<Output = Result<TResponse, TError>> + Send + 'static,
    {
        self.request_handlers.insert(
            path.into(),
            crate::bridge::create_request_handler(handler),
        );
        self
    }

    pub fn stream_handler<TFn, TData, TFuture>(
        mut self,
        path: impl Into<String>,
        handler: TFn,
    ) -> Self
    where
        TData: for<'de> serde::Deserialize<'de>,
        TFn: FnMut(BoxStream<'static, Result<TData, StreamError>>) -> TFuture
            + Send
            + Clone
            + 'static,
        TFuture: Future<Output = ()> + Send + 'static,
    {
        self.stream_handlers
            .insert(path.into(), crate::bridge::create_stream_handler(handler));
        self
    }

    pub fn build(self) -> crate::bridge::BridgeRpc<TTransport> {
        crate::bridge::BridgeRpc::new(
            self.transport,
            self.request_handlers,
            self.stream_handlers,
        )
    }
}

#[cfg(test)]
mod tests {
    use tokio_stream::StreamExt as _;

    use super::*;

    #[tokio::test]
    async fn test_builder_request_handler() {
        let transport = crate::MockTransport::new();
        let rpc = BridgeRpcBuilder::new(transport)
            .request_handler("test_path", |req: String| async move {
                Ok::<_, String>(req)
            })
            .build();

        assert!(
            rpc.has_request_handler("test_path").await,
            "handler should be registered"
        );
    }

    #[tokio::test]
    async fn test_builder_stream_handler() {
        let transport = crate::MockTransport::new();
        let rpc = BridgeRpcBuilder::new(transport)
            .stream_handler(
                "test_path",
                |mut s: BoxStream<Result<String, StreamError>>| async move {
                    while let Some(_) = s.next().await {}
                },
            )
            .build();

        assert!(
            rpc.has_stream_handler("test_path").await,
            "handler should be registered"
        );
    }
}
