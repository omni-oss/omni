use derive_new::new;
use serde_repr::Deserialize_repr;
use serde_repr::Serialize_repr;
use strum::EnumDiscriminants;
use strum::EnumIs;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use super::super::status_code::ResponseStatusCode;
pub use super::response_error as error;

use error::{ResponseErrorInner, ResponseResult};

use super::super::{
    super::Id,
    Headers, Trailers,
    frame::{ResponseError, ResponseStart},
};

#[derive(new)]
pub struct PendingResponse {
    id: Id,
    response_start_receiver: oneshot::Receiver<ResponseStart>,
    response_frame_receiver: mpsc::Receiver<ResponseFrameEvent>,
    response_error_receiver: oneshot::Receiver<ResponseError>,
}

impl PendingResponse {
    pub async fn wait(
        self,
    ) -> Result<Response, super::response_error::ResponseError> {
        let response_start =
            self.response_start_receiver.await.map_err(|_| {
                ResponseErrorInner::FailedToReceiveResponseStartFrame {
                    response_id: self.id,
                }
            })?;

        Ok(Response::new(
            self.id,
            response_start.status,
            response_start.headers,
            self.response_frame_receiver,
            self.response_error_receiver,
        ))
    }
}

#[derive(new)]
pub struct Response {
    id: Id,
    status: ResponseStatusCode,
    headers: Option<Headers>,
    response_frame_receiver: mpsc::Receiver<ResponseFrameEvent>,
    response_error_receiver: oneshot::Receiver<ResponseError>,
}

impl Response {
    pub fn headers(&self) -> Option<&Headers> {
        self.headers.as_ref()
    }

    pub fn status(&self) -> ResponseStatusCode {
        self.status
    }

    pub fn into_parts(
        self,
    ) -> (ResponseStatusCode, Option<Headers>, ResponseReader) {
        (
            self.status,
            self.headers,
            ResponseReader::new(
                self.id,
                self.response_frame_receiver,
                self.response_error_receiver,
            ),
        )
    }

    pub fn into_reader(self) -> ResponseReader {
        ResponseReader::new(
            self.id,
            self.response_frame_receiver,
            self.response_error_receiver,
        )
    }
}

#[derive(new)]
pub struct ResponseReader {
    id: Id,
    response_frame_receiver: mpsc::Receiver<ResponseFrameEvent>,
    response_error_receiver: oneshot::Receiver<ResponseError>,
    #[new(default)]
    ended: bool,
    #[new(default)]
    trailers: Option<Trailers>,
}

impl ResponseReader {
    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(response_id = ?self.id)))]
    pub async fn read_body_chunk(
        &mut self,
    ) -> Result<Option<Vec<u8>>, super::response_error::ResponseError> {
        if self.ended {
            return Ok(None);
        }

        return_if_error(&mut self.response_error_receiver).await?;

        let frame = self.response_frame_receiver.recv().await;

        return_if_error(&mut self.response_error_receiver).await?;
        if let Some(frame) = frame {
            match frame {
                ResponseFrameEvent::BodyChunk { chunk } => Ok(Some(chunk)),
                ResponseFrameEvent::End { trailers } => {
                    self.ended = true;
                    self.trailers = trailers;
                    Ok(None)
                }
            }
        } else {
            Ok(None)
        }
    }

    pub fn trailers(
        &self,
    ) -> Result<Option<&Trailers>, super::response_error::ResponseError> {
        if self.ended {
            Ok(self.trailers.as_ref())
        } else {
            Err(ResponseErrorInner::TrailersNotAvailable.into())
        }
    }
}

async fn return_if_error(
    error_rx: &mut oneshot::Receiver<ResponseError>,
) -> ResponseResult<()> {
    match error_rx.try_recv() {
        Ok(error) => {
            return Err(ResponseErrorInner::Response(error).into());
        }
        Err(e) => match e {
            oneshot::error::TryRecvError::Empty
            | oneshot::error::TryRecvError::Closed => return Ok(()),
        },
    }
}

#[derive(Debug, Clone, EnumIs, EnumDiscriminants, new)]
#[strum_discriminants(
    derive(strum::Display, Serialize_repr, Deserialize_repr),
    name(ResponseFrameEventType)
)]
#[repr(u8)]
pub(crate) enum ResponseFrameEvent {
    BodyChunk { chunk: Vec<u8> },
    End { trailers: Option<Trailers> },
}
