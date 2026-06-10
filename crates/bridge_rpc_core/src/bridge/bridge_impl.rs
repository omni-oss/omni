use futures::FutureExt as _;
use strum::IntoDiscriminant;
use tokio::sync::{
    Mutex,
    oneshot::{self},
};
use tokio::sync::{mpsc, watch};
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
use super::ResponseErrorCode;
use super::ResponseStatusCode;
use super::client::request::*;
use super::client::response::ResponseFrameEvent;
use super::client_handle::ClientHandle;
use super::constants::{BYTES_WORKER_BUFFER_SIZE, RESPONSE_BUFFER_SIZE};
use super::contexts::*;
use super::frame::*;
use super::frame_transporter::FrameTransporter;
use super::server;
use super::server::request::RequestFrameEvent;
use super::service::{
    Service, ServiceContext,
    error::{ServiceError, ServiceResult},
};
use super::session::*;
use super::{Headers, Trailers};
use crate::Transport;
use crate::bridge::utils::send_frame_to_transport;

pub type BoxStream<'a, T> = Pin<Box<dyn TokioStream<Item = T> + Send + 'a>>;

pub struct BridgeRpc<TTransport: Transport, TService: Service> {
    id: Id,
    service: Arc<TService>,
    transport: Arc<TTransport>,
    pending_ping: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    frame_transporter: Arc<Mutex<Option<FrameTransporter>>>,
    service_tasks: Arc<Mutex<Option<JoinSet<Result<(), ServiceError>>>>>,
    session_manager:
        SessionManager<RequestSessionContext, ResponseSessionContext>,
    stop_signal_sender: Arc<Mutex<Option<watch::Sender<bool>>>>,
    client_handle: Arc<ClientHandle>,
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
            frame_transporter: self.frame_transporter.clone(),
            service_tasks: self.service_tasks.clone(),
            session_manager: self.session_manager.clone(),
            stop_signal_sender: self.stop_signal_sender.clone(),
            client_handle: self.client_handle.clone(),
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
        let id = Id::new();
        let session_manager = SessionManager::new();
        let frame_transporter = Arc::new(Mutex::new(None));
        Self {
            id,
            transport: Arc::new(transport),
            service: Arc::new(service),
            pending_ping: Arc::new(Mutex::new(None)),
            service_tasks: Arc::new(Mutex::new(None)),
            frame_transporter: frame_transporter.clone(),
            session_manager: session_manager.clone(),
            stop_signal_sender: Arc::new(Mutex::new(None)),
            client_handle: Arc::new(ClientHandle::new(
                id,
                session_manager,
                frame_transporter,
            )),
        }
    }
}

