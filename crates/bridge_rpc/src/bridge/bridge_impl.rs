use futures::FutureExt as _;
use strum::IntoDiscriminant;
use tokio::sync::mpsc;
use tokio::sync::{
    Mutex,
    oneshot::{self},
};
use tokio::task::JoinSet;
use tokio_stream::Stream as TokioStream;
use tracing::Instrument;

use std::ops::ControlFlow;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use super::super::Id;
use super::BridgeRpcError;
use super::BridgeRpcErrorInner;
use super::BridgeRpcResult;
use super::ResponseStatusCode;
use super::bytes_worker::BytesWorker;
use super::client::request::*;
use super::client::response::ResponseFrameEvent;
use super::client_handle::ClientHandle;
use super::constants::{BYTES_WORKER_BUFFER_SIZE, RESPONSE_BUFFER_SIZE};
use super::contexts::*;
use super::frame::*;
use super::server;
use super::server::request::RequestFrameEvent;
use super::service::{
    Service, ServiceContext,
    error::{ServiceError, ServiceResult},
};
use super::session::*;
use super::utils::send_bytes_to_transport;
use super::utils::send_frame_to_channel;
use super::{Headers, Trailers};
use crate::Transport;

pub type BoxStream<'a, T> = Pin<Box<dyn TokioStream<Item = T> + Send + 'a>>;

pub struct BridgeRpc<TTransport: Transport, TService: Service> {
    id: Id,
    service: Arc<TService>,
    transport: Arc<TTransport>,
    pending_ping: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    bytes_worker: Arc<Mutex<Option<BytesWorker>>>,
    service_tasks: Arc<Mutex<Option<JoinSet<Result<(), ServiceError>>>>>,
    session_manager:
        SessionManager<RequestSessionContext, ResponseSessionContext>,
}

impl<TTransport: Transport, TService: Service> Clone
    for BridgeRpc<TTransport, TService>
{
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            transport: self.transport.clone(),
            pending_ping: self.pending_ping.clone(),
            service: self.service.clone(),
            bytes_worker: self.bytes_worker.clone(),
            service_tasks: self.service_tasks.clone(),
            session_manager: self.session_manager.clone(),
        }
    }
}

impl<TTransport: Transport, TService: Service> BridgeRpc<TTransport, TService> {
    pub fn id(&self) -> Id {
        self.id
    }
}

impl<TTransport: Transport, TService: Service> BridgeRpc<TTransport, TService> {
    pub fn new(transport: TTransport, service: TService) -> Self {
        Self {
            id: Id::new(),
            transport: Arc::new(transport),
            service: Arc::new(service),
            pending_ping: Arc::new(Mutex::new(None)),
            bytes_worker: Arc::new(Mutex::new(None)),
            service_tasks: Arc::new(Mutex::new(None)),
            session_manager: SessionManager::new(),
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

impl<TTransport: Transport, TService: Service> BridgeRpc<TTransport, TService> {
    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(rpc_id = ?self.id, request_id = ?request_id, path = path.as_ref())))]
    pub(crate) async fn request_with_id(
        &self,
        request_id: Id,
        path: impl AsRef<str>,
    ) -> BridgeRpcResult<PendingRequest> {
        let tx = self.clone_bytes_sender().await?;
        Ok(super::request_utils::create_request(
            request_id,
            path,
            tx,
            &self.session_manager,
        )
        .await?)
    }

    #[inline(always)]
    pub async fn request(
        &self,
        path: impl AsRef<str>,
    ) -> BridgeRpcResult<PendingRequest> {
        self.request_with_id(Id::new(), path).await
    }

