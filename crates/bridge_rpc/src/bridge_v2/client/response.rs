use derive_new::new;
use strum::IntoDiscriminant;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use super::super::{
    frame::{ChannelResponseFrame, ChannelResponseFrameType},
    status_code::ResponseStatusCode,
};
pub use super::response_error as error;

use error::{ResponseErrorInner, ResponseResult};

use super::super::{Headers, Trailers, frame::ResponseError, id::Id};

pub struct Response {
    id: Id,
    headers: Option<Headers>,
    status: ResponseStatusCode,
    response_frame_rx: mpsc::Receiver<ChannelResponseFrame>,
    response_error_rx: oneshot::Receiver<ResponseError>,
}

impl Response {
    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(response_id = ?id)))]
    pub async fn init(
        id: Id,
        mut response_frame_rx: mpsc::Receiver<ChannelResponseFrame>,
        mut response_error_rx: oneshot::Receiver<ResponseError>,
    ) -> ResponseResult<Response> {
        return_if_error(&mut response_error_rx).await?;

        let start_frame = match response_frame_rx.recv().await {
            Some(e) => {
                if let ChannelResponseFrame::ResponseStart(start) = e {
                    start
                } else {
                    return Err(ResponseErrorInner::new_unexpected_frame(
                        id,
                        vec![ChannelResponseFrameType::ResponseBodyStart],
                        e.discriminant(),
                    )
                    .into());
                }
            }
            None => {
                return Err(ResponseErrorInner::new_no_frame(
                    id,
                    vec![ChannelResponseFrameType::ResponseStart],
                )
                .into());
            }
        };
        return_if_error(&mut response_error_rx).await?;

        let mut headers = None;

        match response_frame_rx.recv().await {
            Some(frame) => {
                let disc = frame.discriminant();
                if let ChannelResponseFrame::ResponseHeaders(header_frame) =
                    frame
                {
                    headers = Some(header_frame.headers);
                    return_if_error(&mut response_error_rx).await?;

                    if let Some(frame) = response_frame_rx.recv().await {
                        if let ChannelResponseFrame::ResponseBodyStart(_) =
                            frame
                        {
                            return_if_error(&mut response_error_rx).await?;
                            // do nothing here
                        } else {
                            return Err(
                                ResponseErrorInner::new_unexpected_frame(
                                    id,
                                    vec![ChannelResponseFrameType::ResponseBodyStart],
                                    disc,
                                )
                                .into(),
                            );
                        }
                    } else {
                        return Err(ResponseErrorInner::new_no_frame(
                            id,
                            vec![ChannelResponseFrameType::ResponseBodyStart],
                        )
                        .into());
                    }
                } else if let ChannelResponseFrame::ResponseBodyStart(_) = frame
                {
                    return_if_error(&mut response_error_rx).await?;
                    // do nothing with data start signal
                } else {
                    return Err(ResponseErrorInner::new_unexpected_frame(
                        id,
                        vec![
                            ChannelResponseFrameType::ResponseHeaders,
                            ChannelResponseFrameType::ResponseBodyStart,
                        ],
                        disc,
                    )
                    .into());
                }
            }
            None => {
                return Err(ResponseErrorInner::new_no_frame(
                    id,
                    vec![
                        ChannelResponseFrameType::ResponseHeaders,
                        ChannelResponseFrameType::ResponseBodyStart,
                    ],
                )
                .into());
            }
        }
        return_if_error(&mut response_error_rx).await?;

        Ok(Response {
            id,
            response_frame_rx,
            response_error_rx,
            headers,
            status: start_frame.status,
        })
    }
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
                self.response_frame_rx,
                self.response_error_rx,
            ),
        )
    }

    pub fn into_reader(self) -> ResponseReader {
        ResponseReader::new(
            self.id,
            self.response_frame_rx,
            self.response_error_rx,
        )
    }
}

