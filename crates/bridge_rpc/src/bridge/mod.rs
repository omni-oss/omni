mod builder;
mod error;
mod frame;
mod request_id;

pub use builder::*;
pub use error::*;
use frame::*;
use futures::future::BoxFuture;
use request_id::*;

use std::{collections::HashMap, fmt::Display, sync::Arc};

use serde::Serialize;
use tokio::sync::{
    Mutex,
    oneshot::{self},
};

use crate::Transport;

type TxPipe = oneshot::Sender<Result<rmpv::Value, ErrorData>>;

type TxPipeMaps = HashMap<RequestId, TxPipe>;

pub type RequestHandlerFnFuture =
    BoxFuture<'static, JsBridgeResult<rmpv::Value>>;

pub type RequestHandlerFn =
    Box<dyn FnMut(rmpv::Value) -> RequestHandlerFnFuture + Send>;

pub struct BridgeRpc<TTransport: Transport> {
    transport: TTransport,
    response_pipes: Arc<Mutex<TxPipeMaps>>,
    request_handlers: Arc<Mutex<HashMap<String, RequestHandlerFn>>>,
}

pub fn create_handler<TFn, TRequest, TResponse, TError, TFuture>(
    handler: TFn,
) -> RequestHandlerFn
where
    TFn: FnMut(TRequest) -> TFuture + Send + Clone + 'static,
    TRequest: for<'de> serde::Deserialize<'de>,
    TResponse: serde::Serialize,
    TError: Display,
    TFuture:
        Future<Output = Result<TResponse, TError>> + Send + Unpin + 'static,
{
    Box::new(move |request: rmpv::Value| {
        let mut handler = handler.clone();
        Box::pin(async move {
            let request: TRequest = rmpv::ext::from_value(request)
                .map_err(JsBridgeErrorInner::ValueConversion)?;
            let response = handler(request).await.map_err(|e| {
                JsBridgeErrorInner::Unknown(eyre::eyre!(e.to_string()))
            })?;
            Ok(rmpv::ext::to_value(response)
                .map_err(JsBridgeErrorInner::ValueConversion)?)
        })
    })
}

impl<TTransport: Transport> BridgeRpc<TTransport> {
    pub fn new(
        transport: impl Into<TTransport>,
        handlers: HashMap<String, RequestHandlerFn>,
    ) -> Self {
        Self {
            transport: transport.into(),
            response_pipes: Arc::new(Mutex::new(HashMap::new())),
            request_handlers: Arc::new(Mutex::new(handlers)),
        }
    }
}

impl<TTransport: Transport> BridgeRpc<TTransport> {
    pub async fn close(&self) -> JsBridgeResult<()> {
        let bytes = rmp_serde::to_vec(&FRAME_CLOSE)
            .map_err(JsBridgeErrorInner::Serialization)?;

        self.transport
            .send(bytes)
            .await
            .map_err(JsBridgeErrorInner::transport)?;

        Ok(())
    }

    async fn handle_internal_op(&self, op: InternalOp) -> JsBridgeResult<()> {
        match op {
            InternalOp::Close => Ok(()),
        }
    }

    async fn handle_request(
        &self,
        request: BridgeRequest<rmpv::Value>,
    ) -> JsBridgeResult<()> {
        if let Some(handler) =
            self.request_handlers.lock().await.get_mut(&request.path)
        {
            let response = (*handler)(request.data).await.map_err(|e| {
                JsBridgeErrorInner::Unknown(eyre::eyre!(e.to_string()))
            })?;

            let response = BridgeResponse {
                request_id: request.request_id,
                data: Some(response),
                error: None,
            };

            let bytes = rmp_serde::to_vec(&f_res(response))
                .map_err(JsBridgeErrorInner::Serialization)?;

            self.transport
                .send(bytes)
                .await
                .map_err(JsBridgeErrorInner::transport)?;
        } else {
            return Err(JsBridgeErrorInner::Unknown(eyre::eyre!(
                "No handler found for path: {}",
                request.path
            ))
            .into());
        }

        Ok(())
    }

    pub async fn run(&self) -> JsBridgeResult<()> {
        loop {
            let bytes = self
                .transport
                .receive()
                .await
                .map_err(JsBridgeErrorInner::transport)?;

            let response: BridgeFrame<rmpv::Value> =
                rmp_serde::from_slice(&bytes)
                    .map_err(JsBridgeErrorInner::Deserialization)?;

            match response {
                BridgeFrame::Response(response) => {
                    let req_id = response.request_id;

                    let mut response_pipes = self.response_pipes.lock().await;
                    if let Some(response_tx) = response_pipes.remove(&req_id) {
                        let response = if let Some(error) = response.error {
                            Err(error)
                        } else if let Some(data) = response.data {
                            Ok(data)
                        } else {
                            continue; // No data or error, skip
                        };

                        response_tx.send(response).map_err(|_| {
                            JsBridgeErrorInner::send("Failed to send response")
                        })?;
                    }
                }
                BridgeFrame::InternalOp(op) => {
                    self.handle_internal_op(op).await?;
                }
                BridgeFrame::Request(request) => {
                    self.handle_request(request).await?;
                }
            }
        }
    }

    pub async fn request<TRequestData, TResponseData>(
        &self,
        path: impl Into<String>,
        data: TRequestData,
    ) -> JsBridgeResult<TResponseData>
    where
        TRequestData: Serialize,
        TResponseData: for<'de> serde::Deserialize<'de>,
    {
        let request_id = RequestId::new();
        let request = BridgeRequest {
            request_id,
            data,
            path: path.into(),
        };
        let bytes = rmp_serde::to_vec(&f_req(request))
            .map_err(JsBridgeErrorInner::Serialization)?;

        self.transport
            .send(bytes)
            .await
            .map_err(JsBridgeErrorInner::transport)?;

        let (response_tx, response_rx) = oneshot::channel();

        self.response_pipes
            .lock()
            .await
            .insert(request_id, response_tx);

        let response = response_rx
            .await
            .map_err(JsBridgeErrorInner::Receive)?
            .map_err(|e| {
                JsBridgeErrorInner::Unknown(eyre::eyre!(
                    "Error: {}",
                    e.error_message
                ))
            })?;

        let response_data = rmpv::ext::from_value(response)
            .map_err(JsBridgeErrorInner::ValueConversion)?;

        Ok(response_data)
    }
}
