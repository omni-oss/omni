use derive_new::new;
use tokio::sync::mpsc;

pub use super::response_error as error;

use error::ResponseResult;

use super::super::{
    super::Id, Headers, ResponseErrorCode, ResponseStatusCode, Trailers,
    frame::Frame, utils::send_frame_to_channel,
};

#[derive(new)]
pub struct PendingResponse {
    id: Id,
    tx: mpsc::Sender<Vec<u8>>,
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
        send_frame_to_channel(
            &self.tx,
            &Frame::response_start(self.id, status, headers),
        )
        .await?;

        Ok(ActiveResponse::new(self.id, self.tx))
    }
}

struct ResponseDataImpl {
    id: Id,
    is_ended: bool,
    tx: mpsc::Sender<Vec<u8>>,
}

impl Drop for ResponseDataImpl {
    fn drop(&mut self) {
        if self.is_ended {
            return;
        }

        let tx = self.tx.clone();
        let frame = Frame::response_end(self.id, None);
        tokio::spawn(async move {
            let result = send_frame_to_channel(&tx, &frame).await;

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
    pub(self) fn new(id: Id, tx: mpsc::Sender<Vec<u8>>) -> Self {
        Self {
            data: ResponseDataImpl {
                id: id,
                is_ended: false,
                tx,
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
        send_frame_to_channel(
            &self.data.tx,
            &Frame::response_body_chunk(self.data.id, bytes),
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

        send_frame_to_channel(
            &self.data.tx,
            &Frame::response_end(self.data.id, trailers),
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
        send_frame_to_channel(
            &self.data.tx,
            &Frame::response_error(self.data.id, code, message.into()),
        )
        .await?;

        Ok(())
    }
}
