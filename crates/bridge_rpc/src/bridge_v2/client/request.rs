use derive_new::new;
use serde::Serialize;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

pub use super::request_error as error;
use super::response::Response;

use error::{RequestErrorInner, RequestResult};

use super::super::{
    Headers, RequestErrorCode, Trailers,
    frame::{ChannelResponseFrame, Frame, ResponseError},
    id::Id,
    utils::send_frame_to_channel,
};

#[derive(new)]
pub struct NewRequest {
    id: Id,
    path: String,
    tx: mpsc::Sender<Vec<u8>>,
    error_rx: oneshot::Receiver<ResponseError>,
    response_rx: mpsc::Receiver<ChannelResponseFrame>,
}

impl NewRequest {
    pub async fn start_with_headers(
        mut self,
        headers: Headers,
    ) -> RequestResult<Request> {
        send_frame_to_channel(
            &self.tx,
            &Frame::request_start(self.id, self.path),
        )
        .await?;

        return_if_error(&mut self.error_rx).await?;

        send_frame_to_channel(
            &self.tx,
            &Frame::request_headers(self.id, headers),
        )
        .await?;

        return_if_error(&mut self.error_rx).await?;

        Ok(Request::new(
            self.id,
            self.tx,
            self.error_rx,
            self.response_rx,
        ))
    }

    pub async fn start(mut self) -> RequestResult<Request> {
        send_frame_to_channel(
            &self.tx,
            &Frame::request_start(self.id, self.path),
        )
        .await?;

        return_if_error(&mut self.error_rx).await?;

        Ok(Request::new(
            self.id,
            self.tx,
            self.error_rx,
            self.response_rx,
        ))
    }
}

struct RequestDataImpl {
    id: Id,
    is_ended: bool,
    tx: mpsc::Sender<Vec<u8>>,
}

impl Drop for RequestDataImpl {
    fn drop(&mut self) {
        if self.is_ended {
            return;
        }

        let tx = self.tx.clone();
        let frame = Frame::request_end(self.id);
        tokio::spawn(async move {
            let result = send_frame_to_channel(&tx, &frame).await;

            if let Err(e) = result {
                trace::error!("failed to send stream end frame: {}", e);
            }
        });
    }
}

pub struct Request {
    data: RequestDataImpl,
    error_rx: oneshot::Receiver<ResponseError>,
    response_rx: mpsc::Receiver<ChannelResponseFrame>,
}

impl Request {
    pub fn new(
        id: Id,
        tx: mpsc::Sender<Vec<u8>>,
        error_rx: oneshot::Receiver<ResponseError>,
        response_rx: mpsc::Receiver<ChannelResponseFrame>,
    ) -> Self {
        Self {
            data: RequestDataImpl {
                id: id,
                is_ended: false,
                tx,
            },
            error_rx,
            response_rx,
        }
    }
}

impl Request {
    pub fn id(&self) -> Id {
        self.data.id
    }
}

impl Request {
    async fn return_if_error(&mut self) -> RequestResult<()> {
        return_if_error(&mut self.error_rx).await
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(call_id = ?self.data.id)))]
    pub async fn data<TData: Serialize>(
        &mut self,
        bytes: Vec<u8>,
    ) -> RequestResult<()> {
        self.return_if_error().await?;

        send_frame_to_channel(
            &self.data.tx,
            &Frame::request_body_chunk(self.data.id, bytes),
        )
        .await?;

        self.return_if_error().await?;

        Ok(())
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(call_id = ?self.data.id)))]
    pub async fn end(mut self) -> RequestResult<Response> {
        self.return_if_error().await?;

        send_frame_to_channel(&self.data.tx, &Frame::request_end(self.data.id))
            .await?;

        self.return_if_error().await?;
        let response =
            Response::init(self.data.id, self.response_rx, self.error_rx)
                .await?;

        Ok(response)
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(call_id = ?self.data.id)))]
    pub async fn end_with_trailers(
        mut self,
        trailers: Trailers,
    ) -> RequestResult<()> {
        self.return_if_error().await?;

        send_frame_to_channel(
            &self.data.tx,
            &Frame::request_trailers(self.data.id, trailers),
        )
        .await?;

        self.data.is_ended = true;

        self.return_if_error().await?;

        send_frame_to_channel(&self.data.tx, &Frame::request_end(self.data.id))
            .await?;

        self.return_if_error().await?;

        Ok(())
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(call_id = ?self.data.id)))]
    pub async fn error(
        mut self,
        code: RequestErrorCode,
        message: impl Into<String>,
    ) -> RequestResult<()> {
        self.return_if_error().await?;

        send_frame_to_channel(
            &self.data.tx,
            &Frame::request_error(self.data.id, code, message.into()),
        )
        .await?;

        self.return_if_error().await?;

        Ok(())
    }
}

async fn return_if_error(
    error_rx: &mut oneshot::Receiver<ResponseError>,
) -> RequestResult<()> {
    match error_rx.try_recv() {
        Ok(error) => {
            return Err(
                RequestErrorInner::ReceivedResponseErrorFrame(error).into()
            );
        }
        Err(e) => match e {
            oneshot::error::TryRecvError::Empty => return Ok(()),
            oneshot::error::TryRecvError::Closed => return Ok(()),
        },
    }
}
