use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant};

use crate::bridge_v2::ResponseStatusCode;

use super::{Headers, RequestErrorCode, Trailers};

use super::error_code::ResponseErrorCode;

use super::id::Id;

#[derive(Debug, Clone, EnumIs, EnumDiscriminants)]
#[strum_discriminants(
    derive(strum::Display, Serialize_repr, Deserialize_repr),
    name(FrameType)
)]
#[repr(u8)]
pub(crate) enum Frame {
    RequestStart(RequestStart) = 0,
    RequestHeaders(RequestHeaders),
    RequestBodyStart(RequestBodyStart),
    RequestBodyChunk(RequestBodyChunk),
    RequestBodyEnd(RequestBodyEnd),
    RequestTrailers(RequestTrailers),
    RequestEnd(RequestEnd),
    RequestError(RequestError),

    ResponseStart(ResponseStart) = 20,
    ResponseHeaders(ResponseHeaders),
    ResponseBodyStart(ResponseBodyStart),
    ResponseBodyChunk(ResponseBodyChunk),
    ResponseBodyEnd(ResponseBodyEnd),
    ResponseTrailers(ResponseTrailers),
    ResponseEnd(ResponseEnd),
    ResponseError(ResponseError),

    Close = 40,
    // Optional for TCP
    Ping,
    Pong,
}

impl Frame {
    pub fn to_repr(
        &self,
    ) -> Result<FrameRepr<'static, rmpv::Value>, rmpv::ext::Error> {
        let ty = self.discriminant();
        let data = match self {
            Frame::RequestStart(request_start) => {
                rmpv::ext::to_value(request_start)?
            }
            Frame::RequestHeaders(request_headers) => {
                rmpv::ext::to_value(request_headers)?
            }
            Frame::RequestBodyStart(request_data_start) => {
                rmpv::ext::to_value(request_data_start)?
            }
            Frame::RequestBodyChunk(request_data) => {
                rmpv::ext::to_value(request_data)?
            }
            Frame::RequestBodyEnd(request_data_end) => {
                rmpv::ext::to_value(request_data_end)?
            }
            Frame::RequestTrailers(request_trailers) => {
                rmpv::ext::to_value(request_trailers)?
            }
            Frame::RequestEnd(request_end) => rmpv::ext::to_value(request_end)?,
            Frame::RequestError(request_error) => {
                rmpv::ext::to_value(request_error)?
            }
            Frame::ResponseStart(response_start) => {
                rmpv::ext::to_value(response_start)?
            }
            Frame::ResponseHeaders(response_headers) => {
                rmpv::ext::to_value(response_headers)?
            }
            Frame::ResponseBodyStart(response_data_start) => {
                rmpv::ext::to_value(response_data_start)?
            }
            Frame::ResponseBodyChunk(response_data) => {
                rmpv::ext::to_value(response_data)?
            }
            Frame::ResponseBodyEnd(response_data_end) => {
                rmpv::ext::to_value(response_data_end)?
            }
            Frame::ResponseTrailers(response_trailers) => {
                rmpv::ext::to_value(response_trailers)?
            }
            Frame::ResponseEnd(response_end) => {
                rmpv::ext::to_value(response_end)?
            }
            Frame::ResponseError(response_error) => {
                rmpv::ext::to_value(response_error)?
            }
            Frame::Close => rmpv::Value::Nil,
            Frame::Ping => rmpv::Value::Nil,
            Frame::Pong => rmpv::Value::Nil,
        };

