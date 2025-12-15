use std::{collections::HashMap, fmt::Display};

use super::{
    BridgeRpc, BridgeRpcBuilderError, HandlerContext, HandlerFn, StreamError,
    bridge_impl as bridge,
};

use crate::Transport;

pub struct BridgeRpcBuilder<TTransport: Transport> {
    transport: TTransport,
    handlers: HashMap<String, HandlerFn>,
}

impl<TTransport: Transport> BridgeRpcBuilder<TTransport> {
    pub fn new(transport: TTransport) -> Self {
        Self {
            transport,
            handlers: HashMap::new(),
        }
    }

    pub fn handler<TStartData, TStreamData, TError, TFuture, TFn>(
        mut self,
        path: impl Into<String>,
        handler: TFn,
    ) -> Self
    where
        TStartData: for<'de> serde::Deserialize<'de>,
        TStreamData: for<'de> serde::Deserialize<'de>,
        TError: Display,
        TFuture: Future<Output = Result<(), TError>> + Send + 'static,
        TFn: Fn(HandlerContext<TStartData, TStreamData, StreamError>) -> TFuture
            + Send
            + Sync
            + Clone
            + 'static,
    {
        self.handlers
            .insert(path.into(), bridge::create_handler(handler));
        self
    }

    pub fn build(self) -> Result<BridgeRpc<TTransport>, BridgeRpcBuilderError> {
        Ok(BridgeRpc::new(self.transport, self.handlers))
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
            rpc.has_handler("test_path").await,
            "handler should be registered"
        );
    }

    #[tokio::test]
    async fn test_builder_stream_handler() {
        let transport = MockTransport::new();
        let rpc = BridgeRpcBuilder::new(transport)
            .handler("test_path", |mut s: HandlerContext| async move {
                while let Some(_) = s.stream.next().await {}

                Ok::<_, String>(())
            })
            .build()
            .expect("should be able to build");

        assert!(
            rpc.has_handler("test_path").await,
            "handler should be registered"
        );
    }
}
