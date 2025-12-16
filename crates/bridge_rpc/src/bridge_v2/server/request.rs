pub use super::request_error as error;

use derive_new::new;
use strum::IntoDiscriminant;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use super::super::frame::{
    ChannelRequestFrame, ChannelRequestFrameType, ResponseError,
};

use error::{RequestErrorInner, RequestResult};

use super::super::{Headers, Trailers, id::Id};

pub struct Request {
    id: Id,
    rx: mpsc::Receiver<ChannelRequestFrame>,
    error_rx: oneshot::Receiver<ResponseError>,
    headers: Option<Headers>,
    path: String,
}

impl Request {
    pub async fn init(
        id: Id,
        mut rx: mpsc::Receiver<ChannelRequestFrame>,
        mut error_rx: oneshot::Receiver<ResponseError>,
    ) -> RequestResult<Request> {
        return_if_error(&mut error_rx).await?;

        let start_frame = match rx.recv().await {
            Some(e) => {
                if let ChannelRequestFrame::RequestStart(start) = e {
                    start
                } else {
                    return Err(RequestErrorInner::new_unexpected_frame(
                        id,
                        vec![ChannelRequestFrameType::RequestBodyStart],
                        e.discriminant(),
                    )
                    .into());
                }
            }
            None => {
                return Err(RequestErrorInner::new_no_frame(
                    id,
                    vec![ChannelRequestFrameType::RequestStart],
                )
                .into());
            }
        };
        return_if_error(&mut error_rx).await?;

        let mut headers = None;

        match rx.recv().await {
            Some(frame) => {
                let disc = frame.discriminant();
                if let ChannelRequestFrame::RequestHeaders(header_frame) = frame
                {
                    headers = Some(header_frame.headers);
                    return_if_error(&mut error_rx).await?;

                    if let Some(frame) = rx.recv().await {
                        if let ChannelRequestFrame::RequestBodyStart(_) = frame
                        {
                            return_if_error(&mut error_rx).await?;
                            // do nothing here
                        } else {
                            return Err(
                                RequestErrorInner::new_unexpected_frame(
                                    id,
                                    vec![ChannelRequestFrameType::RequestBodyStart],
                                    disc,
                                )
                                .into(),
                            );
                        }
                    } else {
                        return Err(RequestErrorInner::new_no_frame(
                            id,
                            vec![ChannelRequestFrameType::RequestBodyStart],
                        )
                        .into());
                    }
                } else if let ChannelRequestFrame::RequestBodyStart(_) = frame {
                    return_if_error(&mut error_rx).await?;
                    // do nothing with data start signal
                } else {
                    return Err(RequestErrorInner::new_unexpected_frame(
                        id,
                        vec![
                            ChannelRequestFrameType::RequestHeaders,
                            ChannelRequestFrameType::RequestBodyStart,
                        ],
                        disc,
                    )
                    .into());
                }
            }
            None => {
                return Err(RequestErrorInner::new_no_frame(
                    id,
                    vec![
                        ChannelRequestFrameType::RequestHeaders,
                        ChannelRequestFrameType::RequestBodyStart,
                    ],
                )
                .into());
            }
        }
        return_if_error(&mut error_rx).await?;

        Ok(Request {
            id,
            rx,
            error_rx,
            headers,
            path: start_frame.path,
        })
    }
}

impl Request {
    pub fn headers(&self) -> Option<&Headers> {
        self.headers.as_ref()
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn parts(self) -> (String, Option<Headers>, RequestBodyStream) {
        (
            self.path,
            self.headers,
            RequestBodyStream::new(self.id, self.rx, self.error_rx),
        )
    }

    pub fn body(self) -> RequestBodyStream {
        RequestBodyStream::new(self.id, self.rx, self.error_rx)
    }
}

#[derive(new)]
pub struct RequestBodyStream {
    id: Id,
    rx: mpsc::Receiver<ChannelRequestFrame>,
    error_rx: oneshot::Receiver<ResponseError>,
    #[new(default)]
    body_ended: bool,
}

impl RequestBodyStream {
    pub async fn read(
        &mut self,
    ) -> Result<Option<Vec<u8>>, super::request_error::RequestError> {
        if self.body_ended {
            return Ok(None);
        }

        return_if_error(&mut self.error_rx).await?;

        let frame = self.rx.recv().await;

        return_if_error(&mut self.error_rx).await?;
        match frame {
            Some(ChannelRequestFrame::RequestBodyChunk(chunk)) => {
                Ok(Some(chunk.chunk))
            }
            Some(ChannelRequestFrame::RequestBodyEnd(_)) => {
                self.body_ended = true;
                Ok(None)
            }
            None => Err(RequestErrorInner::new_no_frame(
                self.id,
                vec![
                    ChannelRequestFrameType::RequestBodyChunk,
                    ChannelRequestFrameType::RequestBodyEnd,
                ],
            )
            .into()),
            Some(frame) => Err(RequestErrorInner::new_unexpected_frame(
                self.id,
                vec![ChannelRequestFrameType::RequestBodyChunk],
                frame.discriminant(),
            )
            .into()),
        }
    }

    pub async fn finalize(
        mut self,
    ) -> Result<Option<Trailers>, super::request_error::RequestError> {
        let trailers;
        if self.body_ended {
            return_if_error(&mut self.error_rx).await?;

            let trailer_or_end = self.rx.recv().await;

            match trailer_or_end {
                Some(ChannelRequestFrame::RequestTrailers(
                    request_trailers,
                )) => {
                    trailers = Some(request_trailers.trailers);
                }
                Some(ChannelRequestFrame::RequestBodyEnd(_)) => {
                    return Ok(None);
                }
                Some(frame) => {
                    return Err(RequestErrorInner::new_unexpected_frame(
                        self.id,
                        vec![
                            ChannelRequestFrameType::RequestTrailers,
                            ChannelRequestFrameType::RequestBodyEnd,
                        ],
                        frame.discriminant(),
                    )
                    .into());
                }
                None => {
                    return Err(RequestErrorInner::new_no_frame(
                        self.id,
                        vec![
                            ChannelRequestFrameType::RequestTrailers,
                            ChannelRequestFrameType::RequestBodyEnd,
                        ],
                    )
                    .into());
                }
            }

            return_if_error(&mut self.error_rx).await?;

            let end = self.rx.recv().await;

            return_if_error(&mut self.error_rx).await?;

            match end {
                Some(ChannelRequestFrame::RequestBodyEnd(_)) => {
                    return Ok(trailers);
                }
                Some(frame) => {
                    return Err(RequestErrorInner::new_unexpected_frame(
                        self.id,
                        vec![ChannelRequestFrameType::RequestBodyEnd],
                        frame.discriminant(),
                    )
                    .into());
                }
                None => {
                    return Err(RequestErrorInner::new_no_frame(
                        self.id,
                        vec![ChannelRequestFrameType::RequestBodyEnd],
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
) -> RequestResult<()> {
    match error_rx.try_recv() {
        Ok(error) => {
            return Err(RequestErrorInner::ResponseError(error).into());
        }
        Err(e) => match e {
            oneshot::error::TryRecvError::Empty => return Ok(()),
            oneshot::error::TryRecvError::Closed => return Ok(()),
        },
    }
}
