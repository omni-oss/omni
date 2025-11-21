use super::frame::*;
use super::id::*;
use futures::FutureExt as _;
use futures::future::BoxFuture;
use tokio::sync::mpsc;
use tokio_stream::StreamExt as _;

use std::pin::Pin;
use std::time::Duration;
use std::{collections::HashMap, fmt::Display, sync::Arc};

use serde::Serialize;
use tokio::sync::{
    Mutex,
    oneshot::{self},
};
use tokio_stream::Stream as TokioStream;

use crate::BridgeRpcError;
use crate::BridgeRpcErrorInner;
use crate::BridgeRpcResult;
use crate::StreamError;
use crate::StreamErrorInner;
use crate::StreamHandle as BridgeStream;
use crate::Transport;
use crate::bridge::utils::send_frame;
use crate::bridge::utils::serialize;

type ResponsePipe = oneshot::Sender<Result<rmpv::Value, ErrorData>>;
type ResponsePipeMaps = HashMap<Id, ResponsePipe>;

type StreamPipe = mpsc::Sender<Result<rmpv::Value, eyre::Report>>;
type StreamPipeMaps = HashMap<Id, StreamPipe>;

type StreamStartResponsePipe = oneshot::Sender<StreamStartResponse>;
type StreamStartResponsePipeMaps = HashMap<Id, StreamStartResponsePipe>;

pub type BoxStream<'a, T> = Pin<Box<dyn TokioStream<Item = T> + Send + 'a>>;

pub type RequestHandlerFnFuture =
    BoxFuture<'static, BridgeRpcResult<rmpv::Value>>;

pub type StreamHandlerFnFuture = BoxFuture<'static, BridgeRpcResult<()>>;

pub type RequestHandlerFn =
    Box<dyn FnMut(rmpv::Value) -> RequestHandlerFnFuture + Send>;

pub struct StreamContext<
    TStartData = (),
    TStreamData = (),
    TError = StreamError,
> {
    pub start_data: Option<TStartData>,
    pub stream: BoxStream<'static, Result<TStreamData, TError>>,
}

pub struct RequestContext<TData = ()> {
    pub data: TData,
}

pub type StreamHandlerFn = Box<
    dyn FnMut(
            StreamContext<rmpv::Value, rmpv::Value, eyre::Report>,
        ) -> StreamHandlerFnFuture
        + Send,
>;

pub struct BridgeRpc<TTransport: Transport> {
    transport: Arc<TTransport>,
    message_response_pipes: Arc<Mutex<ResponsePipeMaps>>,
    stream_pipes: Arc<Mutex<StreamPipeMaps>>,
    stream_start_response_pipes: Arc<Mutex<StreamStartResponsePipeMaps>>,
    request_handlers: Arc<Mutex<HashMap<String, RequestHandlerFn>>>,
    stream_handlers: Arc<Mutex<HashMap<String, StreamHandlerFn>>>,
    pending_probe: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    // Response buffer for testing purposes only to make it easier to test
    // #[cfg(test)]
    // response_buffer: Arc<Mutex<HashMap<RequestId, rmpv::Value>>>,
}

impl<TTransport: Transport> Clone for BridgeRpc<TTransport> {
    fn clone(&self) -> Self {
        Self {
            transport: self.transport.clone(),
            message_response_pipes: self.message_response_pipes.clone(),
            request_handlers: self.request_handlers.clone(),
            pending_probe: self.pending_probe.clone(),
            stream_handlers: self.stream_handlers.clone(),
            stream_pipes: self.stream_pipes.clone(),
            stream_start_response_pipes: self
                .stream_start_response_pipes
                .clone(),
        }
    }
}

