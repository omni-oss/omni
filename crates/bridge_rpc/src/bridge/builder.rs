use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
};

use crate::{
    BridgeRpcBuilderError, BridgeRpcBuilderErrorInner, RequestContext,
    StreamContext, StreamError, StreamHandlerFn, Transport,
    bridge::{self, BridgeRpc, RequestHandlerFn},
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

    pub fn request_handler<TRequestData, TResponseData, TError, TFuture, TFn>(
        mut self,
        path: impl Into<String>,
        handler: TFn,
    ) -> Self
    where
        TFn: Fn(RequestContext<TRequestData>) -> TFuture
            + Send
            + Sync
            + Clone
            + 'static,
        TRequestData: for<'de> serde::Deserialize<'de>,
        TResponseData: serde::Serialize,
        TError: Display,
        TFuture:
            Future<Output = Result<TResponseData, TError>> + Send + 'static,
    {
        self.request_handlers
            .insert(path.into(), bridge::create_request_handler(handler));
        self
    }

    pub fn stream_handler<TStartData, TStreamData, TError, TFuture, TFn>(
        mut self,
        path: impl Into<String>,
        handler: TFn,
    ) -> Self
    where
        TStartData: for<'de> serde::Deserialize<'de>,
        TStreamData: for<'de> serde::Deserialize<'de>,
        TError: Display,
        TFuture: Future<Output = Result<(), TError>> + Send + 'static,
        TFn: Fn(StreamContext<TStartData, TStreamData, StreamError>) -> TFuture
            + Send
            + Sync
            + Clone
            + 'static,
    {
        self.stream_handlers
            .insert(path.into(), bridge::create_stream_handler(handler));
        self
    }

    fn validate_paths(&self) -> Result<(), BridgeRpcBuilderError> {
        let paths = self
            .request_handlers
            .keys()
            .chain(self.stream_handlers.keys())
            .collect::<HashSet<_>>();

        for path in paths {
            if self.stream_handlers.contains_key(path)
                && self.request_handlers.contains_key(path)
            {
                return Err(BridgeRpcBuilderErrorInner::DuplicatePath(
                    path.clone(),
                )
                .into());
            }
        }

        Ok(())
    }

    pub fn build(self) -> Result<BridgeRpc<TTransport>, BridgeRpcBuilderError> {
        self.validate_paths()?;

        Ok(BridgeRpc::new(
            self.transport,
            self.request_handlers,
            self.stream_handlers,
        ))
    }
}

#[cfg(test)]
mod tests {
    use tokio_stream::StreamExt as _;

    use crate::MockTransport;

    use super::*;

    #[tokio::test]
    async fn test_builder_request_handler() {
        let transport = MockTransport::new();
        let rpc = BridgeRpcBuilder::new(transport)
            .request_handler(
                "test_path",
                |req: RequestContext<String>| async move {
                    Ok::<_, String>(req.data)
                },
            )
            .build()
            .expect("should be able to build");

        assert!(
            rpc.has_request_handler("test_path").await,
            "handler should be registered"
        );
    }

    #[tokio::test]
    async fn test_builder_stream_handler() {
        let transport = MockTransport::new();
        let rpc = BridgeRpcBuilder::new(transport)
            .stream_handler("test_path", |mut s: StreamContext| async move {
                while let Some(_) = s.stream.next().await {}

                Ok::<_, String>(())
            })
            .build()
            .expect("should be able to build");

        assert!(
            rpc.has_stream_handler("test_path").await,
            "handler should be registered"
        );
    }
}
