//! Reusable test harness for service unit tests.
//!
//! This module is gated behind `#[cfg(test)]` and is intended to be used by
//! sibling modules under `services::*` to construct [`ServiceContext`]s with
//! pre-loaded request bodies and to await the response that a service
//! produces (if any).
//!
//! Typical usage:
//!
//! ```ignore
//! let (ctx, awaiter) = ServiceContextBuilder::new("/my-service")
//!     .with_body_json(&payload)
//!     .build()
//!     .await;
//!
//! my_service.run(ctx).await.expect("service should run");
//!
//! let response = awaiter.wait().await;
//! assert_eq!(response.status, ResponseStatusCode::SUCCESS);
//! ```
#![allow(dead_code)]

use std::sync::Once;

use bridge_rpc_core::{
    DynMap, Id, ResponseStatusCode,
    frame::Frame,
    server::{
        request::{Request, RequestFrameEvent},
        response::PendingResponse,
    },
    service::ServiceContext,
};
use tokio::sync::{mpsc, oneshot};

/// Default capacity used for the channels that back the request/response
/// streams in tests. Large enough that almost any reasonable test can buffer
/// its data without blocking on send.
const DEFAULT_CHANNEL_CAPACITY: usize = 64;

/// Ensures that the global `log` max level is wide open, so that calls to
/// `log::log!(...)` from services-under-test are not silently filtered out by
/// the default [`log::LevelFilter::Off`].
///
/// This is safe to call multiple times concurrently from any test.
pub fn ensure_log_max_level_initialized() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        log::set_max_level(log::LevelFilter::Trace);
    });
}

/// Aggregated response collected by [`ResponseAwaiter::wait`].
#[derive(Debug)]
pub struct FullResponse {
    pub id: Id,
    pub headers: Option<DynMap>,
    pub status: ResponseStatusCode,
    pub body: Vec<u8>,
    pub trailers: Option<DynMap>,
}

impl FullResponse {
    /// Deserialize the body as JSON.
    pub fn body_as_json<T: for<'de> serde::Deserialize<'de>>(
        &self,
    ) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.body)
    }
}

/// Receives and aggregates response frames produced by a service.
pub struct ResponseAwaiter {
    frame_receiver: mpsc::Receiver<Frame>,
}

impl ResponseAwaiter {
    fn new(frame_receiver: mpsc::Receiver<Frame>) -> Self {
        Self { frame_receiver }
    }

    /// Wait for the entire response (start, body chunks, end) to be sent.
    ///
    /// Panics if the response channel is closed before a complete response
    /// is received, or if the frames are not in the expected order.
    pub async fn wait(mut self) -> FullResponse {
        let start_frame = self.frame_receiver.recv().await.expect(
            "response channel was closed before any response frame was sent",
        );

        let start = match start_frame {
            Frame::ResponseStart(start) => start,
            other => panic!(
                "expected `ResponseStart` frame, got: {:?}",
                FrameTag::from(&other)
            ),
        };

        let id = start.id;
        let headers = start.headers;
        let status = start.status;

        let mut body = Vec::new();
        loop {
            let frame = self.frame_receiver.recv().await.expect(
                "response channel was closed before a `ResponseEnd` frame was sent",
            );

            match frame {
                Frame::ResponseBodyChunk(chunk) => {
                    body.extend_from_slice(&chunk.chunk);
                }
                Frame::ResponseEnd(end) => {
                    return FullResponse {
                        id,
                        headers,
                        status,
                        body,
                        trailers: end.trailers,
                    };
                }
                other => panic!(
                    "expected `ResponseBodyChunk` or `ResponseEnd` frame, got: {:?}",
                    FrameTag::from(&other)
                ),
            }
        }
    }

    /// Tries to receive the next response frame without blocking.
    ///
    /// Returns `None` if no frame is currently available, regardless of
    /// whether the channel is still open.
    pub fn try_next_frame(&mut self) -> Option<Frame> {
        self.frame_receiver.try_recv().ok()
    }

    /// Returns true if no response frames were produced and the channel is
    /// closed (i.e. the service finished without ever starting a response).
    pub fn is_drained(&mut self) -> bool {
        matches!(
            self.frame_receiver.try_recv(),
            Err(mpsc::error::TryRecvError::Disconnected),
        )
    }
}

/// A small helper to produce readable panic messages without depending on
/// `Frame: Debug` (which can include large binary chunks).
#[derive(Debug)]
enum FrameTag {
    RequestStart,
    RequestBodyChunk,
    RequestEnd,
    RequestError,
    ResponseStart,
    ResponseBodyChunk,
    ResponseEnd,
    ResponseError,
    Close,
    Ping,
    Pong,
}

