use std::sync::Arc;

use tokio::sync::{Mutex, mpsc, oneshot};

use super::{
    client::response::ResponseFrameEvent,
    frame::{RequestError, ResponseError, ResponseStart},
    server::request::RequestFrameEvent,
};

pub(crate) type ResponseFrameSender = mpsc::Sender<ResponseFrameEvent>;

pub(crate) type ResponseStartSender = oneshot::Sender<ResponseStart>;

pub(crate) type ResponseErrorSender = oneshot::Sender<ResponseError>;

#[derive(Clone)]
pub(crate) struct ResponseSessionContext {
    pub response_start_sender: Arc<Mutex<Option<ResponseStartSender>>>,
    pub response_frame_sender: ResponseFrameSender,
    pub response_error_sender: Arc<Mutex<Option<ResponseErrorSender>>>,
}

impl ResponseSessionContext {
    pub(crate) fn new(
        response_start_sender: ResponseStartSender,
        response_frame_sender: ResponseFrameSender,
        response_error_sender: ResponseErrorSender,
    ) -> Self {
        Self {
            response_start_sender: Arc::new(Mutex::new(Some(
                response_start_sender,
            ))),
            response_frame_sender,
            response_error_sender: Arc::new(Mutex::new(Some(
                response_error_sender,
            ))),
        }
    }
}

pub(crate) type RequestFrameReceiver = mpsc::Receiver<RequestFrameEvent>;
pub(crate) type RequestFrameSender = mpsc::Sender<RequestFrameEvent>;

pub(crate) type RequestErrorReceiver = oneshot::Receiver<RequestError>;
pub(crate) type RequestErrorSender = oneshot::Sender<RequestError>;

#[derive(Clone)]
pub(crate) struct RequestSessionContext {
    pub request_frame_sender: RequestFrameSender,
    pub request_error_sender: Arc<Mutex<Option<RequestErrorSender>>>,
}

impl RequestSessionContext {
    pub(crate) fn new(
        request_frame_sender: RequestFrameSender,
        request_error_sender: RequestErrorSender,
    ) -> Self {
        Self {
            request_frame_sender,
            request_error_sender: Arc::new(Mutex::new(Some(
                request_error_sender,
            ))),
        }
    }
}
