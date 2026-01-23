use derive_new::new;
use strum::IntoDiscriminant as _;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::SendError;
use tokio::sync::oneshot;

pub use super::request_error as error;

use error::{RequestErrorInner, RequestResult};

use super::{
    super::{
        super::Id,
        Headers, RequestErrorCode, Trailers,
        frame::{Frame, ResponseError, ResponseStart},
    },
    response::{PendingResponse, ResponseFrameEvent},
};

#[derive(new)]
pub struct PendingRequest {
    id: Id,
    path: String,
    frame_sender: mpsc::Sender<Frame>,
    response_start_receiver: oneshot::Receiver<ResponseStart>,
    response_frame_receiver: mpsc::Receiver<ResponseFrameEvent>,
    response_error_receiver: oneshot::Receiver<ResponseError>,
}

impl PendingRequest {
    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(request_id = ?self.id)))]
    pub async fn start_with_headers(
        mut self,
        headers: Headers,
    ) -> RequestResult<ActiveRequest> {
        send_frame(
            &self.frame_sender,
            Frame::request_start(self.id, self.path, Some(headers)),
        )
        .await?;

        return_if_error(&mut self.response_error_receiver).await?;

        Ok(ActiveRequest::new(
            self.id,
            self.frame_sender,
            self.response_start_receiver,
            self.response_frame_receiver,
            self.response_error_receiver,
        ))
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(request_id = ?self.id)))]
    pub async fn start(mut self) -> RequestResult<ActiveRequest> {
        send_frame(
            &self.frame_sender,
            Frame::request_start(self.id, self.path, None),
        )
        .await?;

        return_if_error(&mut self.response_error_receiver).await?;

        Ok(ActiveRequest::new(
            self.id,
            self.frame_sender,
            self.response_start_receiver,
            self.response_frame_receiver,
            self.response_error_receiver,
        ))
    }
}

struct RequestDataImpl {
    id: Id,
    is_ended: bool,
    frame_sender: mpsc::Sender<Frame>,
}

impl Drop for RequestDataImpl {
    fn drop(&mut self) {
        if self.is_ended {
            return;
        }

        let tx = self.frame_sender.clone();
        let frame = Frame::request_end(self.id, None);
        tokio::spawn(async move {
            let result = tx.send(frame).await;

            if let Err(e) = result {
                trace::error!("failed to send stream end frame: {}", e);
            }
        });
    }
}

pub struct ActiveRequest {
    data: RequestDataImpl,
    response_start_receiver: oneshot::Receiver<ResponseStart>,
    response_frame_receiver: mpsc::Receiver<ResponseFrameEvent>,
    response_error_receiver: oneshot::Receiver<ResponseError>,
}

impl ActiveRequest {
    pub(self) fn new(
        id: Id,
        frame_sender: mpsc::Sender<Frame>,
        response_start_receiver: oneshot::Receiver<ResponseStart>,
        response_frame_receiver: mpsc::Receiver<ResponseFrameEvent>,
        response_error_receiver: oneshot::Receiver<ResponseError>,
    ) -> Self {
        Self {
            data: RequestDataImpl {
                id: id,
                is_ended: false,
                frame_sender,
            },
            response_error_receiver,
            response_frame_receiver,
            response_start_receiver,
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
        return_if_error(&mut self.response_error_receiver).await
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(request_id = ?self.data.id)))]
    pub async fn write_body_chunk(
        &mut self,
        bytes: Vec<u8>,
    ) -> RequestResult<()> {
        self.return_if_error().await?;

        send_frame(
            &self.data.frame_sender,
            Frame::request_body_chunk(self.data.id, bytes),
        )
        .await?;

        self.return_if_error().await?;

        Ok(())
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(request_id = ?self.data.id)))]
    async fn end_inner(
        mut self,
        trailers: Option<Trailers>,
    ) -> RequestResult<PendingResponse> {
        self.data.is_ended = true;

        self.return_if_error().await?;

        send_frame(
            &self.data.frame_sender,
            Frame::request_end(self.data.id, trailers),
        )
        .await?;

        self.return_if_error().await?;

        Ok(PendingResponse::new(
            self.data.id,
            self.response_start_receiver,
            self.response_frame_receiver,
            self.response_error_receiver,
        ))
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(request_id = ?self.data.id)))]
    pub async fn end(self) -> RequestResult<PendingResponse> {
        self.end_inner(None).await
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(request_id = ?self.data.id)))]
    pub async fn end_with_trailers(
        self,
        trailers: Trailers,
    ) -> RequestResult<PendingResponse> {
        self.end_inner(Some(trailers)).await
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(request_id = ?self.data.id)))]
    pub async fn error(
        mut self,
        code: RequestErrorCode,
        message: impl Into<String>,
    ) -> RequestResult<()> {
        self.return_if_error().await?;

        send_frame(
            &self.data.frame_sender,
            Frame::request_error(self.data.id, code, message.into()),
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
            oneshot::error::TryRecvError::Empty
            | oneshot::error::TryRecvError::Closed => return Ok(()),
        },
    }
}

async fn send_frame(
    frame_sender: &mpsc::Sender<Frame>,
    frame: Frame,
) -> RequestResult<()> {
    let result = frame_sender.send(frame).await;

    if let Err(SendError(frame)) = result {
        return Err(RequestErrorInner::Send {
            error: eyre::eyre!(
                "failed to send frame of type {}",
                frame.discriminant()
            ),
        }
        .into());
    }

    Ok(())
}