impl From<&Frame> for FrameTag {
    fn from(value: &Frame) -> Self {
        match value {
            Frame::RequestStart(_) => FrameTag::RequestStart,
            Frame::RequestBodyChunk(_) => FrameTag::RequestBodyChunk,
            Frame::RequestEnd(_) => FrameTag::RequestEnd,
            Frame::RequestError(_) => FrameTag::RequestError,
            Frame::ResponseStart(_) => FrameTag::ResponseStart,
            Frame::ResponseBodyChunk(_) => FrameTag::ResponseBodyChunk,
            Frame::ResponseEnd(_) => FrameTag::ResponseEnd,
            Frame::ResponseError(_) => FrameTag::ResponseError,
            Frame::Close => FrameTag::Close,
            Frame::Ping => FrameTag::Ping,
            Frame::Pong => FrameTag::Pong,
        }
    }
}

/// Builder for a [`ServiceContext`] suitable for unit-testing services.
///
/// The builder pre-loads the request stream with a single body chunk (if any
/// body has been configured) followed by an `End` frame, so that the service
/// under test can read the whole request body without further interaction.
pub struct ServiceContextBuilder {
    id: Id,
    path: String,
    headers: Option<DynMap>,
    body: Vec<u8>,
    trailers: Option<DynMap>,
}

impl ServiceContextBuilder {
    /// Create a new builder targeting the given service path.
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            id: Id::new(),
            path: path.into(),
            headers: None,
            body: Vec::new(),
            trailers: None,
        }
    }

    /// Override the request id (defaults to a freshly generated [`Id`]).
    pub fn with_id(mut self, id: Id) -> Self {
        self.id = id;
        self
    }

    /// Attach request headers.
    pub fn with_headers(mut self, headers: DynMap) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Configure the request body as raw bytes.
    pub fn with_body_bytes(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    /// Configure the request body by serializing the given value as JSON.
    ///
    /// Panics if serialization fails.
    pub fn with_body_json<T: serde::Serialize>(mut self, body: &T) -> Self {
        self.body = serde_json::to_vec(body)
            .expect("failed to serialize request body as JSON");
        self
    }

    /// Configure the request body by serializing the given value as
    /// MessagePack (named representation).
    ///
    /// Panics if serialization fails. Currently unused outside of opt-in
    /// callers - left here for the convenience of future tests and gated by
    /// the `rmp-serde` build dependency. Disabled until that crate is added.
    #[doc(hidden)]
    pub fn with_body_raw<T: AsRef<[u8]>>(mut self, body: T) -> Self {
        self.body = body.as_ref().to_vec();
        self
    }

    /// Attach request trailers, sent as part of the `End` frame.
    pub fn with_trailers(mut self, trailers: DynMap) -> Self {
        self.trailers = Some(trailers);
        self
    }

    /// Build a [`ServiceContext`] and a paired [`ResponseAwaiter`].
    ///
    /// The request body chunk (if any) and the `End` frame are pre-pushed
    /// into the request stream before this returns.
    ///
    /// As a side effect, the global [`log::max_level`](log::max_level) is
    /// initialized to [`log::LevelFilter::Trace`], so that services that emit
    /// log records via the global filter are observable from tests.
    pub async fn build(self) -> (ServiceContext, ResponseAwaiter) {
        ensure_log_max_level_initialized();

        let (request_frame_tx, request_frame_rx) =
            mpsc::channel(DEFAULT_CHANNEL_CAPACITY);
        let (_request_error_tx, request_error_rx) = oneshot::channel();
        let (response_frame_tx, response_frame_rx) =
            mpsc::channel(DEFAULT_CHANNEL_CAPACITY);

        let request = Request::new(
            self.id,
            self.path,
            self.headers,
            request_frame_rx,
            request_error_rx,
        );

        let pending_response = PendingResponse::new(self.id, response_frame_tx);

        if !self.body.is_empty() {
            request_frame_tx
                .send(RequestFrameEvent::BodyChunk { chunk: self.body })
                .await
                .expect("failed to push request body chunk into test stream");
        }

        request_frame_tx
            .send(RequestFrameEvent::End {
                trailers: self.trailers,
            })
            .await
            .expect("failed to push request `End` frame into test stream");

        // We deliberately leak the request_frame_tx into the receiver's
        // lifetime by dropping it here so that any further `recv()` on the
        // receiver returns `None`, simulating the client closing its side of
        // the request stream.
        drop(request_frame_tx);

        let context = ServiceContext::from_request_and_response(
            request,
            pending_response,
        );

        (context, ResponseAwaiter::new(response_frame_rx))
    }
}
