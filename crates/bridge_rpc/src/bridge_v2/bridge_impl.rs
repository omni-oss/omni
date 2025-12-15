use futures::FutureExt as _;
use futures::future::BoxFuture;
use strum::IntoDiscriminant;
use tokio::sync::mpsc;
use tokio::sync::{
    Mutex,
    oneshot::{self},
};
use tokio::task::AbortHandle;
use tokio::task::JoinSet;
use tokio_stream::Stream as TokioStream;
use tracing::Instrument;

use std::ops::ControlFlow;
use std::pin::Pin;
use std::time::Duration;
use std::{collections::HashMap, sync::Arc};

use super::BridgeRpcError;
use super::BridgeRpcErrorInner;
use super::BridgeRpcResult;
use super::StreamError;
use super::constants::{BYTES_WORKER_BUFFER_SIZE, RESPONSE_BUFFER_SIZE};
use super::frame::*;
use super::id::*;
use super::request::*;
use super::utils::send_bytes_to_transport;
use super::utils::send_frame_to_channel;
use crate::Transport;

pub type BoxStream<'a, T> = Pin<Box<dyn TokioStream<Item = T> + Send + 'a>>;

pub type HandlerFnFuture = BoxFuture<'static, BridgeRpcResult<()>>;

type ResponseReceiver = mpsc::Receiver<ChannelResponseFrame>;
type ResponseSender = mpsc::Sender<ChannelResponseFrame>;
type ResponseSenders = HashMap<Id, ResponseSender>;

type ErrorReceiver = oneshot::Receiver<ResponseError>;
type ErrorSender = oneshot::Sender<ResponseError>;
type ErrorSenders = HashMap<Id, ErrorSender>;

pub struct HandlerContext<
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

pub type HandlerFn = Arc<
    dyn Fn(
            HandlerContext<rmpv::Value, rmpv::Value, eyre::Report>,
        ) -> HandlerFnFuture
        + Send
        + Sync,
>;

pub struct BridgeRpc<TTransport: Transport> {
    id: Id,
    transport: Arc<TTransport>,
    handlers: Arc<Mutex<HashMap<String, HandlerFn>>>,
    pending_ping: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    bytes_worker: Arc<Mutex<Option<BytesWorker>>>,
    handler_tasks: Arc<Mutex<Option<JoinSet<Result<(), BridgeRpcError>>>>>,
    response_senders: Arc<Mutex<ResponseSenders>>,
    error_senders: Arc<Mutex<ErrorSenders>>,
}

struct BytesWorker {
    pub(crate) sender: mpsc::Sender<Vec<u8>>,
    pub(crate) abort_handle: AbortHandle,
}

impl<TTransport: Transport> Clone for BridgeRpc<TTransport> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            transport: self.transport.clone(),
            pending_ping: self.pending_ping.clone(),
            handlers: self.handlers.clone(),
            bytes_worker: self.bytes_worker.clone(),
            handler_tasks: self.handler_tasks.clone(),
            response_senders: self.response_senders.clone(),
            error_senders: self.error_senders.clone(),
        }
    }
}

impl<TTransport: Transport> BridgeRpc<TTransport> {
    pub fn id(&self) -> Id {
        self.id
    }
}

// pub fn create_request_handler<TRequestData, TResponse, TError, TFuture, TFn>(
//     handler: TFn,
// ) -> RequestHandlerFn
// where
//     TRequestData: for<'de> serde::Deserialize<'de>,
//     TResponse: serde::Serialize,
//     TError: Display,
//     TFuture: Future<Output = Result<TResponse, TError>> + Send + 'static,
//     TFn: Fn(RequestContext<TRequestData>) -> TFuture
//         + Send
//         + Sync
//         + Clone
//         + 'static,
// {
//     Box::new(move |request: rmpv::Value| {
//         let handler = handler.clone();
//         Box::pin(async move {
//             let request: TRequestData = rmpv::ext::from_value(request)
//                 .map_err(BridgeRpcErrorInner::ValueConversion)?;
//             let response = handler(RequestContext { data: request })
//                 .await
//                 .map_err(|e| {
//                     BridgeRpcErrorInner::Unknown(eyre::eyre!(e.to_string()))
//                 })?;
//             Ok(rmpv::ext::to_value(response)
//                 .map_err(BridgeRpcErrorInner::ValueConversion)?)
//         })
//     })
// }

