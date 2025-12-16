use derive_new::new;
use strum::IntoDiscriminant;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use super::super::{
    frame::{ChannelResponseFrame, ResponseFrameType},
    status_code::ResponseStatusCode,
};
pub use super::response_error as error;

use error::{ResponseErrorInner, ResponseResult};

use super::super::{Headers, Trailers, frame::ResponseError, id::Id};

pub struct Response {
    id: Id,
    rx: mpsc::Receiver<ChannelResponseFrame>,
    error_rx: oneshot::Receiver<ResponseError>,
    headers: Option<Headers>,
    status: ResponseStatusCode,
}

impl Response {
    pub async fn init(
        id: Id,
        mut rx: mpsc::Receiver<ChannelResponseFrame>,
        mut error_rx: oneshot::Receiver<ResponseError>,
    ) -> ResponseResult<Response> {
        return_if_error(&mut error_rx).await?;

        let start_frame = match rx.recv().await {
            Some(e) => {
                if let ChannelResponseFrame::ResponseStart(start) = e {
                    start
                } else {
                    return Err(ResponseErrorInner::new_unexpected_frame(
                        id,
                        vec![ResponseFrameType::ResponseBodyStart],
                        e.discriminant(),
                    )
                    .into());
                }
            }
            None => {
                return Err(ResponseErrorInner::new_no_frame(
                    id,
                    vec![ResponseFrameType::ResponseStart],
                )
                .into());
            }
        };
        return_if_error(&mut error_rx).await?;

        let mut headers = None;

        match rx.recv().await {
            Some(frame) => {
                let disc = frame.discriminant();
                if let ChannelResponseFrame::ResponseHeaders(header_frame) =
                    frame
                {
                    headers = Some(header_frame.headers);
                    return_if_error(&mut error_rx).await?;

                    if let Some(frame) = rx.recv().await {
                        if let ChannelResponseFrame::ResponseBodyStart(_) =
                            frame
                        {
                            return_if_error(&mut error_rx).await?;
                            // do nothing here
                        } else {
                            return Err(
                                ResponseErrorInner::new_unexpected_frame(
                                    id,
                                    vec![ResponseFrameType::ResponseBodyStart],
                                    disc,
                                )
                                .into(),
                            );
                        }
                    } else {
                        return Err(ResponseErrorInner::new_no_frame(
                            id,
                            vec![ResponseFrameType::ResponseBodyStart],
                        )
                        .into());
                    }
                } else if let ChannelResponseFrame::ResponseBodyStart(_) = frame
                {
                    return_if_error(&mut error_rx).await?;
                    // do nothing with data start signal
                } else {
                    return Err(ResponseErrorInner::new_unexpected_frame(
                        id,
                        vec![
                            ResponseFrameType::ResponseHeaders,
                            ResponseFrameType::ResponseBodyStart,
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
                        ResponseFrameType::ResponseHeaders,
                        ResponseFrameType::ResponseBodyStart,
                    ],
                )
                .into());
            }
        }
        return_if_error(&mut error_rx).await?;

        Ok(Response {
            id,
            rx,
            error_rx,
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

    pub fn parts(
        self,
    ) -> (ResponseStatusCode, Option<Headers>, ResponseBodyStream) {
        (
            self.status,
            self.headers,
            ResponseBodyStream::new(self.id, self.rx, self.error_rx),
        )
    }

    pub fn body(self) -> ResponseBodyStream {
        ResponseBodyStream::new(self.id, self.rx, self.error_rx)
    }
}

#[derive(new)]
pub struct ResponseBodyStream {
    id: Id,
    rx: mpsc::Receiver<ChannelResponseFrame>,
    error_rx: oneshot::Receiver<ResponseError>,
    #[new(default)]
    body_ended: bool,
}

impl ResponseBodyStream {
    pub async fn read(
        &mut self,
    ) -> Result<Option<Vec<u8>>, super::response_error::ResponseError> {
        if self.body_ended {
            return Ok(None);
        }

        return_if_error(&mut self.error_rx).await?;

        let frame = self.rx.recv().await;

        return_if_error(&mut self.error_rx).await?;
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
                    ResponseFrameType::ResponseBodyChunk,
                    ResponseFrameType::ResponseBodyEnd,
                ],
            )
            .into()),
            Some(frame) => Err(ResponseErrorInner::new_unexpected_frame(
                self.id,
                vec![ResponseFrameType::ResponseBodyChunk],
                frame.discriminant(),
            )
            .into()),
        }
    }

    pub async fn finalize(
        mut self,
    ) -> Result<Option<Trailers>, super::response_error::ResponseError> {
        let trailers;
        if self.body_ended {
            return_if_error(&mut self.error_rx).await?;

            let trailer_or_end = self.rx.recv().await;

            match trailer_or_end {
                Some(ChannelResponseFrame::ResponseTrailers(
                    response_trailers,
                )) => {
                    trailers = Some(response_trailers.trailers);
                }
                Some(ChannelResponseFrame::ResponseBodyEnd(_)) => {
                    return Ok(None);
                }
                Some(frame) => {
                    return Err(ResponseErrorInner::new_unexpected_frame(
                        self.id,
                        vec![
                            ResponseFrameType::ResponseTrailers,
                            ResponseFrameType::ResponseBodyEnd,
                        ],
                        frame.discriminant(),
                    )
                    .into());
                }
                None => {
                    return Err(ResponseErrorInner::new_no_frame(
                        self.id,
                        vec![
                            ResponseFrameType::ResponseTrailers,
                            ResponseFrameType::ResponseBodyEnd,
                        ],
                    )
                    .into());
                }
            }

            return_if_error(&mut self.error_rx).await?;

            let end = self.rx.recv().await;

            return_if_error(&mut self.error_rx).await?;

            match end {
                Some(ChannelResponseFrame::ResponseBodyEnd(_)) => {
                    return Ok(trailers);
                }
                Some(frame) => {
                    return Err(ResponseErrorInner::new_unexpected_frame(
                        self.id,
                        vec![ResponseFrameType::ResponseBodyEnd],
                        frame.discriminant(),
                    )
                    .into());
                }
                None => {
                    return Err(ResponseErrorInner::new_no_frame(
                        self.id,
                        vec![ResponseFrameType::ResponseBodyEnd],
                    )
                    .into());
                }
            }
        } else {
            // consume all body
            while let Some(_) = self.read().await? {}

            Box::pin(self.finalize()).await
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
            oneshot::error::TryRecvError::Empty => return Ok(()),
            oneshot::error::TryRecvError::Closed => return Ok(()),
        },
    }
}