macro_rules! do_work_with_frame_transporter {
    ($self:expr, $work:expr) => {
        async move {
            let frame_transporter = $self.frame_transporter.lock().await;
            if let Some(frame_transporter) = frame_transporter.as_ref() {
                $work(frame_transporter).await
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
        let tx = self.clone_frame_sender().await?;
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
    pub fn get_client_handle(&self) -> Arc<ClientHandle> {
        self.client_handle.clone()
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, ret, fields(rpc_id = ?self.id)))]
    pub async fn ping(&self, timeout: Duration) -> BridgeRpcResult<bool> {
        if self.has_pending_probe().await {
            Err(BridgeRpcErrorInner::ProbeInProgress)?;
        }

        let (tx, rx) = oneshot::channel();
        self.pending_ping.lock().await.replace(tx);

        do_work_with_frame_transporter!(
            self,
            async |transporter: &FrameTransporter| {
                transporter.transport(Frame::ping()).await
            }
        )
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
        do_work_with_frame_transporter!(
            self,
            async |frame_transporter: &FrameTransporter| {
                frame_transporter.transport(Frame::close()).await
            }
        )
        .await?;

        if let Some(stop_signal_sender) =
            self.stop_signal_sender.lock().await.take()
        {
            stop_signal_sender.send(true).map_err(|_| {
                BridgeRpcErrorInner::new_send(eyre::eyre!(
                    "failed to send stop signal"
                ))
            })?;
        }

        Ok(())
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(rpc_id = ?self.id)))]
    pub async fn run(&self) -> BridgeRpcResult<()> {
        if self.stop_signal_sender.lock().await.is_some() {
            return Err(BridgeRpcErrorInner::AlreadyRunning)?;
        }

        let (stop_signal_sender, mut stop_signal_receiver) =
            watch::channel(false);
        *self.stop_signal_sender.lock().await = Some(stop_signal_sender);

        self.clear_handler_tasks().await?;

        let (frame_sender, mut frame_receiver) =
            mpsc::channel::<Frame>(BYTES_WORKER_BUFFER_SIZE);
        let task = {
            let transport = self.transport.clone();

            trace::if_enabled! {
                let id = self.id;
            };

            tokio::spawn(async move {
                while let Some(frame) = frame_receiver.recv().await {
                    let fut =
                        send_frame_to_transport(transport.as_ref(), &frame);

                    trace::if_enabled! {
                        let span = trace::info_span!("run_send_task", rpc_id = ?id);
                        let fut = fut.instrument(span);
                    };

                    fut.await
                        .inspect_err(|e| {
                            trace::error!(
                                error = ?e,
                                "failed_to_send_bytes_to_transport",
                            )
                        })
                        .ok();
                }

                trace::trace!("stream_data_task_stopped");
            })
        };
        let abort_handle = task.abort_handle();

        let bytes_worker = FrameTransporter {
            sender: frame_sender.clone(),
            abort_handle,
        };

        let existing =
            self.frame_transporter.lock().await.replace(bytes_worker);

        if let Some(existing) = existing {
            existing.abort_handle.abort();

            trace::trace!("closed_existing_bytes_worker");
        }

        trace::trace!("running_rpc");

        loop {
            tokio::select! {
                bytes = self.transport.receive() => {
                    let bytes = bytes.map_err(|e| {
                        BridgeRpcErrorInner::new_transport(eyre::eyre!(e.to_string()))
                    })?;

                    let result = self.handle_receive(bytes, &frame_sender).await?;
                    if result.is_break() {
                        trace::trace!("received_stop_signal");
                        break;
                    }
                }
                _ = stop_signal_receiver.changed() => {
                    trace::trace!("received_stop_signal");
                    break;
                }
            }
        }

        let frame_transporter = self.frame_transporter.lock().await.take();

        if let Some(bytes_worker) = frame_transporter {
            bytes_worker.abort_handle.abort();
            trace::trace!("stopped_bytes_worker");
        }

        self.clear_handler_tasks().await?;
        trace::trace!("cleared_service_tasks");

        trace::trace!("stopped_rpc");

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
                match result {
                    Ok(()) => {}
                    Err(e) => {
                        trace::error!(error = ?e, "service_task_failed");
                    }
                }
            }
        }

        _ = self.service_tasks.lock().await.insert(Default::default());

        Ok(())
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(rpc_id = ?self.id, bytes_length = bytes.len())))]
    async fn handle_receive(
        &self,
        bytes: bytes::Bytes,
        frame_sender: &mpsc::Sender<Frame>,
    ) -> Result<ControlFlow<()>, BridgeRpcError> {
        let incoming: Frame = rmp_serde::from_slice(&bytes)
            .map_err(BridgeRpcErrorInner::Deserialization)?;

        trace::trace!(
            frame_type = ?incoming.discriminant(),
            "received_frame_from_rpc"
        );

        enum Event {
            Request(RequestEvent),
            Response(ResponseEvent),
        }

        let ev = match incoming {
            Frame::Close => {
                self.handle_close().await;
                return Ok(ControlFlow::Break(()));
            }
            Frame::Ping => {
                self.handle_ping(frame_sender).await;
                return Ok(ControlFlow::Continue(()));
            }
            Frame::Pong => {
                self.handle_pong().await;
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
                let event_id = request_event.id();

                // Get or create the request session; on failure send a
                // protocol-level error back and keep running.
                let (session, request_receiver, request_error_receiver) =
                    if let RequestEvent::Start(ref request_start) =
                        request_event
                    {
                        match self.start_request_session(request_start.id).await
                        {
                            Ok((session, req_rx, err_rx)) => {
                                (session, Some(req_rx), Some(err_rx))
                            }
                            Err(e) => {
                                trace::warn!(
                                    error = ?e,
                                    id = ?event_id,
                                    "failed_to_start_request_session"
                                );
                                // Close the existing session so its
                                // service task can observe the EOF and
                                // exit cleanly instead of hanging.
                                self.close_request_session(event_id).await;
                                Self::send_response_error_frame(
                                    frame_sender,
                                    event_id,
                                    format!("cannot start session: {e}"),
                                )
                                .await;
                                return Ok(ControlFlow::Continue(()));
                            }
                        }
                    } else {
                        match self.get_request_session(event_id).await {
                            Some(session) => (session, None, None),
                            None => {
                                trace::warn!(
                                    id = ?event_id,
                                    "no_request_session_found"
                                );
                                Self::send_response_error_frame(
                                    frame_sender,
                                    event_id,
                                    format!(
                                        "no request session for id: \
                                         {event_id}"
                                    ),
                                )
                                .await;
                                return Ok(ControlFlow::Continue(()));
                            }
                        }
                    };

                let id = session.lock().await.id();
                let should_close_session =
                    request_event.is_error() || request_event.is_end();

                let transition_result =
                    session.lock().await.state_mut().transition(request_event);

                let output = match transition_result {
                    Ok(output) => output,
                    Err(e) => {
                        trace::warn!(
                            error = ?e,
                            id = ?id,
                            "request_state_machine_error"
                        );
                        // Lock is NOT held here — close_request_session
                        // can safely re-acquire it.
                        Self::send_response_error_frame(
                            frame_sender,
                            id,
                            format!("protocol error: {e}"),
                        )
                        .await;
                        self.close_request_session(id).await;
                        return Ok(ControlFlow::Continue(()));
                    }
                };

                match output {
                    RequestStateTransitionOutput::Wait => {
                        // do nothing
                    }
                    RequestStateTransitionOutput::Start {
                        id,
                        path,
                        headers,
                    } => {
                        if let Err(e) = self
                            .handle_request_start(
                                id,
                                path,
                                headers,
                                request_receiver.expect("should be set"),
                                request_error_receiver.expect("should be set"),
                            )
                            .await
                        {
                            trace::error!(
                                error = ?e,
                                id = ?id,
                                "failed_to_handle_request_start"
                            );
                        }
                    }
                    RequestStateTransitionOutput::BodyChunk { chunk } => {
                        self.handle_request_body_chunk(session, chunk).await;
                    }
                    RequestStateTransitionOutput::End { trailers } => {
                        self.handle_request_end(session, trailers).await;
                    }
                    RequestStateTransitionOutput::Error { error } => {
                        self.handle_request_error(session, error).await;
                    }
                }

                if should_close_session {
                    self.close_request_session(id).await;
                }

                Ok(ControlFlow::Continue(()))
            }
            Event::Response(response_event) => {
                let id = response_event.id();

                // If there is no session it may be a late/spurious frame
                // (e.g. the session was already closed) — log and ignore.
                let session = match self.get_response_session(id).await {
                    Some(session) => session,
                    None => {
                        trace::warn!(
                            id = ?id,
                            "no_response_session_found_ignoring"
                        );
                        return Ok(ControlFlow::Continue(()));
                    }
                };

                let should_close_session =
                    response_event.is_error() || response_event.is_end();

                let transition_result =
                    session.lock().await.state_mut().transition(response_event);

                let output = match transition_result {
                    Ok(output) => output,
                    Err(e) => {
                        trace::warn!(
                            error = ?e,
                            id = ?id,
                            "response_state_machine_error"
                        );
                        // Lock is NOT held here — close_response_session
                        // can safely re-acquire it.
                        self.close_response_session(id).await;
                        return Ok(ControlFlow::Continue(()));
                    }
                };

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
                        .await;
                    }
                    ResponseStateTransitionOutput::BodyChunk { chunk } => {
                        self.handle_response_body_chunk(session, chunk).await;
                    }
                    ResponseStateTransitionOutput::End { trailers } => {
                        self.handle_response_end(session, trailers).await;
                    }
                    ResponseStateTransitionOutput::Error { error } => {
                        self.handle_response_error(session, error).await;
                    }
                }

                if should_close_session {
                    self.close_response_session(id).await;
                }

                Ok(ControlFlow::Continue(()))
            }
        }
    }

    async fn handle_close(&self) {
        trace::trace!("received_close_frame");
        if let Some(stop_signal_) = self.stop_signal_sender.lock().await.take()
        {
            if let Err(e) = stop_signal_.send(true) {
                trace::warn!(error = ?e, "failed_to_send_stop_signal");
            }
        }
    }

    async fn handle_ping(&self, frame_sender: &mpsc::Sender<Frame>) {
        trace::debug!("received_ping");
        if let Err(e) = frame_sender.send(Frame::pong()).await {
            trace::warn!(error = ?e, "failed_to_send_pong");
        } else {
            trace::trace!("sent_pong");
        }
    }

    async fn handle_pong(&self) {
        trace::debug!("received_pong");
        if let Some(rx) = self.pending_ping.lock().await.take() {
            if let Err(_) = rx.send(()) {
                trace::warn!("failed_to_send_probe_ack");
            } else {
                trace::trace!("removed_pending_ping_receiver");
            }
        }
    }

    async fn handle_request_start(
        &self,
        id: Id,
        path: String,
        headers: Option<Headers>,
        request_frame_receiver: RequestFrameReceiver,
        request_error_receiver: RequestErrorReceiver,
    ) -> Result<(), BridgeRpcError> {
        trace::trace!("received_request_start");
        let service = self.service.clone();

        let frame_sender = self.clone_frame_sender().await?;

        let request = server::request::Request::new(
            id,
            path,
            headers,
            request_frame_receiver,
            request_error_receiver,
        );

        let response = server::response::PendingResponse::new(id, frame_sender);

        let client_handle = self.client_handle.clone();

        self.spawn_service_task(async move {
            trace::trace!("running_service");
            let context = ServiceContext::new(request, response, client_handle);

            let result = service.run(context).await;

            if let Err(error) = result {
                trace::error!(error = ?error, "service_error");
                Err(error)?
            } else {
                trace::trace!("service_finished");
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
        trace::trace!("spawning_service_task");
        let mut tasks = self.service_tasks.lock().await;
        let tasks = tasks.get_or_insert_default();
        tasks.spawn(future);
    }

    async fn handle_request_body_chunk(
        &self,
        session: Arc<Mutex<RequestSession<RequestSessionContext>>>,
        chunk: Vec<u8>,
    ) {
        trace::trace!("received_request_body_chunk");
        if let Err(e) = session
            .lock()
            .await
            .context_mut()
            .request_frame_sender
            .send(RequestFrameEvent::new_body_chunk(chunk))
            .await
        {
            trace::warn!(error = ?e, "failed_to_forward_request_body_chunk");
        } else {
            trace::trace!("sent_request_body_chunk_to_session");
        }
    }

    async fn handle_request_end(
        &self,
        session: Arc<Mutex<RequestSession<RequestSessionContext>>>,
        trailers: Option<Trailers>,
    ) {
        trace::trace!("received_request_end");
        if let Err(e) = session
            .lock()
            .await
            .context_mut()
            .request_frame_sender
            .send(RequestFrameEvent::new_end(trailers))
            .await
        {
            trace::warn!(error = ?e, "failed_to_forward_request_end");
        } else {
            trace::trace!("forwarded_request_end");
        }
    }

    async fn handle_request_error(
        &self,
        session: Arc<Mutex<RequestSession<RequestSessionContext>>>,
        error: RequestError,
    ) {
        trace::trace!("received_request_error");
        let sender = session
            .lock()
            .await
            .context_mut()
            .request_error_sender
            .lock()
            .await
            .take();
        if let Some(sender) = sender {
            if let Err(_) = sender.send(error) {
                trace::warn!("failed_to_forward_request_error");
            } else {
                trace::trace!("forwarded_request_error");
            }
        }
    }

    async fn handle_response_start(
        &self,
        response_session: Arc<Mutex<ResponseSession<ResponseSessionContext>>>,
        id: Id,
        status: ResponseStatusCode,
        headers: Option<Headers>,
    ) {
        trace::trace!("received_response_start");
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
            if let Err(e) = start.send(response_start) {
                trace::warn!(id = ?e.id, "failed_to_forward_response_start");
            } else {
                trace::trace!("forwarded_response_start");
            }
        }
    }

    async fn handle_response_body_chunk(
        &self,
        response_session: Arc<Mutex<ResponseSession<ResponseSessionContext>>>,
        chunk: Vec<u8>,
    ) {
        trace::trace!("received_response_body_chunk");
        if let Err(e) = response_session
            .lock()
            .await
            .context_mut()
            .response_frame_sender
            .send(ResponseFrameEvent::new_body_chunk(chunk))
            .await
        {
            trace::warn!(error = ?e, "failed_to_forward_response_body_chunk");
        } else {
            trace::trace!("forwarded_response_body_chunk");
        }
    }

    async fn handle_response_end(
        &self,
        session: Arc<Mutex<ResponseSession<ResponseSessionContext>>>,
        trailers: Option<Trailers>,
    ) {
        trace::trace!("received_response_end");
        if let Err(e) = session
            .lock()
            .await
            .context_mut()
            .response_frame_sender
            .send(ResponseFrameEvent::new_end(trailers))
            .await
        {
            trace::warn!(error = ?e, "failed_to_forward_response_end");
        } else {
            trace::trace!("forwarded_response_end");
        }
    }

    async fn handle_response_error(
        &self,
        session: Arc<Mutex<ResponseSession<ResponseSessionContext>>>,
        response_error: ResponseError,
    ) {
        trace::trace!("received_response_error");
        let error = session
            .lock()
            .await
            .context_mut()
            .response_error_sender
            .lock()
            .await
            .take();
        if let Some(error) = error {
            if let Err(_) = error.send(response_error) {
                trace::warn!("failed_to_forward_response_error");
            } else {
                trace::trace!("forwarded_response_error");
            }
        }
    }

    async fn send_response_error_frame(
        frame_sender: &mpsc::Sender<Frame>,
        id: Id,
        message: impl Into<String>,
    ) {
        let frame = Frame::response_error(
            id,
            ResponseErrorCode::UNEXPECTED_FRAME,
            message.into(),
        );
        if let Err(e) = frame_sender.send(frame).await {
            trace::warn!(error = ?e, id = ?id, "failed_to_send_response_error_frame");
        }
    }

    async fn clone_frame_sender(&self) -> BridgeRpcResult<mpsc::Sender<Frame>> {
        trace::trace!("cloning bytes sender");
        let transporter = self.frame_transporter.lock().await;
        if let Some(transporter) = transporter.as_ref() {
            Ok(transporter.sender.clone())
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
    use crate::{BridgeRpcErrorKind, MockTransport};
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

        transport.expect_receive().returning(move || {
            delayed(
                Ok(serialize(&Frame::close())
                    .expect("Failed to serialize close frame")
                    .into()),
                Duration::from_millis(20),
            )
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
    async fn test_should_not_all_double_run() {
        let mut transport = mock_transport();

        transport
            .expect_send()
            .returning(|_| delayed(Ok(()), Duration::from_millis(5)));

        transport.expect_receive().returning(move || {
            delayed(
                Ok(serialize(&Frame::close())
                    .expect("Failed to serialize close frame")
                    .into()),
                Duration::from_millis(20),
            )
        });

        let rpc = empty_rpc(transport);

        let rpc2 = rpc.clone();
        tokio::spawn(async move {
            rpc2.run().await.expect("Failed to run RPC");
            trace::trace!("rpc2_run_done");
        });
        yield_now().await; // wait for rpc2 to proceed to run

        let result = rpc.run().await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err().kind(),
            BridgeRpcErrorKind::AlreadyRunning
        ));

        trace::trace!("test_should_not_all_double_run");

        rpc.close().await.expect("Failed to close RPC");
    }
}