        Ok(FrameRepr {
            r#type: ty,
            data,
            _data: std::marker::PhantomData,
        })
    }

    pub fn from_repr(
        repr: FrameRepr<'static, rmpv::Value>,
    ) -> Result<Self, rmpv::ext::Error> {
        let ty = repr.r#type;
        let data = repr.data;

        Ok(match ty {
            FrameType::RequestStart => {
                let request_start: RequestStart = rmpv::ext::from_value(data)?;
                Frame::RequestStart(request_start)
            }
            FrameType::RequestHeaders => {
                let request_headers: RequestHeaders =
                    rmpv::ext::from_value(data)?;
                Frame::RequestHeaders(request_headers)
            }
            FrameType::RequestBodyStart => {
                let request_data_start: RequestBodyStart =
                    rmpv::ext::from_value(data)?;
                Frame::RequestBodyStart(request_data_start)
            }
            FrameType::RequestBodyChunk => {
                let request_data: RequestBodyChunk =
                    rmpv::ext::from_value(data)?;
                Frame::RequestBodyChunk(request_data)
            }
            FrameType::RequestBodyEnd => {
                let request_data_end: RequestBodyEnd =
                    rmpv::ext::from_value(data)?;
                Frame::RequestBodyEnd(request_data_end)
            }
            FrameType::RequestTrailers => {
                let request_trailers: RequestTrailers =
                    rmpv::ext::from_value(data)?;
                Frame::RequestTrailers(request_trailers)
            }
            FrameType::RequestEnd => {
                let request_end: RequestEnd = rmpv::ext::from_value(data)?;
                Frame::RequestEnd(request_end)
            }
            FrameType::RequestError => {
                let request_error: RequestError = rmpv::ext::from_value(data)?;
                Frame::RequestError(request_error)
            }
            FrameType::ResponseStart => {
                let response_start: ResponseStart =
                    rmpv::ext::from_value(data)?;
                Frame::ResponseStart(response_start)
            }
            FrameType::ResponseHeaders => {
                let response_headers: ResponseHeaders =
                    rmpv::ext::from_value(data)?;
                Frame::ResponseHeaders(response_headers)
            }
            FrameType::ResponseBodyStart => {
                let response_data_start: ResponseBodyStart =
                    rmpv::ext::from_value(data)?;
                Frame::ResponseBodyStart(response_data_start)
            }
            FrameType::ResponseBodyChunk => {
                let response_data: ResponseBodyChunk =
                    rmpv::ext::from_value(data)?;
                Frame::ResponseBodyChunk(response_data)
            }
            FrameType::ResponseBodyEnd => {
                let response_data_end: ResponseBodyEnd =
                    rmpv::ext::from_value(data)?;
                Frame::ResponseBodyEnd(response_data_end)
            }
            FrameType::ResponseTrailers => {
                let response_trailers: ResponseTrailers =
                    rmpv::ext::from_value(data)?;
                Frame::ResponseTrailers(response_trailers)
            }
            FrameType::ResponseEnd => {
                let response_end: ResponseEnd = rmpv::ext::from_value(data)?;
                Frame::ResponseEnd(response_end)
            }
            FrameType::ResponseError => {
                let response_error: ResponseError =
                    rmpv::ext::from_value(data)?;
                Frame::ResponseError(response_error)
            }
            FrameType::Close => Frame::Close,
            FrameType::Ping => Frame::Ping,
            FrameType::Pong => Frame::Pong,
        })
    }
}

impl Serialize for Frame {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let repr = self.to_repr().map_err(|e| {
            serde::ser::Error::custom(format!(
                "failed to serialize frame: {}",
                e
            ))
        })?;

        repr.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Frame {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let frame = FrameRepr::deserialize(deserializer)?;

        Self::from_repr(frame).map_err(|e| {
            serde::de::Error::custom(format!(
                "failed to deserialize frame: {}",
                e
            ))
        })
    }
}

/// General constructors
impl Frame {
    pub const fn request_start(id: Id, path: String) -> Self {
        Frame::RequestStart(RequestStart { id, path })
    }

    pub const fn request_headers(id: Id, headers: Headers) -> Self {
        Frame::RequestHeaders(RequestHeaders { id, headers })
    }

    pub const fn request_body_start(id: Id) -> Self {
        Frame::RequestBodyStart(RequestBodyStart { id })
    }

    pub const fn request_body_chunk(id: Id, data: Vec<u8>) -> Self {
        Frame::RequestBodyChunk(RequestBodyChunk { id, chunk: data })
    }

    pub const fn request_body_end(id: Id) -> Self {
        Frame::RequestBodyEnd(RequestBodyEnd { id })
    }

    pub const fn request_trailers(id: Id, trailers: Trailers) -> Self {
        Frame::RequestTrailers(RequestTrailers { id, trailers })
    }

    pub const fn request_end(id: Id) -> Self {
        Frame::RequestEnd(RequestEnd { id })
    }

    pub const fn request_error(
        id: Id,
        code: RequestErrorCode,
        message: String,
    ) -> Self {
        Frame::RequestError(RequestError { id, code, message })
    }

