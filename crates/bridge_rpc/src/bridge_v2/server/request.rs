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
use super::response::{PendingResponse, error::ResponseResult};

pub struct Request {
    id: Id,
    headers: Option<Headers>,
    path: String,
    response_bytes_tx: mpsc::Sender<Vec<u8>>,
    request_frame_rx: mpsc::Receiver<ChannelRequestFrame>,
    response_error_rx: oneshot::Receiver<ResponseError>,
}

impl Request {
    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(request_id = ?id)))]
    pub async fn init(
        id: Id,
        response_bytes_tx: mpsc::Sender<Vec<u8>>,
        mut request_frame_rx: mpsc::Receiver<ChannelRequestFrame>,
        mut response_error_rx: oneshot::Receiver<ResponseError>,
    ) -> RequestResult<Request> {
        return_if_error(&mut response_error_rx).await?;

        let start_frame = match request_frame_rx.recv().await {
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
        return_if_error(&mut response_error_rx).await?;

        let mut headers = None;

        match request_frame_rx.recv().await {
            Some(frame) => {
                let disc = frame.discriminant();
                if let ChannelRequestFrame::RequestHeaders(header_frame) = frame
                {
                    headers = Some(header_frame.headers);
                    return_if_error(&mut response_error_rx).await?;

                    if let Some(frame) = request_frame_rx.recv().await {
                        if let ChannelRequestFrame::RequestBodyStart(_) = frame
                        {
                            return_if_error(&mut response_error_rx).await?;
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
                    return_if_error(&mut response_error_rx).await?;
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
        return_if_error(&mut response_error_rx).await?;

        Ok(Request {
            id,
            response_bytes_tx,
            request_frame_rx,
            response_error_rx,
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

    pub fn into_parts(self) -> (String, Option<Headers>, RequestReader) {
        (
            self.path,
            self.headers,
            RequestReader::new(
                self.id,
                self.response_bytes_tx,
                self.request_frame_rx,
                self.response_error_rx,
            ),
        )
    }

    pub fn into_reader(self) -> RequestReader {
        RequestReader::new(
            self.id,
            self.response_bytes_tx,
            self.request_frame_rx,
            self.response_error_rx,
        )
    }
}

#[derive(new)]
pub struct RequestReader {
    id: Id,
    response_bytes_tx: mpsc::Sender<Vec<u8>>,
    request_frame_rx: mpsc::Receiver<ChannelRequestFrame>,
    response_error_rx: oneshot::Receiver<ResponseError>,
    #[new(default)]
    body_ended: bool,
}

impl RequestReader {
    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(request_id = ?self.id)))]
    pub async fn read_body_chunk(
        &mut self,
    ) -> Result<Option<Vec<u8>>, super::request_error::RequestError> {
        if self.body_ended {
            return Ok(None);
        }

        return_if_error(&mut self.response_error_rx).await?;

        let frame = self.request_frame_rx.recv().await;

        return_if_error(&mut self.response_error_rx).await?;
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

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(request_id = ?self.id)))]
    pub async fn end(
        mut self,
    ) -> Result<EndedRequest, super::request_error::RequestError> {
        let trailers;
        if self.body_ended {
            return_if_error(&mut self.response_error_rx).await?;

            let trailer_or_end = self.request_frame_rx.recv().await;

            match trailer_or_end {
                Some(ChannelRequestFrame::RequestTrailers(
                    request_trailers,
                )) => {
                    trailers = Some(request_trailers.trailers);
                }
                Some(ChannelRequestFrame::RequestBodyEnd(_)) => {
                    return Ok(EndedRequest::new(
                        None,
                        self.id,
                        self.response_bytes_tx,
                        self.response_error_rx,
                    ));
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

            return_if_error(&mut self.response_error_rx).await?;

            let end = self.request_frame_rx.recv().await;

            return_if_error(&mut self.response_error_rx).await?;

            match end {
                Some(ChannelRequestFrame::RequestEnd(_)) => {
                    return Ok(EndedRequest::new(
                        trailers,
                        self.id,
                        self.response_bytes_tx,
                        self.response_error_rx,
                    ));
                }
                Some(frame) => {
                    return Err(RequestErrorInner::new_unexpected_frame(
                        self.id,
                        vec![ChannelRequestFrameType::RequestEnd],
                        frame.discriminant(),
                    )
                    .into());
                }
                None => {
                    return Err(RequestErrorInner::new_no_frame(
                        self.id,
                        vec![ChannelRequestFrameType::RequestEnd],
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

#[derive(new)]
pub struct EndedRequest {
    trailers: Option<Trailers>,
    id: Id,
    response_bytes_tx: mpsc::Sender<Vec<u8>>,
    response_error_rx: oneshot::Receiver<ResponseError>,
}

impl EndedRequest {
    pub fn trailers(&self) -> Option<&Trailers> {
        self.trailers.as_ref()
    }

    pub async fn respond(self) -> ResponseResult<PendingResponse> {
        Ok(PendingResponse::new(
            self.id,
            self.response_bytes_tx,
            self.response_error_rx,
        ))
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
            oneshot::error::TryRecvError::Empty
            | oneshot::error::TryRecvError::Closed => return Ok(()),
        },
    }
}
