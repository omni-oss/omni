use derive_new::new;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

pub use super::response_error as error;

use error::{ResponseErrorInner, ResponseResult};

use super::super::{
    Headers, ResponseErrorCode, ResponseStatusCode, Trailers,
    frame::{Frame, ResponseError},
    id::Id,
    utils::send_frame_to_channel,
};

#[derive(new)]
pub struct PendingResponse {
    id: Id,
    tx: mpsc::Sender<Vec<u8>>,
    error_rx: oneshot::Receiver<ResponseError>,
}

impl PendingResponse {
    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(response_id = ?self.id)))]
    pub async fn start_with_headers(
        mut self,
        status: ResponseStatusCode,
        headers: Headers,
    ) -> ResponseResult<ActiveResponse> {
        send_frame_to_channel(
            &self.tx,
            &Frame::response_start(self.id, status),
        )
        .await?;

        return_if_error(&mut self.error_rx).await?;

        send_frame_to_channel(
            &self.tx,
            &Frame::response_headers(self.id, headers),
        )
        .await?;

        return_if_error(&mut self.error_rx).await?;

        Ok(ActiveResponse::new(self.id, self.tx, self.error_rx))
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(response_id = ?self.id)))]
    pub async fn start(
        mut self,
        status: ResponseStatusCode,
    ) -> ResponseResult<ActiveResponse> {
        send_frame_to_channel(
            &self.tx,
            &Frame::response_start(self.id, status),
        )
        .await?;

        return_if_error(&mut self.error_rx).await?;

        Ok(ActiveResponse::new(self.id, self.tx, self.error_rx))
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
        let frame = Frame::response_end(self.id);
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
    error_rx: oneshot::Receiver<ResponseError>,
}

impl ActiveResponse {
    pub fn new(
        id: Id,
        tx: mpsc::Sender<Vec<u8>>,
        error_rx: oneshot::Receiver<ResponseError>,
    ) -> Self {
        Self {
            data: ResponseDataImpl {
                id: id,
                is_ended: false,
                tx,
            },
            error_rx,
        }
    }
}

impl ActiveResponse {
    pub fn id(&self) -> Id {
        self.data.id
    }
}

impl ActiveResponse {
    async fn return_if_error(&mut self) -> ResponseResult<()> {
        return_if_error(&mut self.error_rx).await
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(response_id = ?self.data.id)))]
    pub async fn write_body_chunk(
        &mut self,
        bytes: Vec<u8>,
    ) -> ResponseResult<()> {
        self.return_if_error().await?;

        send_frame_to_channel(
            &self.data.tx,
            &Frame::response_body_chunk(self.data.id, bytes),
        )
        .await?;

        self.return_if_error().await?;

        Ok(())
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(response_id = ?self.data.id)))]
    pub async fn end(mut self) -> ResponseResult<()> {
        self.data.is_ended = true;

        self.return_if_error().await?;

        send_frame_to_channel(
            &self.data.tx,
            &Frame::response_end(self.data.id),
        )
        .await?;

        self.return_if_error().await?;

        Ok(())
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(response_id = ?self.data.id)))]
    pub async fn end_with_trailers(
        mut self,
        trailers: Trailers,
    ) -> ResponseResult<()> {
        self.return_if_error().await?;

        send_frame_to_channel(
            &self.data.tx,
            &Frame::response_trailers(self.data.id, trailers),
        )
        .await?;

        self.end().await
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(response_id = ?self.data.id)))]
    pub async fn error(
        mut self,
        code: ResponseErrorCode,
        message: impl Into<String>,
    ) -> ResponseResult<()> {
        self.return_if_error().await?;

        send_frame_to_channel(
            &self.data.tx,
            &Frame::response_error(self.data.id, code, message.into()),
        )
        .await?;

        self.return_if_error().await?;

        Ok(())
    }
}

async fn return_if_error(
    error_rx: &mut oneshot::Receiver<ResponseError>,
) -> ResponseResult<()> {
    match error_rx.try_recv() {
        Ok(error) => {
            return Err(
                ResponseErrorInner::ReceivedResponseErrorFrame(error).into()
            );
        }
        Err(e) => match e {
            oneshot::error::TryRecvError::Empty
            | oneshot::error::TryRecvError::Closed => return Ok(()),
        },
    }
}