// pub fn create_handler<TStartData, TStreamData, TError, TFuture, TFn>(
//     handler: TFn,
// ) -> HandlerFn
// where
//     TStartData: for<'de> serde::Deserialize<'de>,
//     TStreamData: for<'de> serde::Deserialize<'de>,
//     TError: Display,
//     TFuture: Future<Output = Result<(), TError>> + Send + 'static,
//     TFn: Fn(HandlerContext<TStartData, TStreamData, StreamError>) -> TFuture
//         + Send
//         + Sync
//         + Clone
//         + 'static,
// {
//     Arc::new(move |context| {
//         let handler = handler.clone();
//         Box::pin(async move {
//             let start_data = context
//                 .start_data
//                 .map(|data| {
//                     rmpv::ext::from_value::<TStartData>(data).map_err(|e| {
//                         BridgeRpcError(BridgeRpcErrorInner::ValueConversion(e))
//                     })
//                 })
//                 .transpose()?;

//             let stream = context.stream.map(
//                 |result| -> Result<TStreamData, StreamError> {
//                     match result {
//                         Ok(data) => {
//                             Ok(rmpv::ext::from_value::<TStreamData>(data)
//                                 .map_err(|e| {
//                                     StreamError(
//                                         StreamErrorInner::ValueConversion(e),
//                                     )
//                                 })?)
//                         }
//                         Err(e) => Err(StreamError(StreamErrorInner::Custom(e))),
//                     }
//                 },
//             );
//             let stream = Box::pin(stream);

//             let context = HandlerContext::<TStartData, TStreamData> {
//                 start_data,
//                 stream,
//             };

//             handler(context).await.map_err(|e| {
//                 BridgeRpcErrorInner::Unknown(eyre::eyre!(e.to_string()))
//             })?;

//             Ok(())
//         })
//     })
// }

