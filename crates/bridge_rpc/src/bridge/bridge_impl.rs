use super::frame::*;
use super::request_id::*;
use futures::FutureExt as _;
use futures::future::BoxFuture;

use std::time::Duration;
use std::{collections::HashMap, fmt::Display, sync::Arc};

use serde::Serialize;
use tokio::sync::{
    Mutex,
    oneshot::{self},
};

use crate::JsBridgeErrorInner;
use crate::JsBridgeResult;
use crate::Transport;

type TxPipe = oneshot::Sender<Result<rmpv::Value, ErrorData>>;

type TxPipeMaps = HashMap<RequestId, TxPipe>;

pub type RequestHandlerFnFuture =
    BoxFuture<'static, JsBridgeResult<rmpv::Value>>;

pub type RequestHandlerFn =
    Box<dyn FnMut(rmpv::Value) -> RequestHandlerFnFuture + Send>;

pub struct BridgeRpc<TTransport: Transport> {
    transport: Arc<Mutex<TTransport>>,
    response_pipes: Arc<Mutex<TxPipeMaps>>,
    request_handlers: Arc<Mutex<HashMap<String, RequestHandlerFn>>>,
    pending_probe: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    // Response buffer for testing purposes only to make it easier to test
    // #[cfg(test)]
    // response_buffer: Arc<Mutex<HashMap<RequestId, rmpv::Value>>>,
}

impl<TTransport: Transport> Clone for BridgeRpc<TTransport> {
    fn clone(&self) -> Self {
        Self {
            transport: self.transport.clone(),
            response_pipes: self.response_pipes.clone(),
            request_handlers: self.request_handlers.clone(),
            pending_probe: self.pending_probe.clone(),
        }
    }
}