    #[inline(always)]
    pub async fn create_client_handle(&self) -> BridgeRpcResult<ClientHandle> {
        Ok(ClientHandle::new(
            self.id,
            self.session_manager.clone(),
            self.bytes_worker.clone(),
        ))
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, ret, fields(rpc_id = ?self.id)))]
    pub async fn ping(&self, timeout: Duration) -> BridgeRpcResult<bool> {
        if self.has_pending_probe().await {
            Err(BridgeRpcErrorInner::ProbeInProgress)?;
        }

        let (tx, rx) = oneshot::channel();
        self.pending_ping.lock().await.replace(tx);

        do_work_with_bytes_worker!(self, async |worker: &BytesWorker| {
            send_frame_to_channel(&worker.sender, &Frame::ping()).await
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

            trace::trace!("closing existing bytes worker");
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

    async fn get_response_session(
        &self,
        id: Id,
    ) -> Option<Concurrent<ResponseSession<ResponseSessionContext>>> {
        self.session_manager.get_response_session(id).await
    }

    async fn close_response_session(&self, id: Id) {
        self.session_manager.close_response_session(id).await
    }

    async fn start_request_session(
        &self,
        id: Id,
    ) -> Result<
        (
            Concurrent<RequestSession<RequestSessionContext>>,
            mpsc::Receiver<RequestFrameEvent>,
            oneshot::Receiver<RequestError>,
        ),
        SessionManagerError,
    > {
        let (request_sender, request_receiver) =
            mpsc::channel(RESPONSE_BUFFER_SIZE);
        let (request_error_sender, request_error_receiver) = oneshot::channel();

        let request_context =
            RequestSessionContext::new(request_sender, request_error_sender);
        let session = self
            .session_manager
            .start_request_session(id, request_context)
            .await?;

        Ok((session, request_receiver, request_error_receiver))
    }

    async fn get_request_session(
        &self,
        id: Id,
    ) -> Option<Concurrent<RequestSession<RequestSessionContext>>> {
        self.session_manager.get_request_session(id).await
    }

    async fn close_request_session(&self, id: Id) {
        self.session_manager.close_request_session(id).await
    }

    async fn clear_handler_tasks(&self) -> BridgeRpcResult<()> {
        let stream_handler_tasks = self.service_tasks.lock().await.take();

        if let Some(stream_handler_tasks) = stream_handler_tasks {
            let results = stream_handler_tasks.join_all().await;

            for result in results {
                result.inspect_err(|e| {
                    trace::error!("stream handler task failed: {}", e);
                })?;
            }
        }

        _ = self.service_tasks.lock().await.insert(Default::default());

        trace::trace!("cleared service tasks");

        Ok(())
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(rpc_id = ?self.id, bytes_length = bytes.len())))]
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

        enum Event {
            Request(RequestEvent),
            Response(ResponseEvent),
        }

        let ev = match incoming {
            Frame::Close => {
                self.handle_close(data_tx).await?;

                return Ok(ControlFlow::Break(()));
            }
            Frame::Ping => {
                self.handle_ping(data_tx).await?;

                return Ok(ControlFlow::Continue(()));
            }
            Frame::Pong => {
                self.handle_pong(data_tx).await?;

                return Ok(ControlFlow::Continue(()));
            }
            Frame::RequestStart(request_start) => {
                Event::Request(RequestEvent::Start(request_start))
            }
            Frame::RequestBodyChunk(request_data) => {
                Event::Request(RequestEvent::BodyChunk(request_data))
            }
            Frame::RequestEnd(request_end) => {
                Event::Request(RequestEvent::End(request_end))
            }
            Frame::RequestError(request_error) => {
                Event::Request(RequestEvent::Error(request_error))
            }
            Frame::ResponseStart(response_start) => {
                Event::Response(ResponseEvent::Start(response_start))
            }
            Frame::ResponseBodyChunk(response_data) => {
                Event::Response(ResponseEvent::BodyChunk(response_data))
            }
            Frame::ResponseEnd(response_end) => {
                Event::Response(ResponseEvent::End(response_end))
            }
            Frame::ResponseError(response_error) => {
                Event::Response(ResponseEvent::Error(response_error))
            }
        };

        match ev {
            Event::Request(request_event) => {
                let (session, request_receiver, request_error_receiver) =
                    if let RequestEvent::Start(ref request_start) =
                        request_event
                    {
                        let (session, request_receiver, request_error_receiver) =
                            self.start_request_session(request_start.id).await?;

                        (
                            session,
                            Some(request_receiver),
                            Some(request_error_receiver),
                        )
                    } else {
                        (
                            self.get_request_session(request_event.id())
                                .await
                                .ok_or_else(|| {
                                    BridgeRpcErrorInner::Unknown(eyre::eyre!(
                                        "no request session found for id: {}",
                                        request_event.id()
                                    ))
                                })?,
                            None,
                            None,
                        )
                    };

                let id = session.lock().await.id();

                let should_close_session =
                    request_event.is_error() || request_event.is_end();

                let output = session
                    .lock()
                    .await
                    .state_mut()
                    .transition(request_event)?;

                match output {
                    RequestStateTransitionOutput::Wait => {
                        // do nothing
                    }
                    RequestStateTransitionOutput::Start {
                        id,
                        path,
                        headers,
                    } => {
                        self.handle_request_start(
                            id,
                            path,
                            headers,
                            request_receiver.expect("should be set"),
                            request_error_receiver.expect("should be set"),
                        )
                        .await?;
                    }
                    RequestStateTransitionOutput::BodyChunk { chunk } => {
                        self.handle_request_body_chunk(session, chunk).await?;
                    }
                    RequestStateTransitionOutput::End { trailers } => {
                        self.handle_request_end(session, trailers).await?;
                    }
                    RequestStateTransitionOutput::Error { error } => {
                        self.handle_request_error(session, error).await?;
                    }
                }

                if should_close_session {
                    self.close_request_session(id).await;
                }

                Ok(ControlFlow::Continue(()))
            }
            Event::Response(response_event) => {
                let id = response_event.id();
                let session =
                    self.get_response_session(id).await.ok_or_else(|| {
                        BridgeRpcErrorInner::Unknown(eyre::eyre!(
                            "no response session found for id: {}",
                            id
                        ))
                    })?;

                let should_close_session =
                    response_event.is_error() || response_event.is_end();
                let output = session
                    .lock()
                    .await
                    .state_mut()
                    .transition(response_event)?;

                match output {
                    ResponseStateTransitionOutput::Wait => {
                        // do nothing
                    }
                    ResponseStateTransitionOutput::Start {
                        id,
                        status,
                        headers,
                    } => {
                        self.handle_response_start(
                            session, id, status, headers,
                        )
                        .await?;
                    }
                    ResponseStateTransitionOutput::BodyChunk { chunk } => {
                        self.handle_response_body_chunk(session, chunk).await?;
                    }
                    ResponseStateTransitionOutput::End { trailers } => {
                        self.handle_response_end(session, trailers).await?;
                    }
                    ResponseStateTransitionOutput::Error { error } => {
                        self.handle_response_error(session, error).await?;
                    }
                }

                if should_close_session {
                    self.close_response_session(id).await;
                }

                Ok(ControlFlow::Continue(()))
            }
        }
    }

    async fn handle_close(
        &self,
        _data_tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<(), BridgeRpcError> {
        trace::trace!("received close frame, stopping rpc");
        Ok(())
    }

    async fn handle_ping(
        &self,
        data_tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<(), BridgeRpcError> {
        trace::debug!("received probe, sending probe ack");
        send_frame_to_channel(data_tx, &Frame::pong()).await?;
        Ok(())
    }

    async fn handle_pong(
        &self,
        _data_tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<(), BridgeRpcError> {
        trace::debug!("received probe ack");
        if let Some(rx) = self.pending_ping.lock().await.take() {
            rx.send(()).map_err(|_| {
                BridgeRpcErrorInner::new_send(eyre::eyre!(
                    "failed to send probe ack"
                ))
            })?;
        };
        Ok(())
    }

    async fn handle_request_start(
        &self,
        id: Id,
        path: String,
        headers: Option<Headers>,
        request_frame_receiver: RequestFrameReceiver,
        request_error_receiver: RequestErrorReceiver,
    ) -> Result<(), BridgeRpcError> {
        trace::trace!("received request start, starting request session");
        let service = self.service.clone();

        let response_bytes_tx = self.clone_bytes_sender().await?;

        let request = server::request::Request::new(
            id,
            path,
            headers,
            request_frame_receiver,
            request_error_receiver,
        );

        let response =
            server::response::PendingResponse::new(id, response_bytes_tx);

        self.spawn_service_task(async move {
            trace::trace!("running service");
            let context = ServiceContext::new(request, response);

            let result = service.run(context).await;

            if let Err(error) = result {
                trace::error!("service error: {}", error);
                Err(error)?
            } else {
                trace::trace!("service finished");
                Ok(())
            }
        })
        .await;

        Ok(())
    }

    async fn spawn_service_task<F>(&self, future: F)
    where
        F: Future<Output = ServiceResult<()>> + Send + 'static,
    {
        trace::trace!("spawning service task");
        let mut tasks = self.service_tasks.lock().await;
        let tasks = tasks.get_or_insert_default();
        tasks.spawn(future);
    }

    async fn handle_request_body_chunk(
        &self,
        session: Arc<Mutex<RequestSession<RequestSessionContext>>>,
        chunk: Vec<u8>,
    ) -> Result<(), BridgeRpcError> {
        trace::trace!(
            "received request body chunk, sending request body chunk"
        );
        session
            .lock()
            .await
            .context_mut()
            .request_frame_sender
            .send(RequestFrameEvent::new_body_chunk(chunk))
            .await
            .map_err(|e| BridgeRpcErrorInner::new_send(eyre::Report::new(e)))?;

        Ok(())
    }

    async fn handle_request_end(
        &self,
        session: Arc<Mutex<RequestSession<RequestSessionContext>>>,
        trailers: Option<Trailers>,
    ) -> Result<(), BridgeRpcError> {
        trace::trace!("received request end, sending request end");
        session
            .lock()
            .await
            .context_mut()
            .request_frame_sender
            .send(RequestFrameEvent::new_end(trailers))
            .await
            .map_err(|e| BridgeRpcErrorInner::new_send(eyre::Report::new(e)))?;

        Ok(())
    }

    async fn handle_request_error(
        &self,
        session: Arc<Mutex<RequestSession<RequestSessionContext>>>,
        error: RequestError,
    ) -> Result<(), BridgeRpcError> {
        trace::trace!("received request error, sending request error");
        let sender = session
            .lock()
            .await
            .context_mut()
            .request_error_sender
            .lock()
            .await
            .take();
        if let Some(sender) = sender {
            sender.send(error).map_err(|_| {
                BridgeRpcErrorInner::new_send(eyre::eyre!(
                    "failed to send request error",
                ))
            })?;
        }

        Ok(())
    }

    async fn handle_response_start(
        &self,
        response_session: Arc<Mutex<ResponseSession<ResponseSessionContext>>>,
        id: Id,
        status: ResponseStatusCode,
        headers: Option<Headers>,
    ) -> Result<(), BridgeRpcError> {
        trace::trace!("received response start, sending response start");
        let start = response_session
            .lock()
            .await
            .context_mut()
            .response_start_sender
            .lock()
            .await
            .take();

        let response_start = ResponseStart::new(id, status, headers);

        if let Some(start) = start {
            start.send(response_start).map_err(|e| {
                BridgeRpcErrorInner::new_send(eyre::eyre!(
                    "can't send response start (id: {})",
                    e.id
                ))
            })?;
        }

        Ok(())
    }

    async fn handle_response_body_chunk(
        &self,
        response_session: Arc<Mutex<ResponseSession<ResponseSessionContext>>>,
        chunk: Vec<u8>,
    ) -> Result<(), BridgeRpcError> {
        trace::trace!(
            "received response body chunk, sending response body chunk"
        );
        response_session
            .lock()
            .await
            .context_mut()
            .response_frame_sender
            .send(ResponseFrameEvent::new_body_chunk(chunk))
            .await
            .map_err(|e| BridgeRpcErrorInner::new_send(eyre::Report::new(e)))?;

        Ok(())
    }

    async fn handle_response_end(
        &self,
        session: Arc<Mutex<ResponseSession<ResponseSessionContext>>>,
        trailers: Option<Trailers>,
    ) -> Result<(), BridgeRpcError> {
        trace::trace!("received response end, sending response end");
        session
            .lock()
            .await
            .context_mut()
            .response_frame_sender
            .send(ResponseFrameEvent::new_end(trailers))
            .await
            .map_err(|e| BridgeRpcErrorInner::new_send(eyre::Report::new(e)))?;

        Ok(())
    }

    async fn handle_response_error(
        &self,
        session: Arc<Mutex<ResponseSession<ResponseSessionContext>>>,
        response_error: ResponseError,
    ) -> Result<(), BridgeRpcError> {
        trace::trace!("received response error, sending response error");
        let error = session
            .lock()
            .await
            .context_mut()
            .response_error_sender
            .lock()
            .await
            .take();
        if let Some(error) = error {
            error.send(response_error).map_err(|_| {
                BridgeRpcErrorInner::new_send(eyre::eyre!(
                    "failed to send response error",
                ))
            })?;
        }

        Ok(())
    }

    async fn clone_bytes_sender(
        &self,
    ) -> BridgeRpcResult<mpsc::Sender<Vec<u8>>> {
        trace::trace!("cloning bytes sender");
        let bytes_worker = self.bytes_worker.lock().await;
        if let Some(bytes_worker) = bytes_worker.as_ref() {
            Ok(bytes_worker.sender.clone())
        } else {
            Err(BridgeRpcErrorInner::new_not_running().into())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{future, time::Duration};

    use super::super::frame::Frame;
    use super::super::frame_utils::{
        single_data_request_frames, single_data_response_frames,
    };
    use super::super::service::MockService;
    use super::super::utils::serialize;
    use super::*;
    use crate::MockTransport;
    use ntest::timeout;
    use rmp_serde;
    use tokio::{task::yield_now, time::sleep};

    const TEST_DATA: &str = "test_data";
    const TEST_PATH: &str = "test_path";

    fn mock_transport() -> MockTransport {
        MockTransport::new()
    }

    fn mock_service() -> MockService {
        MockService::new()
    }

    fn empty_rpc<TTransport: Transport>(
        t: TTransport,
    ) -> BridgeRpc<TTransport, MockService> {
        rpc_with_service(t, mock_service())
    }

    fn rpc_with_service<TTransport: Transport, TService: Service>(
        t: TTransport,
        service: TService,
    ) -> BridgeRpc<TTransport, TService> {
        BridgeRpc::new(t, service)
    }

    #[inline(always)]
    fn ready<V>(value: V) -> Pin<Box<dyn Future<Output = V> + Send>>
    where
        V: Send + Sync + 'static,
    {
        fut(future::ready(value))
    }

    #[inline(always)]
    fn fut<R, F>(f: F) -> Pin<Box<dyn Future<Output = R> + Send>>
    where
        F: Future<Output = R> + Send + Sync + 'static,
    {
        Box::pin(f)
    }

    #[inline(always)]
    fn delayed_fut<R, F>(
        f: F,
        delay: Duration,
    ) -> Pin<Box<dyn Future<Output = R> + Send>>
    where
        F: Future<Output = R> + Send + Sync + 'static,
    {
        fut(async move {
            sleep(delay).await;
            f.await
        })
    }

    #[inline(always)]
    fn delayed<V>(
        value: V,
        delay: Duration,
    ) -> Pin<Box<dyn Future<Output = V> + Send>>
    where
        V: Send + Sync + 'static,
    {
        delayed_fut(future::ready(value), delay)
    }

    #[tokio::test]
    #[test_log::test]
    #[timeout(1000)]
    async fn test_close() {
        let mut transport = mock_transport();

        transport.expect_send().returning(move |bytes| {
            let frame: Frame = rmp_serde::from_slice(&bytes)
                .expect("Failed to deserialize frame");

            assert_eq!(
                frame.discriminant(),
                FrameType::Close,
                "Expected close frame"
            );

            ready(Ok(()))
        });

        let rpc = empty_rpc(transport);

        let run = {
            let rpc = rpc.clone();
            tokio::spawn(async move {
                rpc.run().await.expect("Failed to run RPC");
            })
        };

        let close = async {
            yield_now().await;
            rpc.close().await
        };

        let (_, response) = tokio::join!(run, close);

        response.expect("Close should return a valid result");
    }

    #[tokio::test]
    #[test_log::test]
    #[timeout(1000)]
    async fn test_probe() {
        let mut transport = mock_transport();

        let mut received_probe = false;
        transport.expect_send().returning(move |bytes| {
            if !received_probe {
                received_probe = true;
                let frame = rmp_serde::from_slice::<Frame>(&bytes)
                    .expect("Failed to deserialize frame");

                assert_eq!(
                    frame.discriminant(),
                    FrameType::Ping,
                    "Expected probe frame"
                );
            }

            ready(Ok(()))
        });

        let mut sent_probe_ack = false;
        transport.expect_receive().returning(move || {
            if !sent_probe_ack {
                sent_probe_ack = true;
                return delayed(
                    Ok(serialize(&Frame::pong())
                        .expect("Failed to serialize probe ack frame")
                        .into()),
                    Duration::from_millis(10),
                );
            }

            delayed(
                Ok(serialize(&Frame::close())
                    .expect("Failed to serialize close frame")
                    .into()),
                Duration::from_millis(10),
            )
        });

        let rpc = empty_rpc(transport);

        let run = {
            let rpc = rpc.clone();
            tokio::spawn(async move {
                rpc.run().await.expect("Failed to run RPC");
            })
        };

        let response = async {
            yield_now().await;
            rpc.ping(Duration::from_millis(100)).await
        };

        let (_, response) = tokio::join!(run, response);

        response.expect("Probe should return a valid result");
        assert!(!rpc.has_pending_probe().await);
    }

    #[tokio::test]
    #[test_log::test]
    #[timeout(1000)]
    async fn test_request() {
        let mut transport = mock_transport();
        let test_data_bytes = serialize(&TEST_DATA).unwrap();

        let req_id = Id::new();

        let expected_request_frames =
            single_data_request_frames(req_id, TEST_PATH, &TEST_DATA).unwrap();

        let mut request_frame_index = 0;

        transport.expect_send().returning(move |bytes| {
            let request: Frame = rmp_serde::from_slice(&bytes)
                .expect("Failed to deserialize request");

            if request_frame_index < expected_request_frames.len() {
                let frame =
                    expected_request_frames.get(request_frame_index).unwrap();

                assert_eq!(
                    request.discriminant(),
                    frame.discriminant(),
                    "Expected request frame type"
                );

                assert_eq!(frame, &request, "Expected request frame");

                request_frame_index += 1;
            }

            fut(async move { Ok(()) })
        });

        let expected_response_frames =
            single_data_response_frames(req_id, &TEST_DATA).unwrap();
        let mut response_frame_index = 0;
        transport.expect_receive().returning(move || {
            if response_frame_index < expected_response_frames.len() {
                let frame =
                    expected_response_frames.get(response_frame_index).unwrap();

                response_frame_index += 1;

                ready(Ok(serialize(&frame)
                    .expect("Failed to serialize response")
                    .into()))
            } else {
                ready(Ok(serialize(&Frame::close())
                    .expect("Failed to serialize close frame")
                    .into()))
            }
        });

        let rpc = empty_rpc(transport);

        let response = {
            let rpc = rpc.clone();
            tokio::spawn(async move {
                let mut request = rpc
                    .request_with_id(req_id, TEST_PATH.to_string())
                    .await
                    .expect("Request failed")
                    .start()
                    .await
                    .expect("Failed to start response");

                request
                    .write_body_chunk(test_data_bytes.clone())
                    .await
                    .expect("Failed to write body chunk");

                let response = request
                    .end()
                    .await
                    .expect("Failed to end response")
                    .wait()
                    .await
                    .expect("Failed to wait for response");

                let (status, headers, mut reader) = response.into_parts();

                let mut data = vec![];

                while let Some(chunk) = reader
                    .read_body_chunk()
                    .await
                    .expect("Failed to read body chunk")
                {
                    data.extend_from_slice(&chunk);
                }

                let trailers =
                    reader.trailers().expect("Failed to read trailers");

                assert_eq!(status, ResponseStatusCode::SUCCESS);
                assert!(headers.is_none());
                assert!(trailers.is_none());
                assert_eq!(data, test_data_bytes);
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
    #[test_log::test]
    #[timeout(1000)]
    async fn test_response() {
        let mut transport = mock_transport();
        let req_id = Id::new();
        let test_data_bytes = serialize(&TEST_DATA).unwrap();

        let expected_response_frames =
            single_data_response_frames(req_id, &TEST_DATA).unwrap();
        let mut response_frame_index = 0;
        transport.expect_send().returning(move |bytes| {
            let response: Frame = rmp_serde::from_slice(&bytes)
                .expect("Failed to deserialize request");

            if response_frame_index < expected_response_frames.len() {
                let frame =
                    expected_response_frames.get(response_frame_index).unwrap();

                assert_eq!(
                    response.discriminant(),
                    frame.discriminant(),
                    "Expected response frame type"
                );

                assert_eq!(frame, &response, "Expected response frame");

                response_frame_index += 1;
            }

            fut(async move { Ok(()) })
        });

        let expected_request_frames =
            single_data_request_frames(req_id, "test_path", &TEST_DATA)
                .unwrap();
        let mut request_frame_index = 0;
        let response_sent = Arc::new(Mutex::new(false));

        {
            let response_sent = response_sent.clone();
            transport.expect_receive().returning(move || {
                if request_frame_index < expected_request_frames.len() {
                    let frame = expected_request_frames
                        .get(request_frame_index)
                        .unwrap();

                    request_frame_index += 1;

                    ready(Ok(serialize(&frame)
                        .expect("Failed to serialize response")
                        .into()))
                } else {
                    let response_sent = response_sent.clone();
                    fut(async move {
                        while !*response_sent.lock().await {
                            yield_now().await;
                        }

                        Ok(serialize(&Frame::close()).unwrap().into())
                    })
                }
            });
        }

        let mut service = mock_service();
        let response_sent = response_sent.clone();
        service.expect_run().returning(move |ctx| {
            let test_data_bytes = test_data_bytes.clone();
            let response_sent = response_sent.clone();
            fut(async move {
                let (path, headers, mut reader) = ctx.request.into_parts();

                let mut data = vec![];

                while let Some(chunk) = reader
                    .read_body_chunk()
                    .await
                    .expect("Failed to read body chunk")
                {
                    data.extend_from_slice(&chunk);
                }
                let trailers =
                    reader.trailers().expect("Failed to read trailers");

                let mut response = ctx
                    .response
                    .start(ResponseStatusCode::SUCCESS)
                    .await
                    .expect("Failed to start response");

                response
                    .write_body_chunk(test_data_bytes.clone())
                    .await
                    .expect("Failed to write body chunk");

                response.end().await.expect("Failed to wait for response");

                assert_eq!(headers, None);
                assert_eq!(path, TEST_PATH);
                assert_eq!(trailers, None);
                assert_eq!(data, test_data_bytes);

                *response_sent.lock().await = true;

                Ok(())
            })
        });

        let rpc = rpc_with_service(transport, service);

        rpc.run().await.expect("Failed to run RPC");
    }

    #[tokio::test]
    #[test_log::test]
    #[timeout(1000)]
    async fn test_create_client_handle() {
        let empty_rpc = empty_rpc(MockTransport::new());

        let run = {
            let empty_rpc = empty_rpc.clone();
            tokio::spawn(async move {
                empty_rpc.run().await.expect("Failed to run RPC");
            })
        };

        let create = {
            let empty_rpc = empty_rpc.clone();
            tokio::spawn(async move {
                let client_handle_result =
                    empty_rpc.create_client_handle().await;

                assert!(client_handle_result.is_ok());
            })
        };

        let close = {
            let empty_rpc = empty_rpc.clone();
            tokio::spawn(async move {
                empty_rpc.close().await.expect("Failed to close RPC");
            })
        };

        _ = tokio::join!(run, create, close);
    }
}