pub fn create_request_handler<TRequestData, TResponse, TError, TFuture, TFn>(
    handler: TFn,
) -> RequestHandlerFn
where
    TRequestData: for<'de> serde::Deserialize<'de>,
    TResponse: serde::Serialize,
    TError: Display,
    TFuture: Future<Output = Result<TResponse, TError>> + Send + 'static,
    TFn:
        FnMut(RequestContext<TRequestData>) -> TFuture + Send + Clone + 'static,
{
    Box::new(move |request: rmpv::Value| {
        let mut handler = handler.clone();
        Box::pin(async move {
            let request: TRequestData = rmpv::ext::from_value(request)
                .map_err(BridgeRpcErrorInner::ValueConversion)?;
            let response = handler(RequestContext { data: request })
                .await
                .map_err(|e| {
                    BridgeRpcErrorInner::Unknown(eyre::eyre!(e.to_string()))
                })?;
            Ok(rmpv::ext::to_value(response)
                .map_err(BridgeRpcErrorInner::ValueConversion)?)
        })
    })
}

pub fn create_stream_handler<TStartData, TStreamData, TError, TFuture, TFn>(
    handler: TFn,
) -> StreamHandlerFn
where
    TStartData: for<'de> serde::Deserialize<'de>,
    TStreamData: for<'de> serde::Deserialize<'de>,
    TError: Display,
    TFuture: Future<Output = Result<(), TError>> + Send + 'static,
    TFn: FnMut(StreamContext<TStartData, TStreamData, StreamError>) -> TFuture
        + Send
        + Clone
        + 'static,
{
    Box::new(move |context| {
        let mut handler = handler.clone();
        Box::pin(async move {
            let start_data = context
                .start_data
                .map(|data| {
                    rmpv::ext::from_value::<TStartData>(data).map_err(|e| {
                        BridgeRpcError(BridgeRpcErrorInner::ValueConversion(e))
                    })
                })
                .transpose()?;

            let stream = context.stream.map(
                |result| -> Result<TStreamData, StreamError> {
                    match result {
                        Ok(data) => {
                            Ok(rmpv::ext::from_value::<TStreamData>(data)
                                .map_err(|e| {
                                    StreamError(
                                        StreamErrorInner::ValueConversion(e),
                                    )
                                })?)
                        }
                        Err(e) => Err(StreamError(StreamErrorInner::Custom(e))),
                    }
                },
            );
            let stream = Box::pin(stream);

            let context =
                StreamContext::<TStartData, TStreamData> { start_data, stream };

            handler(context).await.map_err(|e| {
                BridgeRpcErrorInner::Unknown(eyre::eyre!(e.to_string()))
            })?;

            Ok(())
        })
    })
}