    pub const fn response_start(id: Id, status: ResponseStatusCode) -> Self {
        Frame::ResponseStart(ResponseStart { id, status })
    }

    pub const fn response_headers(id: Id, headers: Headers) -> Self {
        Frame::ResponseHeaders(ResponseHeaders { id, headers })
    }

    pub const fn response_body_start(id: Id) -> Self {
        Frame::ResponseBodyStart(ResponseBodyStart { id })
    }

    pub const fn response_body_chunk(id: Id, data: Vec<u8>) -> Self {
        Frame::ResponseBodyChunk(ResponseBodyChunk { id, chunk: data })
    }

    pub const fn response_body_end(id: Id) -> Self {
        Frame::ResponseBodyEnd(ResponseBodyEnd { id })
    }

    pub const fn response_trailers(id: Id, trailers: Trailers) -> Self {
        Frame::ResponseTrailers(ResponseTrailers { id, trailers })
    }

    pub const fn response_error(
        id: Id,
        code: ResponseErrorCode,
        message: String,
    ) -> Self {
        Frame::ResponseError(ResponseError { id, code, message })
    }

    pub const fn response_end(id: Id) -> Self {
        Frame::ResponseEnd(ResponseEnd { id })
    }

    pub const fn ping() -> Self {
        Frame::Ping
    }

    pub const fn pong() -> Self {
        Frame::Pong
    }

    pub const fn close() -> Self {
        Frame::Close
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FrameRepr<'a, TData>
where
    TData: 'a,
{
    #[serde(rename = "type")]
    pub r#type: FrameType,
    pub data: TData,
    _data: std::marker::PhantomData<&'a ()>,
}

#[derive(Debug, Clone, EnumIs, EnumDiscriminants)]
#[strum_discriminants(
    derive(strum::Display, Serialize_repr, Deserialize_repr),
    name(RequestFrameType)
)]
#[repr(u8)]
pub(crate) enum RequestFrame {
    RequestStart(RequestStart) = 0,
    RequestHeaders(RequestHeaders),
    RequestBodyStart(RequestBodyStart),
    RequestBody(RequestBodyChunk),
    RequestBodyEnd(RequestBodyEnd),
    RequestTrailers(RequestTrailers),
    RequestEnd(RequestEnd),
    RequestError(RequestError),
}

#[derive(Debug, Clone, EnumIs, EnumDiscriminants)]
#[strum_discriminants(
    derive(strum::Display, Serialize_repr, Deserialize_repr),
    name(ResponseFrameType)
)]
#[repr(u8)]
pub(crate) enum ChannelResponseFrame {
    ResponseStart(ResponseStart) = 20,
    ResponseHeaders(ResponseHeaders),
    ResponseBodyStart(ResponseBodyStart),
    ResponseBodyChunk(ResponseBodyChunk),
    ResponseBodyEnd(ResponseBodyEnd),
    ResponseTrailers(ResponseTrailers),
    ResponseEnd(ResponseEnd),
    // ResponseError(ResponseError), // Error frames are sent in a oneshot channel
}

// Request structures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RequestStart {
    pub id: Id,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RequestHeaders {
    pub id: Id,
    pub headers: Headers,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct RequestBodyStart {
    pub id: Id,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RequestBodyChunk {
    pub id: Id,
    pub chunk: Vec<u8>, // Consider bytes::Bytes for zero-copy
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct RequestBodyEnd {
    pub id: Id,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RequestTrailers {
    pub id: Id,
    pub trailers: Trailers,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RequestEnd {
    pub id: Id,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RequestError {
    pub id: Id,
    pub code: RequestErrorCode,
    pub message: String,
}

// Response structures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ResponseStart {
    pub id: Id,
    pub status: ResponseStatusCode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ResponseHeaders {
    pub id: Id,
    pub headers: Headers,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct ResponseBodyStart {
    pub id: Id,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ResponseBodyChunk {
    pub id: Id,
    pub chunk: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct ResponseBodyEnd {
    pub id: Id,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ResponseTrailers {
    pub id: Id,
    pub trailers: Trailers,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ResponseEnd {
    pub id: Id,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ResponseError {
    pub id: Id,
    pub code: ResponseErrorCode,
    pub message: String,
}
