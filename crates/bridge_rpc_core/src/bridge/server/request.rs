pub use super::request_error as error;

use derive_new::new;
use serde_repr::Deserialize_repr;
use serde_repr::Serialize_repr;
use strum::EnumDiscriminants;
use strum::EnumIs;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use super::super::frame::RequestError;

use error::{RequestErrorInner, RequestResult};

use super::super::{super::Id, Headers, Trailers};

#[derive(new)]
pub struct Request {
    id: Id,
    path: String,
    headers: Option<Headers>,
    request_frame_rx: mpsc::Receiver<RequestFrameEvent>,
    request_error_rx: oneshot::Receiver<RequestError>,
}

impl Request {
    pub fn headers(&self) -> Option<&Headers> {
        self.headers.as_ref()
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn into_parts(self) -> (String, Option<Headers>, RequestReader) {
        (
            self.path,
            self.headers,
            RequestReader::new(
                self.id,
                self.request_frame_rx,
                self.request_error_rx,
            ),
        )
    }

    pub fn into_reader(self) -> RequestReader {
        RequestReader::new(
            self.id,
            self.request_frame_rx,
            self.request_error_rx,
        )
    }
}

#[derive(new)]
pub struct RequestReader {
    id: Id,
    request_frame_rx: mpsc::Receiver<RequestFrameEvent>,
    request_error_rx: oneshot::Receiver<RequestError>,
    #[new(default)]
    ended: bool,
    #[new(default)]
    trailers: Option<Trailers>,
}

impl RequestReader {
    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(request_id = ?self.id)))]
    pub async fn read_body_chunk(
        &mut self,
    ) -> Result<Option<Vec<u8>>, error::RequestError> {
        if self.ended {
            return Ok(None);
        }

        return_if_error(&mut self.request_error_rx).await?;

        let frame = self.request_frame_rx.recv().await;

        return_if_error(&mut self.request_error_rx).await?;
        if let Some(frame) = frame {
            match frame {
                RequestFrameEvent::BodyChunk { chunk } => Ok(Some(chunk)),
                RequestFrameEvent::End { trailers } => {
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
    ) -> Result<Option<&Trailers>, super::request_error::RequestError> {
        if self.ended {
            Ok(self.trailers.as_ref())
        } else {
            Err(RequestErrorInner::TrailersNotAvailable.into())
        }
    }
}

#[derive(Debug, Clone, EnumIs, EnumDiscriminants, new)]
#[strum_discriminants(
    derive(strum::Display, Serialize_repr, Deserialize_repr),
    name(RequestFrameEventType)
)]
#[repr(u8)]
pub enum RequestFrameEvent {
    BodyChunk { chunk: Vec<u8> },
    End { trailers: Option<Trailers> },
}

async fn return_if_error(
    error_rx: &mut oneshot::Receiver<RequestError>,
) -> RequestResult<()> {
    match error_rx.try_recv() {
        Ok(error) => {
            return Err(RequestErrorInner::RequestError(error).into());
        }
        Err(e) => match e {
            oneshot::error::TryRecvError::Empty
            | oneshot::error::TryRecvError::Closed => return Ok(()),
        },
    }
}
