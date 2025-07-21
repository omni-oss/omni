use std::{collections::HashMap, fmt::Display};

use crate::{Transport, bridge::RequestHandlerFn};

pub struct BridgeRpcBuilder<TTransport: Transport> {
    transport: TTransport,
    handlers: HashMap<String, RequestHandlerFn>,
}

impl<TTransport: Transport> BridgeRpcBuilder<TTransport> {
    pub fn new(transport: TTransport) -> Self {
        Self {
            transport,
            handlers: HashMap::new(),
        }
    }

    pub fn handler<TFn, TRequest, TResponse, TError, TFuture>(
        mut self,
        path: impl Into<String>,
        handler: TFn,
    ) -> Self
    where
        TFn: FnMut(TRequest) -> TFuture + Send + Clone + 'static,
        TRequest: for<'de> serde::Deserialize<'de>,
        TResponse: serde::Serialize,
        TError: Display,
        TFuture:
            Future<Output = Result<TResponse, TError>> + Unpin + Send + 'static,
    {
        self.handlers
            .insert(path.into(), crate::bridge::create_handler(handler));
        self
    }

    pub fn build(self) -> crate::bridge::BridgeRpc<TTransport> {
        crate::bridge::BridgeRpc::new(self.transport, self.handlers)
    }
}
