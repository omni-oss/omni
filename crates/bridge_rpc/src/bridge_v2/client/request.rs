use derive_new::new;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

pub use super::request_error as error;
use super::response::{Response, error::ResponseResult};

use error::{RequestErrorInner, RequestResult};

use super::super::{
    Headers, RequestErrorCode, Trailers,
    frame::{ChannelResponseFrame, Frame, ResponseError},
    id::Id,
    utils::send_frame_to_channel,
};

#[derive(new)]
pub struct PendingRequest {
    id: Id,
    path: String,
    request_bytes_tx: mpsc::Sender<Vec<u8>>,
    response_error_rx: oneshot::Receiver<ResponseError>,
    response_frame_rx: mpsc::Receiver<ChannelResponseFrame>,
}

impl PendingRequest {
    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(request_id = ?self.id)))]
    pub async fn start_with_headers(
        mut self,
        headers: Headers,
    ) -> RequestResult<ActiveRequest> {
        send_frame_to_channel(
            &self.request_bytes_tx,
            &Frame::request_start(self.id, self.path),
        )
        .await?;

        return_if_error(&mut self.response_error_rx).await?;

        send_frame_to_channel(
            &self.request_bytes_tx,
            &Frame::request_headers(self.id, headers),
        )
        .await?;

        return_if_error(&mut self.response_error_rx).await?;

        Ok(ActiveRequest::new(
            self.id,
            self.request_bytes_tx,
            self.response_frame_rx,
            self.response_error_rx,
        ))
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(request_id = ?self.id)))]
    pub async fn start(mut self) -> RequestResult<ActiveRequest> {
        send_frame_to_channel(
            &self.request_bytes_tx,
            &Frame::request_start(self.id, self.path),
        )
        .await?;

        return_if_error(&mut self.response_error_rx).await?;

        Ok(ActiveRequest::new(
            self.id,
            self.request_bytes_tx,
            self.response_frame_rx,
            self.response_error_rx,
        ))
    }
}

struct RequestDataImpl {
    id: Id,
    is_ended: bool,
    request_bytes_tx: mpsc::Sender<Vec<u8>>,
}

impl Drop for RequestDataImpl {
    fn drop(&mut self) {
        if self.is_ended {
            return;
        }

        let tx = self.request_bytes_tx.clone();
        let frame = Frame::request_end(self.id);
        tokio::spawn(async move {
            let result = send_frame_to_channel(&tx, &frame).await;

            if let Err(e) = result {
                trace::error!("failed to send stream end frame: {}", e);
            }
        });
    }
}

pub struct ActiveRequest {
    data: RequestDataImpl,
    response_frame_rx: mpsc::Receiver<ChannelResponseFrame>,
    response_error_rx: oneshot::Receiver<ResponseError>,
}

impl ActiveRequest {
    pub fn new(
        id: Id,
        request_bytes_tx: mpsc::Sender<Vec<u8>>,
        response_frame_rx: mpsc::Receiver<ChannelResponseFrame>,
        response_error_rx: oneshot::Receiver<ResponseError>,
    ) -> Self {
        Self {
            data: RequestDataImpl {
                id: id,
                is_ended: false,
                request_bytes_tx,
            },
            response_error_rx,
            response_frame_rx,
        }
    }
}

impl ActiveRequest {
    pub fn id(&self) -> Id {
        self.data.id
    }
}

impl ActiveRequest {
    async fn return_if_error(&mut self) -> RequestResult<()> {
        return_if_error(&mut self.response_error_rx).await
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(request_id = ?self.data.id)))]
    pub async fn write_body_chunk(
        &mut self,
        bytes: Vec<u8>,
    ) -> RequestResult<()> {
        self.return_if_error().await?;

        send_frame_to_channel(
            &self.data.request_bytes_tx,
            &Frame::request_body_chunk(self.data.id, bytes),
        )
        .await?;

        self.return_if_error().await?;

        Ok(())
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(request_id = ?self.data.id)))]
    pub async fn end(mut self) -> RequestResult<EndedRequest> {
        self.data.is_ended = true;

        self.return_if_error().await?;

        send_frame_to_channel(
            &self.data.request_bytes_tx,
            &Frame::request_end(self.data.id),
        )
        .await?;

        self.return_if_error().await?;

        Ok(EndedRequest::new(
            self.data.id,
            self.response_frame_rx,
            self.response_error_rx,
        ))
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(request_id = ?self.data.id)))]
    pub async fn end_with_trailers(
        mut self,
        trailers: Trailers,
    ) -> RequestResult<EndedRequest> {
        self.return_if_error().await?;

        send_frame_to_channel(
            &self.data.request_bytes_tx,
            &Frame::request_trailers(self.data.id, trailers),
        )
        .await?;

        self.end().await
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(request_id = ?self.data.id)))]
    pub async fn error(
        mut self,
        code: RequestErrorCode,
        message: impl Into<String>,
    ) -> RequestResult<()> {
        self.return_if_error().await?;

        send_frame_to_channel(
            &self.data.request_bytes_tx,
            &Frame::request_error(self.data.id, code, message.into()),
        )
        .await?;

        self.return_if_error().await?;

        Ok(())
    }
}

#[derive(new)]
pub struct EndedRequest {
    id: Id,
    response_rx: mpsc::Receiver<ChannelResponseFrame>,
    error_rx: oneshot::Receiver<ResponseError>,
}

impl EndedRequest {
    pub async fn start_response(self) -> ResponseResult<Response> {
        Response::init(self.id, self.response_rx, self.error_rx).await
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
            oneshot::error::TryRecvError::Empty
            | oneshot::error::TryRecvError::Closed => return Ok(()),
        },
    }
}
