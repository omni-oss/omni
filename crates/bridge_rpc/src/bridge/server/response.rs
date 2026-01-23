use derive_new::new;
use strum::IntoDiscriminant as _;
use tokio::sync::mpsc::{self, error::SendError};

use crate::server::response_error::ResponseErrorInner;

pub use super::response_error as error;

use error::ResponseResult;

use super::super::{
    super::Id, Headers, ResponseErrorCode, ResponseStatusCode, Trailers,
    frame::Frame,
};

#[derive(new)]
pub struct PendingResponse {
    id: Id,
    frame_sender: mpsc::Sender<Frame>,
}

impl PendingResponse {
    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(response_id = ?self.id)))]
    pub async fn start_with_headers(
        self,
        status: ResponseStatusCode,
        headers: Headers,
    ) -> ResponseResult<ActiveResponse> {
        self.start_inner(status, Some(headers)).await
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(response_id = ?self.id)))]
    pub async fn start(
        self,
        status: ResponseStatusCode,
    ) -> ResponseResult<ActiveResponse> {
        self.start_inner(status, None).await
    }

    async fn start_inner(
        self,
        status: ResponseStatusCode,
        headers: Option<Headers>,
    ) -> ResponseResult<ActiveResponse> {
        send_frame(
            &self.frame_sender,
            Frame::response_start(self.id, status, headers),
        )
        .await?;

        Ok(ActiveResponse::new(self.id, self.frame_sender))
    }
}

struct ResponseDataImpl {
    id: Id,
    is_ended: bool,
    frame_sender: mpsc::Sender<Frame>,
}

impl Drop for ResponseDataImpl {
    fn drop(&mut self) {
        if self.is_ended {
            return;
        }

        let tx = self.frame_sender.clone();
        let frame = Frame::response_end(self.id, None);
        tokio::spawn(async move {
            let result = tx.send(frame).await;

            if let Err(e) = result {
                trace::error!("failed to send stream end frame: {}", e);
            }
        });
    }
}

pub struct ActiveResponse {
    data: ResponseDataImpl,
}

impl ActiveResponse {
    pub(self) fn new(id: Id, tx: mpsc::Sender<Frame>) -> Self {
        Self {
            data: ResponseDataImpl {
                id: id,
                is_ended: false,
                frame_sender: tx,
            },
        }
    }
}

impl ActiveResponse {
    pub fn id(&self) -> Id {
        self.data.id
    }
}

impl ActiveResponse {
    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(response_id = ?self.data.id)))]
    pub async fn write_body_chunk(
        &mut self,
        bytes: Vec<u8>,
    ) -> ResponseResult<()> {
        send_frame(
            &self.data.frame_sender,
            Frame::response_body_chunk(self.data.id, bytes),
        )
        .await?;

        Ok(())
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(response_id = ?self.data.id)))]
    pub async fn end(self) -> ResponseResult<()> {
        self.end_inner(None).await
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(response_id = ?self.data.id)))]
    pub async fn end_with_trailers(
        self,
        trailers: Trailers,
    ) -> ResponseResult<()> {
        self.end_inner(Some(trailers)).await
    }

    async fn end_inner(
        mut self,
        trailers: Option<Trailers>,
    ) -> ResponseResult<()> {
        self.data.is_ended = true;

        send_frame(
            &self.data.frame_sender,
            Frame::response_end(self.data.id, trailers),
        )
        .await?;

        Ok(())
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(response_id = ?self.data.id)))]
    pub async fn error(
        self,
        code: ResponseErrorCode,
        message: impl Into<String>,
    ) -> ResponseResult<()> {
        send_frame(
            &self.data.frame_sender,
            Frame::response_error(self.data.id, code, message.into()),
        )
        .await?;

        Ok(())
    }
}

async fn send_frame(
    frame_sender: &mpsc::Sender<Frame>,
    frame: Frame,
) -> ResponseResult<()> {
    let result = frame_sender.send(frame).await;

    if let Err(SendError(frame)) = result {
        return Err(ResponseErrorInner::Send {
            error: eyre::eyre!(
                "failed to send frame of type {}",
                frame.discriminant()
            ),
        }
        .into());
    }

    Ok(())
}
