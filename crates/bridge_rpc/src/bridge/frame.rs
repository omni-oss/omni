use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use strum::EnumIs;

use crate::bridge::Id;

#[repr(u8)]
#[derive(
    Debug,
    Clone,
    Copy,
    Deserialize_repr,
    Serialize_repr,
    EnumIs,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    strum::FromRepr,
    strum::Display,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FrameType {
    Close = 0,
    CloseAck = 1,
    Probe = 2,
    ProbeAck = 3,
    StreamStart = 4,
    StreamData = 5,
    StreamEnd = 6,
    MessageRequest = 8,
    MessageResponse = 7,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct Frame<D> {
    #[serde(rename = "type")]
    pub r#type: FrameType,

    #[serde(
        skip_serializing_if = "Option::is_none",
        default = "Option::default"
    )]
    pub data: Option<D>,
}

pub type StreamStartFrame<D> = Frame<StreamStart<D>>;
pub type StreamDataFrame<D> = Frame<StreamData<D>>;
pub type StreamEndFrame = Frame<StreamEnd>;
pub type RequestFrame<D> = Frame<Request<D>>;
pub type ResponseFrame<D> = Frame<Response<D>>;

impl<D> Frame<D> {
    pub const fn new(r#type: FrameType, data: Option<D>) -> Self {
        Self { r#type, data }
    }
}

impl Frame<()> {
    pub const fn close() -> Self {
        Self::new(FrameType::Close, None)
    }

    pub const fn close_ack() -> Self {
        Self::new(FrameType::CloseAck, None)
    }

    pub const fn probe() -> Self {
        Self::new(FrameType::Probe, None)
    }

    pub const fn probe_ack() -> Self {
        Self::new(FrameType::ProbeAck, None)
    }
}

impl<TData> StreamStartFrame<TData> {
    pub fn stream_start(
        id: Id,
        path: impl Into<String>,
        data: Option<TData>,
    ) -> Self {
        Self::new(
            FrameType::StreamStart,
            Some(StreamStart {
                id,
                path: path.into(),
                data,
            }),
        )
    }
}

impl<D> StreamDataFrame<D> {
    pub fn stream_data(id: Id, data: D) -> Self {
        Self::new(FrameType::StreamData, Some(StreamData { id, data }))
    }
}

impl StreamEndFrame {
    pub fn stream_end(id: Id, error: Option<String>) -> Self {
        Self::new(
            FrameType::StreamEnd,
            Some(StreamEnd {
                id,
                error: error.map(|e| ErrorData { message: e }),
            }),
        )
    }

    pub fn stream_end_success(id: Id) -> Self {
        Self::stream_end(id, None)
    }

    pub fn stream_end_error(id: Id, error: impl Into<String>) -> Self {
        Self::stream_end(id, Some(error.into()))
    }
}

impl<D> RequestFrame<D> {
    pub fn request(id: Id, path: impl Into<String>, data: D) -> Self {
        Self::new(
            FrameType::MessageRequest,
            Some(Request {
                id,
                path: path.into(),
                data,
            }),
        )
    }
}

impl<D> ResponseFrame<D> {
    pub fn response(id: Id, data: Option<D>, error: Option<String>) -> Self {
        Self::new(
            FrameType::MessageResponse,
            Some(Response {
                id,
                data,
                error: error.map(|e| ErrorData { message: e }),
            }),
        )
    }
}

impl<D> Frame<Response<D>> {
    pub fn success_response(id: Id, data: D) -> Self {
        Self::response(id, Some(data), None)
    }
}

impl Frame<Response<()>> {
    pub fn error_response(id: Id, error: impl Into<String>) -> Self {
        Self::response(id, None, Some(error.into()))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct StreamStart<TData> {
    pub id: Id,
    pub path: String,
    pub data: Option<TData>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct StreamData<TData> {
    pub id: Id,
    pub data: TData,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct StreamEnd {
    pub id: Id,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default = "Option::default"
    )]
    pub error: Option<ErrorData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct Request<TData> {
    pub id: Id,
    pub path: String,
    pub data: TData,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct ErrorData {
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct Response<TResponse> {
    pub id: Id,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default = "Option::default"
    )]
    pub data: Option<TResponse>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default = "Option::default"
    )]
    pub error: Option<ErrorData>,
}