impl<TTransport: Transport> BridgeRpc<TTransport> {
    pub fn new(
        transport: TTransport,
        handlers: HashMap<String, HandlerFn>,
    ) -> Self {
        Self {
            id: Id::new(),
            transport: Arc::new(transport),
            handlers: Arc::new(Mutex::new(handlers)),
            pending_ping: Arc::new(Mutex::new(None)),
            bytes_worker: Arc::new(Mutex::new(None)),
            handler_tasks: Arc::new(Mutex::new(None)),
            response_senders: Arc::new(Mutex::new(HashMap::new())),
            error_senders: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

macro_rules! do_work_with_bytes_worker {
    ($self:expr, $work:expr) => {
        async move {
            let bytes_worker = $self.bytes_worker.lock().await;
            if let Some(bytes_worker) = bytes_worker.as_ref() {
                $work(bytes_worker).await
            } else {
                println!("not running");
                Err(BridgeRpcErrorInner::new_not_running().into())
            }
        }
    };
}

impl<TTransport: Transport> BridgeRpc<TTransport> {
    pub async fn has_handler(&self, path: impl AsRef<str>) -> bool {
        self.handlers.lock().await.contains_key(path.as_ref())
    }

    pub async fn close(&self) -> BridgeRpcResult<()> {
        do_work_with_bytes_worker!(self, async |worker: &BytesWorker| {
            send_frame_to_channel(&worker.sender, &Frame::close()).await
        })
        .await
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(rpc_id = ?self.id)))]
    pub async fn run(&self) -> BridgeRpcResult<()> {
        self.clear_handler_tasks().await?;

        let (bytes_sender, mut byte_receiver) =
            mpsc::channel(BYTES_WORKER_BUFFER_SIZE);
        let task = {
            let transport = self.transport.clone();

            trace::if_enabled! {
                let id = self.id;
            };

            tokio::spawn(async move {
                while let Some(bytes) = byte_receiver.recv().await {
                    let fut =
                        send_bytes_to_transport(transport.as_ref(), bytes);

                    trace::if_enabled! {
                        let span = trace::info_span!("run_send_task", rpc_id = ?id);
                        let fut = fut.instrument(span);
                    };

                    fut.await
                        .inspect_err(|e| {
                            trace::error!(
                                "failed to send bytes to transport: {}",
                                e
                            )
                        })
                        .ok();
                }

                trace::trace!("stream data task stopped");
            })
        };
        let abort_handle = task.abort_handle();

        let bytes_worker = BytesWorker {
            sender: bytes_sender.clone(),
            abort_handle,
        };

        let existing = self.bytes_worker.lock().await.replace(bytes_worker);

        if let Some(existing) = existing {
            drop(existing.sender);
            existing.abort_handle.abort();
        }

        trace::trace!("running rpc");

        loop {
            let bytes = self.transport.receive().await;
            let bytes = bytes.map_err(|e| {
                BridgeRpcErrorInner::new_transport(eyre::eyre!(e.to_string()))
            })?;

            let result = self.handle_receive(bytes, &bytes_sender).await?;
            if result.is_break() {
                trace::trace!("received stop signal, stopping rpc loop");
                break;
            }
        }

        let bytes_worker = self.bytes_worker.lock().await.take();

        if let Some(bytes_worker) = bytes_worker {
            drop(bytes_worker.sender);
            bytes_worker.abort_handle.abort();
        }

        self.clear_handler_tasks().await?;

        trace::trace!("stopped rpc");

        Ok(())
    }

    async fn clear_handler_tasks(&self) -> BridgeRpcResult<()> {
        let stream_handler_tasks = self.handler_tasks.lock().await.take();

        if let Some(stream_handler_tasks) = stream_handler_tasks {
            let results = stream_handler_tasks.join_all().await;

            for result in results {
                result.inspect_err(|e| {
                    trace::error!("stream handler task failed: {}", e);
                })?;
            }
        }

        Ok(())
    }

    async fn handle_receive(
        &self,
        bytes: bytes::Bytes,
        data_tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<ControlFlow<()>, BridgeRpcError> {
        let incoming: Frame = rmp_serde::from_slice(&bytes)
            .map_err(BridgeRpcErrorInner::Deserialization)?;

        trace::trace!(
            frame_type = ?incoming.discriminant(),
            "received frame from rpc"
        );

        match incoming {
            Frame::Close => self.handle_close(data_tx).await,
            Frame::Ping => self.handle_ping(data_tx).await,
            Frame::Pong => self.handle_pong(data_tx).await,
            Frame::RequestStart(request_start) => {
                self.handle_request_start(request_start, data_tx).await
            }
            Frame::RequestHeaders(request_headers) => {
                self.handle_request_headers(request_headers, data_tx).await
            }
            Frame::RequestBodyStart(request_data_start) => {
                self.handle_request_body_start(request_data_start).await
            }
            Frame::RequestBodyChunk(request_data) => {
                self.handle_request_body_chunk(request_data, data_tx).await
            }
            Frame::RequestBodyEnd(request_data_end) => {
                self.handle_request_body_end(request_data_end).await
            }
            Frame::RequestTrailers(request_trailers) => {
                self.handle_request_trailers(request_trailers, data_tx)
                    .await
            }
            Frame::RequestEnd(request_end) => {
                self.handle_request_end(request_end, data_tx).await
            }
            Frame::RequestError(request_error) => {
                self.handle_request_error(request_error, data_tx).await
            }
            Frame::ResponseStart(response_start) => {
                self.handle_response_start(response_start, data_tx).await
            }
            Frame::ResponseHeaders(response_headers) => {
                self.handle_response_headers(response_headers, data_tx)
                    .await
            }
            Frame::ResponseBodyStart(response_data_start) => {
                self.handle_response_body_start(response_data_start, data_tx)
                    .await
            }
            Frame::ResponseBodyChunk(response_data) => {
                self.handle_response_body_chunk(response_data, data_tx)
                    .await
            }
            Frame::ResponseBodyEnd(response_data_end) => {
                self.handle_response_body_end(response_data_end, data_tx)
                    .await
            }
            Frame::ResponseTrailers(response_trailers) => {
                self.handle_response_trailers(response_trailers, data_tx)
                    .await
            }
            Frame::ResponseEnd(response_end) => {
                self.handle_response_end(response_end, data_tx).await
            }
            Frame::ResponseError(response_error) => {
                self.handle_response_error(response_error, data_tx).await
            }
        }
    }

    async fn handle_close(
        &self,
        _data_tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<ControlFlow<()>, BridgeRpcError> {
        Ok(ControlFlow::Break(()))
    }

    async fn handle_ping(
        &self,
        data_tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<ControlFlow<()>, BridgeRpcError> {
        trace::debug!("received probe, sending probe ack");
        send_frame_to_channel(data_tx, &Frame::pong()).await?;
        Ok(ControlFlow::Continue(()))
    }

    async fn handle_pong(
        &self,
        _data_tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<ControlFlow<()>, BridgeRpcError> {
        trace::debug!("received probe ack");
        if let Some(rx) = self.pending_ping.lock().await.take() {
            rx.send(()).map_err(|_| {
                BridgeRpcErrorInner::new_send(eyre::eyre!(
                    "failed to send probe ack"
                ))
            })?;
        };
        Ok(ControlFlow::Continue(()))
    }

    async fn handle_request_start(
        &self,
        _request_start: RequestStart,
        _data_tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<ControlFlow<()>, BridgeRpcError> {
        Ok(ControlFlow::Continue(()))
    }

    async fn handle_request_headers(
        &self,
        _request_headers: RequestHeaders,
        _data_tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<ControlFlow<()>, BridgeRpcError> {
        Ok(ControlFlow::Continue(()))
    }

    async fn handle_request_body_start(
        &self,
        _request_data_start: RequestBodyStart,
    ) -> Result<ControlFlow<()>, BridgeRpcError> {
        Ok(ControlFlow::Continue(()))
    }

    async fn handle_request_body_chunk(
        &self,
        _request_data: RequestBodyChunk,
        _data_tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<ControlFlow<()>, BridgeRpcError> {
        Ok(ControlFlow::Continue(()))
    }

    async fn handle_request_body_end(
        &self,
        _request_data_end: RequestBodyEnd,
    ) -> Result<ControlFlow<()>, BridgeRpcError> {
        Ok(ControlFlow::Continue(()))
    }

    async fn handle_request_trailers(
        &self,
        _request_trailers: RequestTrailers,
        _data_tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<ControlFlow<()>, BridgeRpcError> {
        Ok(ControlFlow::Continue(()))
    }

    async fn handle_request_end(
        &self,
        _request_end: RequestEnd,
        _data_tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<ControlFlow<()>, BridgeRpcError> {
        Ok(ControlFlow::Continue(()))
    }

    async fn handle_request_error(
        &self,
        _request_error: RequestError,
        _data_tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<ControlFlow<()>, BridgeRpcError> {
        Ok(ControlFlow::Continue(()))
    }

    async fn handle_response_start(
        &self,
        response_start: ResponseStart,
        _data_tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<ControlFlow<()>, BridgeRpcError> {
        if let Some(response) = self
            .response_senders
            .lock()
            .await
            .get_mut(&response_start.id)
        {
            response
                .send(ChannelResponseFrame::ResponseStart(response_start))
                .await
                .map_err(|e| {
                    BridgeRpcErrorInner::new_send(eyre::Report::new(e))
                });
        }

        Ok(ControlFlow::Continue(()))
    }

    async fn handle_response_headers(
        &self,
        response_headers: ResponseHeaders,
        _data_tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<ControlFlow<()>, BridgeRpcError> {
        if let Some(response) = self
            .response_senders
            .lock()
            .await
            .get_mut(&response_headers.id)
        {
            response
                .send(ChannelResponseFrame::ResponseHeaders(response_headers))
                .await
                .map_err(|e| {
                    BridgeRpcErrorInner::new_send(eyre::Report::new(e))
                });
        }

        Ok(ControlFlow::Continue(()))
    }

    async fn handle_response_body_start(
        &self,
        response_data_start: ResponseBodyStart,
        _data_tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<ControlFlow<()>, BridgeRpcError> {
        if let Some(response) = self
            .response_senders
            .lock()
            .await
            .get_mut(&response_data_start.id)
        {
            response
                .send(ChannelResponseFrame::ResponseBodyStart(
                    response_data_start,
                ))
                .await
                .map_err(|e| {
                    BridgeRpcErrorInner::new_send(eyre::Report::new(e))
                });
        }

        Ok(ControlFlow::Continue(()))
    }

    async fn handle_response_body_chunk(
        &self,
        response_data: ResponseBodyChunk,
        _data_tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<ControlFlow<()>, BridgeRpcError> {
        if let Some(response) = self
            .response_senders
            .lock()
            .await
            .get_mut(&response_data.id)
        {
            response
                .send(ChannelResponseFrame::ResponseBodyChunk(response_data))
                .await
                .map_err(|e| {
                    BridgeRpcErrorInner::new_send(eyre::Report::new(e))
                });
        }

        Ok(ControlFlow::Continue(()))
    }

    async fn handle_response_body_end(
        &self,
        response_data_end: ResponseBodyEnd,
        _data_tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<ControlFlow<()>, BridgeRpcError> {
        if let Some(response) = self
            .response_senders
            .lock()
            .await
            .get_mut(&response_data_end.id)
        {
            response
                .send(ChannelResponseFrame::ResponseBodyEnd(response_data_end))
                .await
                .map_err(|e| {
                    BridgeRpcErrorInner::new_send(eyre::Report::new(e))
                });
        }

        Ok(ControlFlow::Continue(()))
    }

    async fn handle_response_trailers(
        &self,
        response_trailers: ResponseTrailers,
        _data_tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<ControlFlow<()>, BridgeRpcError> {
        if let Some(response) = self
            .response_senders
            .lock()
            .await
            .get_mut(&response_trailers.id)
        {
            response
                .send(ChannelResponseFrame::ResponseTrailers(response_trailers))
                .await
                .map_err(|e| {
                    BridgeRpcErrorInner::new_send(eyre::Report::new(e))
                });
        }

        Ok(ControlFlow::Continue(()))
    }

    async fn handle_response_end(
        &self,
        response_end: ResponseEnd,
        _data_tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<ControlFlow<()>, BridgeRpcError> {
        self.error_senders.lock().await.remove(&response_end.id);

        if let Some(response) =
            self.response_senders.lock().await.remove(&response_end.id)
        {
            response
                .send(ChannelResponseFrame::ResponseEnd(response_end))
                .await
                .map_err(|e| {
                    BridgeRpcErrorInner::new_send(eyre::Report::new(e))
                });
        }

        Ok(ControlFlow::Continue(()))
    }

    async fn handle_response_error(
        &self,
        response_error: ResponseError,
        _data_tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<ControlFlow<()>, BridgeRpcError> {
        self.response_senders
            .lock()
            .await
            .remove(&response_error.id);

        if let Some(response) =
            self.error_senders.lock().await.remove(&response_error.id)
        {
            response.send(response_error).map_err(|_| {
                BridgeRpcErrorInner::new_send(eyre::eyre!(
                    "failed to send response error",
                ))
            });
        }

        Ok(ControlFlow::Continue(()))
    }

    async fn clone_bytes_sender(
        &self,
    ) -> BridgeRpcResult<mpsc::Sender<Vec<u8>>> {
        let bytes_worker = self.bytes_worker.lock().await;
        if let Some(bytes_worker) = bytes_worker.as_ref() {
            Ok(bytes_worker.sender.clone())
        } else {
            Err(BridgeRpcErrorInner::new_not_running().into())
        }
    }

    pub(crate) async fn request_with_id(
        &self,
        request_id: Id,
        path: impl Into<String>,
    ) -> BridgeRpcResult<NewRequest> {
        let tx = self.clone_bytes_sender().await?;
        let (error_tx, error_rx) = oneshot::channel();

        let mut error_senders = self.error_senders.lock().await;
        error_senders.insert(request_id, error_tx);

        let (response_sender, response_receiver) =
            mpsc::channel(RESPONSE_BUFFER_SIZE);

        let mut response_senders = self.response_senders.lock().await;
        response_senders.insert(request_id, response_sender);

        Ok(NewRequest::new(
            request_id,
            path.into(),
            tx,
            error_rx,
            response_receiver,
        ))
    }

    #[inline(always)]
    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(rpc_id = ?self.id)))]
    pub async fn request(
        &self,
        path: impl Into<String>,
    ) -> BridgeRpcResult<NewRequest> {
        self.request_with_id(Id::new(), path).await
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, ret, fields(rpc_id = ?self.id)))]
    pub async fn ping(&self, timeout: Duration) -> BridgeRpcResult<bool> {
        if self.has_pending_probe().await {
            Err(BridgeRpcErrorInner::ProbeInProgress)?;
        }

        let (tx, rx) = oneshot::channel();
        self.pending_ping.lock().await.replace(tx);

        do_work_with_bytes_worker!(self, async |worker: &BytesWorker| {
            send_frame_to_channel(&worker.sender, &Frame::pong()).await
        })
        .await?;

        let result = tokio::time::timeout(timeout, rx.map(|_| true))
            .await
            .map_err(|e| {
                BridgeRpcErrorInner::new_timeout(eyre::Report::new(e))
            });

        // clear the pending ping if it exists
        _ = self.pending_ping.lock().await.take();

        Ok(result?)
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, ret, fields(rpc_id = ?self.id)))]
    pub async fn has_pending_probe(&self) -> bool {
        self.pending_ping.lock().await.is_some()
    }
}

// fn extract_value<T>(frame: &Frame) -> BridgeRpcResult<T>
// where
//     T: for<'de> serde::Deserialize<'de>,
// {
//     let value = frame
//         .data
//         .as_ref()
//         .ok_or_else(|| BridgeRpcErrorInner::new_missing_data(frame.r#type))?;
//     Ok(rmpv::ext::from_value(value.clone())?)
// }

#[cfg(test)]
mod tests {
    // use std::{future, time::Duration};

    // use super::super::BridgeRpcBuilder;
    // use super::*;
    // use crate::MockTransport;
    // use ntest::timeout;
    // use rmp_serde;
    // use serde::{Deserialize, Serialize};
    // use tokio::{task::yield_now, time::sleep};

    // #[derive(Serialize, Deserialize, Debug)]
    // struct MockRequestData {
    //     data: String,
    // }

    // #[derive(Serialize, Deserialize, Debug)]
    // struct MockResponseData {
    //     data: String,
    // }

    // fn req_data(data: impl Into<String>) -> MockRequestData {
    //     MockRequestData { data: data.into() }
    // }

    // fn res_data(data: impl Into<String>) -> MockResponseData {
    //     MockResponseData { data: data.into() }
    // }

    // fn mock_transport() -> MockTransport {
    //     MockTransport::new()
    // }

    // fn empty_rpc<TTransport: Transport>(
    //     t: TTransport,
    // ) -> BridgeRpc<TTransport> {
    //     BridgeRpc::new(t, HashMap::new(), HashMap::new())
    // }

    // #[inline(always)]
    // fn ready<V>(value: V) -> Pin<Box<dyn Future<Output = V> + Send>>
    // where
    //     V: Send + Sync + 'static,
    // {
    //     fut(future::ready(value))
    // }

    // #[inline(always)]
    // fn fut<R, F>(f: F) -> Pin<Box<dyn Future<Output = R> + Send>>
    // where
    //     F: Future<Output = R> + Send + Sync + 'static,
    // {
    //     Box::pin(f)
    // }

    // #[inline(always)]
    // fn delayed_fut<R, F>(
    //     f: F,
    //     delay: Duration,
    // ) -> Pin<Box<dyn Future<Output = R> + Send>>
    // where
    //     F: Future<Output = R> + Send + Sync + 'static,
    // {
    //     fut(async move {
    //         sleep(delay).await;
    //         f.await
    //     })
    // }

    // #[inline(always)]
    // fn delayed<V>(
    //     value: V,
    //     delay: Duration,
    // ) -> Pin<Box<dyn Future<Output = V> + Send>>
    // where
    //     V: Send + Sync + 'static,
    // {
    //     delayed_fut(future::ready(value), delay)
    // }

    // #[tokio::test]
    // async fn test_create_bridge_rpc() {
    //     let transport = mock_transport();
    //     let rpc = BridgeRpc::new(transport, HashMap::new(), HashMap::new());

    //     assert!(
    //         !rpc.has_request_handler("test_path").await,
    //         "handler should not be registered"
    //     );
    // }

    // #[tokio::test]
    // #[timeout(1000)]
    // async fn test_request() {
    //     let mut transport = mock_transport();

    //     let req_id = Id::new();

    //     transport.expect_send().returning(move |bytes| {
    //         let request: RequestFrame<MockRequestData> =
    //             rmp_serde::from_slice(&bytes)
    //                 .expect("Failed to deserialize request");

    //         assert_eq!(
    //             request.r#type,
    //             FrameType::MessageRequest,
    //             "Expected request frame"
    //         );

    //         assert!(request.data.is_some(), "Expected request data");

    //         if let Some(request) = &request.data {
    //             assert_eq!(request.path, "test_path");
    //             assert_eq!(request.id, req_id);
    //         }

    //         Box::pin(async move { Ok(()) })
    //     });

    //     let mut sent_success = false;
    //     let mut sent_start_ack = false;
    //     transport.expect_receive().returning(move || {
    //         if !sent_start_ack {
    //             sent_start_ack = true;
    //             return ready(Ok(serialize(&Frame::probe_ack())
    //                 .expect("Failed to serialize start ack frame")
    //                 .into()));
    //         }

    //         if !sent_success {
    //             sent_success = true;

    //             return ready(Ok(serialize(&Frame::success_response_start(
    //                 req_id,
    //                 res_data("test_data"),
    //             ))
    //             .expect("Failed to serialize response")
    //             .into()));
    //         }

    //         ready(Ok(serialize(&Frame::close())
    //             .expect("Failed to serialize close frame")
    //             .into()))
    //     });

    //     let rpc = empty_rpc(transport);

    //     let response = {
    //         let rpc = rpc.clone();
    //         tokio::spawn(async move {
    //             let response = rpc
    //                 .request_with_id::<MockResponseData, _>(
    //                     req_id,
    //                     "test_path",
    //                     req_data("test_data"),
    //                 )
    //                 .await
    //                 .expect("Request failed");

    //             assert_eq!(response.data, "test_data");
    //         })
    //     };

    //     // run the RPC to populate response buffer
    //     let run = {
    //         let rpc = rpc.clone();
    //         tokio::spawn(async move {
    //             sleep(Duration::from_millis(100)).await;
    //             rpc.run().await.expect("Failed to run RPC");
    //         })
    //     };

    //     _ = tokio::join!(response, run);
    // }

    // #[tokio::test]
    // #[timeout(1000)]
    // async fn test_close() {
    //     let mut transport = mock_transport();

    //     transport.expect_send().returning(move |bytes| {
    //         let frame: Frame<()> = rmp_serde::from_slice(&bytes)
    //             .expect("Failed to deserialize frame");

    //         assert_eq!(frame.r#type, FrameType::Close, "Expected close frame");

    //         ready(Ok(()))
    //     });

    //     let mut sent_close_ack = false;
    //     transport.expect_receive().returning(move || {
    //         std::thread::sleep(Duration::from_millis(100));
    //         if !sent_close_ack {
    //             sent_close_ack = true;
    //             return delayed(
    //                 Ok(serialize(&Frame::close_ack())
    //                     .expect("Failed to serialize close frame")
    //                     .into()),
    //                 Duration::from_millis(10),
    //             );
    //         }

    //         delayed(
    //             Ok(serialize(&Frame::close())
    //                 .expect("Failed to serialize close frame")
    //                 .into()),
    //             Duration::from_millis(10),
    //         )
    //     });

    //     let rpc = empty_rpc(transport);

    //     let run = {
    //         let rpc = rpc.clone();
    //         tokio::spawn(async move {
    //             rpc.run().await.expect("Failed to run RPC");
    //         })
    //     };

    //     let close = async {
    //         yield_now().await;
    //         rpc.close().await
    //     };

    //     let (_, response) = tokio::join!(run, close);

    //     response.expect("Close should return a valid result");
    // }

    // #[tokio::test]
    // #[timeout(1000)]
    // async fn test_probe() {
    //     let mut transport = mock_transport();

    //     let mut received_probe = false;
    //     transport.expect_send().returning(move |bytes| {
    //         if !received_probe {
    //             received_probe = true;
    //             let frame = rmp_serde::from_slice::<Frame<()>>(&bytes)
    //                 .expect("Failed to deserialize frame");

    //             assert_eq!(
    //                 frame.r#type,
    //                 FrameType::Ping,
    //                 "Expected probe frame"
    //             );
    //         }

    //         ready(Ok(()))
    //     });

    //     let mut sent_probe_ack = false;
    //     transport.expect_receive().returning(move || {
    //         if !sent_probe_ack {
    //             sent_probe_ack = true;
    //             return delayed(
    //                 Ok(serialize(&Frame::probe_ack())
    //                     .expect("Failed to serialize probe ack frame")
    //                     .into()),
    //                 Duration::from_millis(10),
    //             );
    //         }

    //         delayed(
    //             Ok(serialize(&Frame::close())
    //                 .expect("Failed to serialize close frame")
    //                 .into()),
    //             Duration::from_millis(10),
    //         )
    //     });

    //     let rpc = BridgeRpcBuilder::new(transport)
    //         .build()
    //         .expect("should be able to build");

    //     let run = {
    //         let rpc = rpc.clone();
    //         tokio::spawn(async move {
    //             rpc.run().await.expect("Failed to run RPC");
    //         })
    //     };

    //     let response = async {
    //         yield_now().await;
    //         rpc.probe(Duration::from_millis(100)).await
    //     };

    //     let (_, response) = tokio::join!(run, response);

    //     response.expect("Probe should return a valid result");
    //     assert!(!rpc.has_pending_probe().await);
    // }

    // #[tokio::test]
    // #[timeout(1000)]
    // async fn test_respond_existing_path() {
    //     let mut transport = mock_transport();

    //     let req_id = Id::new();

    //     let mut received_response = false;
    //     transport.expect_send().returning(move |bytes| {
    //         if received_response {
    //             return ready(Ok(()));
    //         }

    //         let response: ResponseFrame<MockResponseData> =
    //             rmp_serde::from_slice(&bytes)
    //                 .expect("Failed to deserialize response");

    //         assert_eq!(
    //             response.r#type,
    //             FrameType::MessageResponse,
    //             "Expected response frame"
    //         );

    //         assert!(response.data.is_some(), "Expected response data");

    //         received_response = true;

    //         if let Some(response) = &response.data {
    //             assert_eq!(response.id, req_id);
    //             assert_eq!(
    //                 response.data.as_ref().expect("Should have data").data,
    //                 "test_data"
    //             );
    //         }

    //         ready(Ok(()))
    //     });

    //     let mut sent_success = false;
    //     transport.expect_receive().returning(move || {
    //         if sent_success {
    //             return ready(Ok(serialize(&Frame::close())
    //                 .expect("Failed to serialize close frame")
    //                 .into()));
    //         }

    //         sent_success = true;

    //         ready(Ok(serialize(&Frame::request(
    //             req_id,
    //             "test_path",
    //             req_data("test_data"),
    //         ))
    //         .expect("Failed to serialize response")
    //         .into()))
    //     });

    //     let rpc = BridgeRpcBuilder::new(transport)
    //         .request_handler(
    //             "test_path",
    //             async |req: RequestContext<MockRequestData>| {
    //                 Ok::<_, String>(MockResponseData {
    //                     data: req.data.data,
    //                 })
    //             },
    //         )
    //         .build()
    //         .expect("should be able to build");

    //     // run the RPC to populate response buffer
    //     rpc.run().await.expect("Failed to run RPC");
    // }

    // #[tokio::test]
    // #[timeout(1000)]
    // async fn test_respond_non_existing_path() {
    //     let mut transport = mock_transport();

    //     let req_id = Id::new();

    //     let mut received_response = false;
    //     transport.expect_send().returning(move |bytes| {
    //         if received_response {
    //             return ready(Ok(()));
    //         }

    //         let response: ResponseFrame<MockResponseData> =
    //             rmp_serde::from_slice(&bytes)
    //                 .expect("Failed to deserialize response");

    //         assert_eq!(
    //             response.r#type,
    //             FrameType::MessageResponse,
    //             "Expected response frame"
    //         );

    //         assert!(response.data.is_some(), "Expected response data");

    //         received_response = true;
    //         if let Some(response) = &response.data {
    //             assert_eq!(response.id, req_id);
    //             assert_eq!(
    //                 response.error.as_ref().expect("Should have error").message,
    //                 "no handler found for path: 'test_path_wrong'"
    //             );
    //         }

    //         ready(Ok(()))
    //     });

    //     let mut sent_success = false;
    //     transport.expect_receive().returning(move || {
    //         if sent_success {
    //             return ready(Ok(serialize(&Frame::close())
    //                 .expect("Failed to serialize close frame")
    //                 .into()));
    //         }

    //         sent_success = true;

    //         ready(Ok(serialize(&Frame::request(
    //             req_id,
    //             "test_path_wrong",
    //             req_data("test_data"),
    //         ))
    //         .expect("Failed to serialize response")
    //         .into()))
    //     });

    //     let rpc = empty_rpc(transport);

    //     // run the RPC to populate response buffer
    //     rpc.run().await.expect("Failed to run RPC");
    // }

    // #[tokio::test]
    // #[timeout(1000)]
    // async fn test_respond_stream_start_existing_path() {
    //     let mut transport = mock_transport();

    //     let mut received_response = false;
    //     transport.expect_send().returning(move |bytes| {
    //         if received_response {
    //             return ready(Ok(()));
    //         }

    //         let response: StreamStartResponseFrame =
    //             rmp_serde::from_slice(&bytes)
    //                 .expect("Failed to deserialize response");

    //         assert_eq!(
    //             response.r#type,
    //             FrameType::StreamStartResponse,
    //             "Expected response frame"
    //         );

    //         assert!(response.data.is_some(), "Expected response data");

    //         received_response = true;

    //         if let Some(response) = &response.data {
    //             assert!(response.ok, "Expected success response");
    //         }

    //         ready(Ok(()))
    //     });

    //     let mut sent_success = false;
    //     transport.expect_receive().returning(move || {
    //         if sent_success {
    //             return ready(Ok(serialize(&Frame::close())
    //                 .expect("Failed to serialize close frame")
    //                 .into()));
    //         }

    //         sent_success = true;

    //         ready(Ok(serialize(&StreamStartFrame::<()>::stream_start(
    //             Id::new(),
    //             "test_path",
    //             None,
    //         ))
    //         .expect("Failed to serialize response")
    //         .into()))
    //     });

    //     let rpc = BridgeRpcBuilder::new(transport)
    //         .stream_handler("test_path", |_: StreamContext| async move {
    //             Ok::<_, String>(())
    //         })
    //         .build()
    //         .expect("should be able to build");

    //     // run the RPC to populate response buffer
    //     rpc.run().await.expect("Failed to run RPC");
    // }

    // #[tokio::test]
    // #[timeout(1000)]
    // async fn test_respond_stream_start_non_existing_path() {
    //     let mut transport = mock_transport();

    //     let mut received_response = false;
    //     transport.expect_send().returning(move |bytes| {
    //         if received_response {
    //             return ready(Ok(()));
    //         }

    //         let response: StreamStartResponseFrame =
    //             rmp_serde::from_slice(&bytes)
    //                 .expect("Failed to deserialize response");

    //         assert_eq!(
    //             response.r#type,
    //             FrameType::StreamStartResponse,
    //             "Expected response frame"
    //         );

    //         assert!(response.data.is_some(), "Expected response data");

    //         received_response = true;

    //         if let Some(response) = &response.data {
    //             assert!(!response.ok, "Expected error response");
    //             assert_eq!(
    //                 response.error.as_ref().expect("Should have error").message,
    //                 "no handler found for path: 'test_path_wrong'"
    //             );
    //         }

    //         ready(Ok(()))
    //     });

    //     let mut sent_success = false;
    //     transport.expect_receive().returning(move || {
    //         if sent_success {
    //             return ready(Ok(serialize(&Frame::close())
    //                 .expect("Failed to serialize close frame")
    //                 .into()));
    //         }

    //         sent_success = true;

    //         ready(Ok(serialize(&StreamStartFrame::<()>::stream_start(
    //             Id::new(),
    //             "test_path_wrong",
    //             None,
    //         ))
    //         .expect("Failed to serialize response")
    //         .into()))
    //     });

    //     let rpc = empty_rpc(transport);

    //     // run the RPC to populate response buffer
    //     rpc.run().await.expect("Failed to run RPC");
    // }
}