#[derive(new)]
pub struct ResponseReader {
    id: Id,
    #[new(default)]
    body_ended: bool,
    response_frame_rx: mpsc::Receiver<ChannelResponseFrame>,
    response_error_rx: oneshot::Receiver<ResponseError>,
}

impl ResponseReader {
    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(response_id = ?self.id)))]
    pub async fn read_body_chunk(
        &mut self,
    ) -> Result<Option<Vec<u8>>, super::response_error::ResponseError> {
        if self.body_ended {
            return Ok(None);
        }

        return_if_error(&mut self.response_error_rx).await?;

        let frame = self.response_frame_rx.recv().await;

        return_if_error(&mut self.response_error_rx).await?;
        match frame {
            Some(ChannelResponseFrame::ResponseBodyChunk(chunk)) => {
                Ok(Some(chunk.chunk))
            }
            Some(ChannelResponseFrame::ResponseBodyEnd(_)) => {
                self.body_ended = true;
                Ok(None)
            }
            None => Err(ResponseErrorInner::new_no_frame(
                self.id,
                vec![
                    ChannelResponseFrameType::ResponseBodyChunk,
                    ChannelResponseFrameType::ResponseBodyEnd,
                ],
            )
            .into()),
            Some(frame) => Err(ResponseErrorInner::new_unexpected_frame(
                self.id,
                vec![ChannelResponseFrameType::ResponseBodyChunk],
                frame.discriminant(),
            )
            .into()),
        }
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(response_id = ?self.id)))]
    pub async fn end(
        mut self,
    ) -> Result<Option<Trailers>, super::response_error::ResponseError> {
        let trailers;
        if self.body_ended {
            return_if_error(&mut self.response_error_rx).await?;

            let trailer_or_end = self.response_frame_rx.recv().await;

            match trailer_or_end {
                Some(ChannelResponseFrame::ResponseTrailers(
                    response_trailers,
                )) => {
                    trailers = Some(response_trailers.trailers);
                }
                Some(ChannelResponseFrame::ResponseEnd(_)) => {
                    return Ok(None);
                }
                Some(frame) => {
                    return Err(ResponseErrorInner::new_unexpected_frame(
                        self.id,
                        vec![
                            ChannelResponseFrameType::ResponseTrailers,
                            ChannelResponseFrameType::ResponseEnd,
                        ],
                        frame.discriminant(),
                    )
                    .into());
                }
                None => {
                    return Err(ResponseErrorInner::new_no_frame(
                        self.id,
                        vec![
                            ChannelResponseFrameType::ResponseTrailers,
                            ChannelResponseFrameType::ResponseEnd,
                        ],
                    )
                    .into());
                }
            }

            return_if_error(&mut self.response_error_rx).await?;

            let end = self.response_frame_rx.recv().await;

            return_if_error(&mut self.response_error_rx).await?;

            match end {
                Some(ChannelResponseFrame::ResponseBodyEnd(_)) => {
                    return Ok(trailers);
                }
                Some(frame) => {
                    return Err(ResponseErrorInner::new_unexpected_frame(
                        self.id,
                        vec![ChannelResponseFrameType::ResponseBodyEnd],
                        frame.discriminant(),
                    )
                    .into());
                }
                None => {
                    return Err(ResponseErrorInner::new_no_frame(
                        self.id,
                        vec![ChannelResponseFrameType::ResponseBodyEnd],
                    )
                    .into());
                }
            }
        } else {
            // consume all body
            while let Some(_) = self.read_body_chunk().await? {}

            Box::pin(self.end()).await
        }
    }
}

async fn return_if_error(
    error_rx: &mut oneshot::Receiver<ResponseError>,
) -> ResponseResult<()> {
    match error_rx.try_recv() {
        Ok(error) => {
            return Err(ResponseErrorInner::ResponseError(error).into());
        }
        Err(e) => match e {
            oneshot::error::TryRecvError::Empty
            | oneshot::error::TryRecvError::Closed => return Ok(()),
        },
    }
}