pub fn create_handler<TFn, TRequest, TResponse, TError, TFuture>(
    handler: TFn,
) -> RequestHandlerFn
where
    TFn: FnMut(TRequest) -> TFuture + Send + Clone + 'static,
    TRequest: for<'de> serde::Deserialize<'de>,
    TResponse: serde::Serialize,
    TError: Display,
    TFuture: Future<Output = Result<TResponse, TError>> + Send + 'static,
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
        transport: TTransport,
        handlers: HashMap<String, RequestHandlerFn>,
    ) -> Self {
        Self {
            transport: Arc::new(Mutex::new(transport)),
            response_pipes: Arc::new(Mutex::new(HashMap::new())),
            request_handlers: Arc::new(Mutex::new(handlers)),
            pending_probe: Arc::new(Mutex::new(None)),
            // #[cfg(test)]
            // response_buffer: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl<TTransport: Transport> BridgeRpc<TTransport> {
    pub async fn has_handler(&self, path: &str) -> bool {
        self.request_handlers.lock().await.contains_key(path)
    }

    pub async fn close(&self) -> JsBridgeResult<()> {
        self.send_frame(&FRAME_CLOSE).await?;

        Ok(())
    }

    async fn handle_request(
        &self,
        request: BridgeRequest<rmpv::Value>,
    ) -> JsBridgeResult<()> {
        let r_id = request.id;

        let bytes = (self.get_response(request, r_id).await)
            .map_err(JsBridgeErrorInner::Serialization)?;

        self.transport
            .lock()
            .await
            .send(bytes.into())
            .await
            .map_err(JsBridgeErrorInner::transport)?;

        Ok(())
    }

    async fn get_response(
        &self,
        request: BridgeRequest<rmpv::Value>,
        r_id: RequestId,
    ) -> Result<Vec<u8>, rmp_serde::encode::Error> {
        if let Some(handler) =
            self.request_handlers.lock().await.get_mut(&request.path)
        {
            let response = (*handler)(request.data).await;
            match response {
                Ok(response) => {
                    rmp_serde::to_vec(&f_res_success(r_id, response))
                }
                Err(error) => {
                    rmp_serde::to_vec(&f_res_error(r_id, error.to_string()))
                }
            }
        } else {
            let error_message =
                format!("No handler found for path: '{}'", request.path);

            rmp_serde::to_vec(&f_res_error(r_id, error_message))
        }
    }

    pub async fn run(&self) -> JsBridgeResult<()> {
        loop {
            let bytes = self
                .transport
                .lock()
                .await
                .receive()
                .await
                .map_err(JsBridgeErrorInner::transport)?;

            let response: BridgeFrame<rmpv::Value> =
                rmp_serde::from_slice(&bytes)
                    .map_err(JsBridgeErrorInner::Deserialization)?;

            match response {
                BridgeFrame::Response(response) => {
                    // if the RPC is not running, we don't need to handle the
                    // response

                    let req_id = response.id;

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
                    } else {
                        trace::warn!(
                            "No response pipe found for request ID: {}",
                            req_id
                        );
                    }
                }
                BridgeFrame::InternalOp(op) => match op {
                    InternalOp::Close => {
                        self.send_frame(&FRAME_CLOSE_ACK).await?;
                        return Ok(());
                    }
                    InternalOp::CloseAck => {
                        trace::debug!("Received close ack, closing RPC");
                        return Ok(());
                    }
                    InternalOp::Probe => {
                        trace::debug!("Received probe, sending probe ack");
                        self.send_frame(&FRAME_PROBE_ACK).await?;
                    }
                    InternalOp::ProbeAck => {
                        trace::debug!("Received probe ack");
                        if let Some(rx) = self.pending_probe.lock().await.take()
                        {
                            rx.send(()).map_err(|_| {
                                JsBridgeErrorInner::send(
                                    "Failed to send probe ack",
                                )
                            })?;
                        }
                    }
                },
                BridgeFrame::Request(request) => {
                    // if the RPC is not running, we don't need to handle the
                    // request
                    self.handle_request(request).await?;
                }
            }
        }
    }

    async fn send_bytes_as_frame(&self, bytes: Vec<u8>) -> JsBridgeResult<()> {
        self.transport
            .lock()
            .await
            .send(bytes.into())
            .await
            .map_err(JsBridgeErrorInner::transport)?;
        Ok(())
    }

    async fn send_frame<TData>(
        &self,
        frame: &BridgeFrame<TData>,
    ) -> JsBridgeResult<()>
    where
        TData: Serialize,
    {
        let bytes = rmp_serde::to_vec(&frame)
            .map_err(JsBridgeErrorInner::Serialization)?;

        self.send_bytes_as_frame(bytes).await?;
        Ok(())
    }

    pub(crate) async fn request_with_id<TRequestData, TResponseData>(
        &self,
        request_id: RequestId,
        path: impl Into<String>,
        data: TRequestData,
    ) -> JsBridgeResult<TResponseData>
    where
        TRequestData: Serialize,
        TResponseData: for<'de> serde::Deserialize<'de>,
    {
        self.send_frame(&f_req(request_id, path, data)).await?;

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

    #[inline(always)]
    pub async fn request<TRequestData, TResponseData>(
        &self,
        path: impl Into<String>,
        data: TRequestData,
    ) -> JsBridgeResult<TResponseData>
    where
        TRequestData: Serialize,
        TResponseData: for<'de> serde::Deserialize<'de>,
    {
        self.request_with_id(RequestId::new(), path, data).await
    }

    pub async fn probe(&self, timeout: Duration) -> JsBridgeResult<bool> {
        if self.has_pending_probe().await {
            Err(JsBridgeErrorInner::ProbeInProgress)?;
        }

        let (tx, rx) = oneshot::channel();
        *self.pending_probe.lock().await = Some(tx);

        self.send_frame(&FRAME_PROBE).await?;

        let result = tokio::time::timeout(timeout, rx.map(|_| true))
            .await
            .map_err(|_| {
                JsBridgeErrorInner::Timeout(format!(
                    "Probe timed out after {}ms",
                    timeout.as_millis()
                ))
            });

        // clear the pending probe if it exists
        _ = self.pending_probe.lock().await.take();

        Ok(result?)
    }

    pub async fn has_pending_probe(&self) -> bool {
        self.pending_probe.lock().await.is_some()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{BridgeRpcBuilder, MockTransport};
    use ntest::timeout;
    use rmp_serde;
    use serde::{Deserialize, Serialize};
    use tokio::time::sleep;

    #[derive(Serialize, Deserialize, Debug)]
    struct MockRequestData {
        data: String,
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct MockResponseData {
        data: String,
    }

    fn req_data(data: impl Into<String>) -> MockRequestData {
        MockRequestData { data: data.into() }
    }

    fn res_data(data: impl Into<String>) -> MockResponseData {
        MockResponseData { data: data.into() }
    }

    fn mt() -> MockTransport {
        MockTransport::new()
    }

    #[tokio::test]
    async fn test_create_bridge_rpc() {
        let transport = mt();
        let rpc = BridgeRpc::new(transport, HashMap::new());

        assert!(
            !rpc.has_handler("test_path").await,
            "handler should not be registered"
        );
    }

    #[tokio::test]
    #[timeout(1000)]
    async fn test_request() {
        let mut transport = mt();

        let req_id = RequestId::new();

        transport.expect_send().returning(move |bytes| {
            let request: BridgeFrame<MockRequestData> =
                rmp_serde::from_slice(&bytes)
                    .expect("Failed to deserialize request");

            assert!(
                matches!(request, BridgeFrame::Request(_)),
                "Expected request frame"
            );

            if let BridgeFrame::Request(request) = request {
                assert_eq!(request.path, "test_path");
                assert_eq!(request.id, req_id);
            }

            Ok(())
        });

        let mut sent_success = false;
        let mut sent_start_ack = false;
        transport.expect_receive().returning(move || {
            if !sent_start_ack {
                sent_start_ack = true;
                return Ok(rmp_serde::to_vec(&FRAME_PROBE_ACK)
                    .expect("Failed to serialize start ack frame")
                    .into());
            }

            if !sent_success {
                sent_success = true;

                return Ok(rmp_serde::to_vec(&f_res_success(
                    req_id,
                    res_data("test_data"),
                ))
                .expect("Failed to serialize response")
                .into());
            }

            Ok(rmp_serde::to_vec(&FRAME_CLOSE)
                .expect("Failed to serialize close frame")
                .into())
        });

        let rpc = BridgeRpc::new(transport, HashMap::new());

        let response = {
            let rpc = rpc.clone();
            tokio::spawn(async move {
                let response = rpc
                    .request_with_id::<_, MockResponseData>(
                        req_id,
                        "test_path",
                        req_data("test_data"),
                    )
                    .await
                    .expect("Request failed");

                assert_eq!(response.data, "test_data");
            })
        };

        // run the RPC to populate response buffer
        let run = {
            let rpc = rpc.clone();
            tokio::spawn(async move {
                sleep(Duration::from_millis(100)).await;
                rpc.run().await.expect("Failed to run RPC");
            })
        };

        _ = tokio::join!(response, run);
    }

    #[tokio::test]
    #[timeout(1000)]
    async fn test_close() {
        let mut transport = mt();

        transport.expect_send().returning(move |bytes| {
            let frame: BridgeFrame<()> = rmp_serde::from_slice(&bytes)
                .expect("Failed to deserialize frame");

            assert!(
                matches!(frame, BridgeFrame::InternalOp(InternalOp::Close)),
                "Expected close frame"
            );

            Ok(())
        });

        let rpc = BridgeRpc::new(transport, HashMap::new());

        rpc.close().await.expect("Failed to close RPC");
    }

    #[tokio::test]
    #[timeout(1000)]
    async fn test_probe() {
        let mut transport = mt();

        let mut received_probe = false;
        transport.expect_send().returning(move |bytes| {
            if !received_probe {
                received_probe = true;
                let frame = rmp_serde::from_slice::<BridgeFrame<()>>(&bytes)
                    .expect("Failed to deserialize frame");
                assert!(
                    matches!(frame, BridgeFrame::InternalOp(InternalOp::Probe)),
                    "Expected probe frame"
                );
            }

            Ok(())
        });
        let mut sent_probe_ack = false;
        transport.expect_receive().returning(move || {
            if !sent_probe_ack {
                sent_probe_ack = true;
                return Ok(rmp_serde::to_vec(&FRAME_PROBE_ACK)
                    .expect("Failed to serialize probe ack frame")
                    .into());
            }

            Ok(rmp_serde::to_vec(&FRAME_CLOSE)
                .expect("Failed to serialize close frame")
                .into())
        });

        let rpc = BridgeRpcBuilder::new(transport).build();

        let response = rpc.probe(Duration::from_millis(100));
        let run = rpc.run();

        let (response, ..) = tokio::join!(response, run);

        assert!(response.is_ok(), "Probe should return a valid result");
        assert!(response.unwrap(), "Probe failed");
        assert!(!rpc.has_pending_probe().await);
    }

    #[tokio::test]
    #[timeout(1000)]
    async fn test_respond_existing_path() {
        let mut transport = mt();

        let req_id = RequestId::new();

        let mut received_response = false;
        transport.expect_send().returning(move |bytes| {
            if received_response {
                return Ok(());
            }

            let response: BridgeFrame<MockResponseData> =
                rmp_serde::from_slice(&bytes)
                    .expect("Failed to deserialize response");

            assert!(
                matches!(response, BridgeFrame::Response(_)),
                "Expected response frame"
            );

            received_response = true;

            if let BridgeFrame::Response(response) = response {
                assert_eq!(response.id, req_id);
                assert_eq!(
                    response.data.expect("Should have data").data,
                    "test_data"
                );
            }

            Ok(())
        });

        let mut sent_success = false;
        transport.expect_receive().returning(move || {
            if sent_success {
                return Ok(rmp_serde::to_vec(&FRAME_CLOSE)
                    .expect("Failed to serialize close frame")
                    .into());
            }

            sent_success = true;

            Ok(rmp_serde::to_vec(&f_req(
                req_id,
                "test_path",
                req_data("test_data"),
            ))
            .expect("Failed to serialize response")
            .into())
        });

        let rpc = BridgeRpcBuilder::new(transport)
            .handle("test_path", async |req: MockRequestData| {
                Ok::<_, String>(MockResponseData { data: req.data })
            })
            .build();

        // run the RPC to populate response buffer
        rpc.run().await.expect("Failed to run RPC");
    }

    #[tokio::test]
    #[timeout(1000)]
    async fn test_respond_non_existing_path() {
        let mut transport = mt();

        let req_id = RequestId::new();

        let mut received_response = false;
        transport.expect_send().returning(move |bytes| {
            if received_response {
                return Ok(());
            }

            let response: BridgeFrame<MockResponseData> =
                rmp_serde::from_slice(&bytes)
                    .expect("Failed to deserialize response");

            assert!(
                matches!(response, BridgeFrame::Response(_)),
                "Expected response frame"
            );

            received_response = true;
            if let BridgeFrame::Response(response) = response {
                assert_eq!(response.id, req_id);
                assert_eq!(
                    response.error.expect("Should have error").error_message,
                    "No handler found for path: 'test_path_wrong'"
                );
            }

            Ok(())
        });

        let mut sent_success = false;
        transport.expect_receive().returning(move || {
            if sent_success {
                return Ok(rmp_serde::to_vec(&FRAME_CLOSE)
                    .expect("Failed to serialize close frame")
                    .into());
            }

            sent_success = true;

            Ok(rmp_serde::to_vec(&f_req(
                req_id,
                "test_path_wrong",
                req_data("test_data"),
            ))
            .expect("Failed to serialize response")
            .into())
        });

        let rpc = BridgeRpc::new(transport, HashMap::new());

        // run the RPC to populate response buffer
        rpc.run().await.expect("Failed to run RPC");
    }
}