impl<TTransport: Transport> BridgeRpc<TTransport> {
    pub fn new(
        transport: TTransport,
        request_handlers: HashMap<String, RequestHandlerFn>,
        stream_handlers: HashMap<String, StreamHandlerFn>,
    ) -> Self {
        Self {
            transport: Arc::new(transport),
            message_response_pipes: Arc::new(Mutex::new(HashMap::new())),
            stream_pipes: Arc::new(Mutex::new(HashMap::new())),
            stream_start_response_pipes: Arc::new(Mutex::new(HashMap::new())),
            request_handlers: Arc::new(Mutex::new(request_handlers)),
            stream_handlers: Arc::new(Mutex::new(stream_handlers)),
            pending_probe: Arc::new(Mutex::new(None)),
            // #[cfg(test)]
            // response_buffer: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl<TTransport: Transport> BridgeRpc<TTransport> {
    pub async fn has_request_handler(&self, path: &str) -> bool {
        self.request_handlers.lock().await.contains_key(path)
    }

    pub async fn has_stream_handler(&self, path: &str) -> bool {
        self.stream_handlers.lock().await.contains_key(path)
    }

    pub async fn close(&self) -> BridgeRpcResult<()> {
        send_frame(self.transport.as_ref(), &Frame::close()).await?;

        Ok(())
    }

    async fn get_response(
        &self,
        request: Request<rmpv::Value>,
        r_id: Id,
    ) -> BridgeRpcResult<Vec<u8>> {
        if let Some(handler) =
            self.request_handlers.lock().await.get_mut(&request.path)
        {
            let response = (*handler)(request.data).await;
            match response {
                Ok(response) => {
                    serialize(&Frame::success_response(r_id, response))
                }
                Err(error) => {
                    serialize(&Frame::error_response(r_id, error.to_string()))
                }
            }
        } else {
            let error_message =
                format!("no handler found for path: '{}'", request.path);

            serialize(&Frame::error_response(r_id, error_message))
        }
    }

    pub async fn run(&self) -> BridgeRpcResult<()> {
        loop {
            let bytes = self.transport.receive().await.map_err(|e| {
                BridgeRpcErrorInner::new_transport(eyre::Report::msg(
                    e.to_string(),
                ))
            })?;

            let incoming: Frame<rmpv::Value> = rmp_serde::from_slice(&bytes)
                .map_err(BridgeRpcErrorInner::Deserialization)?;

            println!("incoming: {incoming:?}");

            match incoming.r#type {
                FrameType::Close => {
                    self.handle_close().await?;
                    return Ok(());
                }
                FrameType::CloseAck => {
                    trace::debug!("Received close ack, closing RPC");
                    return Ok(());
                }
                FrameType::Probe => {
                    self.handle_probe().await?;
                }
                FrameType::ProbeAck => {
                    self.handle_probe_ack().await?;
                }
                FrameType::StreamStart => {
                    self.handle_stream_start(&incoming).await?;
                }
                FrameType::StreamStartResponse => {
                    self.handle_stream_start_response(&incoming).await?;
                }
                FrameType::StreamData => {
                    self.handle_stream_data(&incoming).await?;
                }
                FrameType::StreamEnd => {
                    self.handle_stream_end(&incoming).await?;
                }
                FrameType::MessageRequest => {
                    self.handle_message_request(&incoming).await?;
                }
                FrameType::MessageResponse => {
                    self.handle_message_response(&incoming).await?;
                }
            }
        }
    }

    async fn handle_probe(&self) -> Result<(), BridgeRpcError> {
        trace::debug!("Received probe, sending probe ack");
        send_frame(self.transport.as_ref(), &Frame::probe_ack()).await?;
        Ok(())
    }

    async fn handle_close(&self) -> Result<(), BridgeRpcError> {
        send_frame(self.transport.as_ref(), &Frame::close_ack()).await?;
        Ok(())
    }

    async fn handle_probe_ack(&self) -> Result<(), BridgeRpcError> {
        trace::debug!("Received probe ack");
        Ok(if let Some(rx) = self.pending_probe.lock().await.take() {
            rx.send(()).map_err(|_| {
                BridgeRpcErrorInner::new_send(eyre::eyre!(
                    "failed to send probe ack"
                ))
            })?;
        })
    }

    async fn handle_message_request(
        &self,
        incoming: &Frame<rmpv::Value>,
    ) -> Result<(), BridgeRpcError> {
        let request: Request<rmpv::Value> = extract_value(incoming)?;
        let r_id = request.id;
        let bytes = self.get_response(request, r_id).await?;
        self.transport.send(bytes.into()).await.map_err(|e| {
            BridgeRpcErrorInner::new_transport(eyre::Report::msg(e.to_string()))
        })?;
        Ok(())
    }

    async fn handle_message_response(
        &self,
        incoming: &Frame<rmpv::Value>,
    ) -> Result<(), BridgeRpcError> {
        let response: Response<rmpv::Value> = extract_value(&incoming)?;

        let req_id = response.id;

        let mut response_pipes = self.message_response_pipes.lock().await;
        if let Some(response_tx) = response_pipes.remove(&req_id) {
            let response = if let Some(error) = response.error {
                Err(error)
            } else if let Some(data) = response.data {
                Ok(data)
            } else {
                return Ok(());
            };

            response_tx.send(response).map_err(|_| {
                BridgeRpcErrorInner::new_send(eyre::eyre!(
                    "failed to send response"
                ))
            })?;
        } else {
            trace::warn!("No response pipe found for request ID: {}", req_id);
        }

        Ok(())
    }

    async fn handle_stream_end(
        &self,
        incoming: &Frame<rmpv::Value>,
    ) -> Result<(), BridgeRpcError> {
        let tream: StreamEnd = extract_value(incoming)?;
        let sender = self.stream_pipes.lock().await.remove(&tream.id);
        Ok(if let Some(sender) = sender {
            drop(sender);
        })
    }

    async fn handle_stream_data(
        &self,
        incoming: &Frame<rmpv::Value>,
    ) -> Result<(), BridgeRpcError> {
        let stream: StreamData<rmpv::Value> = extract_value(incoming)?;

        let mut pipes = self.stream_pipes.lock().await;
        let sender = pipes.get_mut(&stream.id);
        Ok(if let Some(sender) = sender {
            sender
                .send(Ok(rmpv::ext::to_value(stream.data)?))
                .await
                .map_err(BridgeRpcErrorInner::new_send)?;
        } else {
            trace::warn!("No stream pipe found for stream ID: {}", stream.id);
        })
    }

    async fn handle_stream_start(
        &self,
        incoming: &Frame<rmpv::Value>,
    ) -> Result<(), BridgeRpcError> {
        let stream: StreamStart<rmpv::Value> = extract_value(incoming)?;
        let mut handlers = self.stream_handlers.lock().await;
        let mut handler = handlers.get_mut(&stream.path);
        Ok(if let Some(handler) = handler.as_mut() {
            send_frame(
                self.transport.as_ref(),
                &StreamStartResponseFrame::stream_start_response_ok(stream.id),
            )
            .await?;

            let (tx, rx) = mpsc::channel(1);

            self.stream_pipes.lock().await.insert(stream.id, tx);

            let ctx = StreamContext::<rmpv::Value, rmpv::Value, eyre::Report> {
                start_data: stream.data,
                stream: Box::pin(tokio_stream::wrappers::ReceiverStream::new(
                    rx,
                )),
            };

            handler(ctx).await?;
        } else {
            let error_message =
                format!("no handler found for path: '{}'", stream.path);

            send_frame(
                self.transport.as_ref(),
                &StreamStartResponseFrame::stream_start_response_error(
                    stream.id,
                    error_message,
                ),
            )
            .await?;
        })
    }

    async fn handle_stream_start_response(
        &self,
        incoming: &Frame<rmpv::Value>,
    ) -> Result<(), BridgeRpcError> {
        let value = extract_value::<StreamStartResponse>(incoming)?;
        let id = value.id;

        let mut stream_response_pipes =
            self.stream_start_response_pipes.lock().await;

        if let Some(stream_rejected_tx) =
            stream_response_pipes.remove(&value.id)
        {
            stream_rejected_tx.send(value).map_err(|_| {
                BridgeRpcErrorInner::new_send(eyre::eyre!(
                    "failed to send stream start response (id: {id})",
                ))
            })?;
        } else {
            trace::warn!(
                "No stream start rejected pipe found for stream ID: {}",
                value.id
            );
        }

        Ok(())
    }

    pub(crate) async fn request_with_id<TResponseData, TRequestData>(
        &self,
        request_id: Id,
        path: impl Into<String>,
        data: TRequestData,
    ) -> BridgeRpcResult<TResponseData>
    where
        TRequestData: Serialize,
        TResponseData: for<'de> serde::Deserialize<'de>,
    {
        send_frame(
            self.transport.as_ref(),
            &RequestFrame::request(request_id, path, data),
        )
        .await?;

        let (response_tx, response_rx) = oneshot::channel();

        self.message_response_pipes
            .lock()
            .await
            .insert(request_id, response_tx);

        let response = response_rx
            .await
            .map_err(BridgeRpcErrorInner::Receive)?
            .map_err(|e| {
                BridgeRpcErrorInner::Unknown(eyre::eyre!(
                    "error: {}",
                    e.message
                ))
            })?;

        let response_data = rmpv::ext::from_value(response)
            .map_err(BridgeRpcErrorInner::ValueConversion)?;

        Ok(response_data)
    }

    #[inline(always)]
    pub async fn request<TResponseData, TRequestData>(
        &self,
        path: impl Into<String>,
        data: TRequestData,
    ) -> BridgeRpcResult<TResponseData>
    where
        TRequestData: Serialize,
        TResponseData: for<'de> serde::Deserialize<'de>,
    {
        self.request_with_id::<TResponseData, TRequestData>(
            Id::new(),
            path,
            data,
        )
        .await
    }

    pub(crate) async fn begin_stream_internal<TData: Serialize>(
        &self,
        path: impl Into<String>,
        data: Option<TData>,
    ) -> BridgeRpcResult<BridgeStream<TTransport>> {
        let transport = self.transport.clone();

        let id = Id::new();

        let start_frame = StreamStartFrame::stream_start(id, path, data);

        let (response_tx, response_rx) = oneshot::channel();

        self.stream_start_response_pipes
            .lock()
            .await
            .insert(id, response_tx);

        send_frame(self.transport.as_ref(), &start_frame).await?;

        let response =
            response_rx.await.map_err(BridgeRpcErrorInner::Receive)?;

        if !response.ok {
            return Err(BridgeRpcErrorInner::StreamStartResponse {
                id,
                error: response.error,
            }
            .into());
        }

        Ok(BridgeStream::new(id, transport))
    }

    #[inline(always)]
    pub async fn begin_stream_with_data<TData: Serialize>(
        &self,
        path: impl Into<String>,
        data: TData,
    ) -> BridgeRpcResult<BridgeStream<TTransport>> {
        self.begin_stream_internal(path, Some(data)).await
    }

    #[inline(always)]
    pub async fn begin_stream(
        &self,
        path: impl Into<String>,
    ) -> BridgeRpcResult<BridgeStream<TTransport>> {
        self.begin_stream_internal::<()>(path, None).await
    }

    pub async fn probe(&self, timeout: Duration) -> BridgeRpcResult<bool> {
        if self.has_pending_probe().await {
            Err(BridgeRpcErrorInner::ProbeInProgress)?;
        }

        let (tx, rx) = oneshot::channel();
        *self.pending_probe.lock().await = Some(tx);

        send_frame(self.transport.as_ref(), &Frame::probe()).await?;

        let result = tokio::time::timeout(timeout, rx.map(|_| true))
            .await
            .map_err(|e| {
                BridgeRpcErrorInner::new_timeout(eyre::Report::new(e))
            });

        // clear the pending probe if it exists
        _ = self.pending_probe.lock().await.take();

        Ok(result?)
    }

    pub async fn has_pending_probe(&self) -> bool {
        self.pending_probe.lock().await.is_some()
    }
}

fn extract_value<T>(frame: &Frame<rmpv::Value>) -> BridgeRpcResult<T>
where
    T: for<'de> serde::Deserialize<'de>,
{
    let value = frame
        .data
        .as_ref()
        .ok_or_else(|| BridgeRpcErrorInner::new_missing_data(frame.r#type))?;
    Ok(rmpv::ext::from_value(value.clone())?)
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

    fn mock_transport() -> MockTransport {
        MockTransport::new()
    }

    fn empty_rpc<TTransport: Transport>(
        t: TTransport,
    ) -> BridgeRpc<TTransport> {
        BridgeRpc::new(t, HashMap::new(), HashMap::new())
    }

    #[tokio::test]
    async fn test_create_bridge_rpc() {
        let transport = mock_transport();
        let rpc = BridgeRpc::new(transport, HashMap::new(), HashMap::new());

        assert!(
            !rpc.has_request_handler("test_path").await,
            "handler should not be registered"
        );
    }

    #[tokio::test]
    #[timeout(1000)]
    async fn test_request() {
        let mut transport = mock_transport();

        let req_id = Id::new();

        transport.expect_send().returning(move |bytes| {
            let request: RequestFrame<MockRequestData> =
                rmp_serde::from_slice(&bytes)
                    .expect("Failed to deserialize request");

            assert_eq!(
                request.r#type,
                FrameType::MessageRequest,
                "Expected request frame"
            );

            assert!(request.data.is_some(), "Expected request data");

            if let Some(request) = &request.data {
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
                return Ok(serialize(&Frame::probe_ack())
                    .expect("Failed to serialize start ack frame")
                    .into());
            }

            if !sent_success {
                sent_success = true;

                return Ok(serialize(&Frame::success_response(
                    req_id,
                    res_data("test_data"),
                ))
                .expect("Failed to serialize response")
                .into());
            }

            Ok(serialize(&Frame::close())
                .expect("Failed to serialize close frame")
                .into())
        });

        let rpc = empty_rpc(transport);

        let response = {
            let rpc = rpc.clone();
            tokio::spawn(async move {
                let response = rpc
                    .request_with_id::<MockResponseData, _>(
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
        let mut transport = mock_transport();

        transport.expect_send().returning(move |bytes| {
            let frame: Frame<()> = rmp_serde::from_slice(&bytes)
                .expect("Failed to deserialize frame");

            assert_eq!(frame.r#type, FrameType::Close, "Expected close frame");

            Ok(())
        });

        let rpc = empty_rpc(transport);

        rpc.close().await.expect("Failed to close RPC");
    }

    #[tokio::test]
    #[timeout(1000)]
    async fn test_probe() {
        let mut transport = mock_transport();

        let mut received_probe = false;
        transport.expect_send().returning(move |bytes| {
            if !received_probe {
                received_probe = true;
                let frame = rmp_serde::from_slice::<Frame<()>>(&bytes)
                    .expect("Failed to deserialize frame");

                assert_eq!(
                    frame.r#type,
                    FrameType::Probe,
                    "Expected probe frame"
                );
            }

            Ok(())
        });
        let mut sent_probe_ack = false;
        transport.expect_receive().returning(move || {
            if !sent_probe_ack {
                sent_probe_ack = true;
                return Ok(serialize(&Frame::probe_ack())
                    .expect("Failed to serialize probe ack frame")
                    .into());
            }

            Ok(serialize(&Frame::close())
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
        let mut transport = mock_transport();

        let req_id = Id::new();

        let mut received_response = false;
        transport.expect_send().returning(move |bytes| {
            if received_response {
                return Ok(());
            }

            let response: ResponseFrame<MockResponseData> =
                rmp_serde::from_slice(&bytes)
                    .expect("Failed to deserialize response");

            assert_eq!(
                response.r#type,
                FrameType::MessageResponse,
                "Expected response frame"
            );

            assert!(response.data.is_some(), "Expected response data");

            received_response = true;

            if let Some(response) = &response.data {
                assert_eq!(response.id, req_id);
                assert_eq!(
                    response.data.as_ref().expect("Should have data").data,
                    "test_data"
                );
            }

            Ok(())
        });

        let mut sent_success = false;
        transport.expect_receive().returning(move || {
            if sent_success {
                return Ok(serialize(&Frame::close())
                    .expect("Failed to serialize close frame")
                    .into());
            }

            sent_success = true;

            Ok(serialize(&Frame::request(
                req_id,
                "test_path",
                req_data("test_data"),
            ))
            .expect("Failed to serialize response")
            .into())
        });

        let rpc = BridgeRpcBuilder::new(transport)
            .request_handler(
                "test_path",
                async |req: RequestContext<MockRequestData>| {
                    Ok::<_, String>(MockResponseData {
                        data: req.data.data,
                    })
                },
            )
            .build();

        // run the RPC to populate response buffer
        rpc.run().await.expect("Failed to run RPC");
    }

    #[tokio::test]
    #[timeout(1000)]
    async fn test_respond_non_existing_path() {
        let mut transport = mock_transport();

        let req_id = Id::new();

        let mut received_response = false;
        transport.expect_send().returning(move |bytes| {
            if received_response {
                return Ok(());
            }

            let response: ResponseFrame<MockResponseData> =
                rmp_serde::from_slice(&bytes)
                    .expect("Failed to deserialize response");

            assert_eq!(
                response.r#type,
                FrameType::MessageResponse,
                "Expected response frame"
            );

            assert!(response.data.is_some(), "Expected response data");

            received_response = true;
            if let Some(response) = &response.data {
                assert_eq!(response.id, req_id);
                assert_eq!(
                    response.error.as_ref().expect("Should have error").message,
                    "no handler found for path: 'test_path_wrong'"
                );
            }

            Ok(())
        });

        let mut sent_success = false;
        transport.expect_receive().returning(move || {
            if sent_success {
                return Ok(serialize(&Frame::close())
                    .expect("Failed to serialize close frame")
                    .into());
            }

            sent_success = true;

            Ok(serialize(&Frame::request(
                req_id,
                "test_path_wrong",
                req_data("test_data"),
            ))
            .expect("Failed to serialize response")
            .into())
        });

        let rpc = empty_rpc(transport);

        // run the RPC to populate response buffer
        rpc.run().await.expect("Failed to run RPC");
    }

    #[tokio::test]
    #[timeout(1000)]
    async fn test_respond_stream_start_existing_path() {
        let mut transport = mock_transport();

        let mut received_response = false;
        transport.expect_send().returning(move |bytes| {
            if received_response {
                return Ok(());
            }

            let response: StreamStartResponseFrame =
                rmp_serde::from_slice(&bytes)
                    .expect("Failed to deserialize response");

            assert_eq!(
                response.r#type,
                FrameType::StreamStartResponse,
                "Expected response frame"
            );

            assert!(response.data.is_some(), "Expected response data");

            received_response = true;

            if let Some(response) = &response.data {
                assert!(response.ok, "Expected success response");
            }

            Ok(())
        });

        let mut sent_success = false;
        transport.expect_receive().returning(move || {
            if sent_success {
                return Ok(serialize(&Frame::close())
                    .expect("Failed to serialize close frame")
                    .into());
            }

            sent_success = true;

            Ok(serialize(&StreamStartFrame::<()>::stream_start(
                Id::new(),
                "test_path",
                None,
            ))
            .expect("Failed to serialize response")
            .into())
        });

        let rpc = BridgeRpcBuilder::new(transport)
            .stream_handler("test_path", |_: StreamContext| async move {
                Ok::<_, String>(())
            })
            .build();

        // run the RPC to populate response buffer
        rpc.run().await.expect("Failed to run RPC");
    }

    #[tokio::test]
    #[timeout(1000)]
    async fn test_respond_stream_start_non_existing_path() {
        let mut transport = mock_transport();

        let mut received_response = false;
        transport.expect_send().returning(move |bytes| {
            if received_response {
                return Ok(());
            }

            let response: StreamStartResponseFrame =
                rmp_serde::from_slice(&bytes)
                    .expect("Failed to deserialize response");

            assert_eq!(
                response.r#type,
                FrameType::StreamStartResponse,
                "Expected response frame"
            );

            assert!(response.data.is_some(), "Expected response data");

            received_response = true;

            if let Some(response) = &response.data {
                assert!(!response.ok, "Expected error response");
                assert_eq!(
                    response.error.as_ref().expect("Should have error").message,
                    "no handler found for path: 'test_path_wrong'"
                );
            }

            Ok(())
        });

        let mut sent_success = false;
        transport.expect_receive().returning(move || {
            if sent_success {
                return Ok(serialize(&Frame::close())
                    .expect("Failed to serialize close frame")
                    .into());
            }

            sent_success = true;

            Ok(serialize(&StreamStartFrame::<()>::stream_start(
                Id::new(),
                "test_path_wrong",
                None,
            ))
            .expect("Failed to serialize response")
            .into())
        });

        let rpc = empty_rpc(transport);

        // run the RPC to populate response buffer
        rpc.run().await.expect("Failed to run RPC");
    }
}
