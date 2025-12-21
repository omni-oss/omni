use derive_new::new;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant};

use crate::bridge::ResponseStatusCode;

use super::{Headers, RequestErrorCode, Trailers};

use super::error_code::ResponseErrorCode;

use super::super::Id;

#[derive(Debug, Clone, EnumIs, EnumDiscriminants, PartialEq)]
#[strum_discriminants(
    derive(strum::Display, Serialize_repr, Deserialize_repr),
    name(FrameType)
)]
#[repr(u8)]
pub(crate) enum Frame {
    RequestStart(RequestStart) = 0,
    RequestBodyChunk(RequestBodyChunk),
    RequestEnd(RequestEnd),
    RequestError(RequestError),

    ResponseStart(ResponseStart) = 20,
    ResponseBodyChunk(ResponseBodyChunk),
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
            Frame::RequestBodyChunk(request_data) => {
                rmpv::ext::to_value(request_data)?
            }
            Frame::RequestEnd(request_end) => rmpv::ext::to_value(request_end)?,
            Frame::RequestError(request_error) => {
                rmpv::ext::to_value(request_error)?
            }
            Frame::ResponseStart(response_start) => {
                rmpv::ext::to_value(response_start)?
            }
            Frame::ResponseBodyChunk(response_data) => {
                rmpv::ext::to_value(response_data)?
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
            FrameType::RequestBodyChunk => {
                let request_data: RequestBodyChunk =
                    rmpv::ext::from_value(data)?;
                Frame::RequestBodyChunk(request_data)
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
            FrameType::ResponseBodyChunk => {
                let response_data: ResponseBodyChunk =
                    rmpv::ext::from_value(data)?;
                Frame::ResponseBodyChunk(response_data)
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

impl serde::Serialize for Frame {
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
    pub const fn request_start(
        id: Id,
        path: String,
        headers: Option<Headers>,
    ) -> Self {
        Frame::RequestStart(RequestStart { id, path, headers })
    }

    pub const fn request_body_chunk(id: Id, data: Vec<u8>) -> Self {
        Frame::RequestBodyChunk(RequestBodyChunk { id, chunk: data })
    }

    pub const fn request_end(id: Id, trailers: Option<Trailers>) -> Self {
        Frame::RequestEnd(RequestEnd { id, trailers })
    }

    pub const fn request_error(
        id: Id,
        code: RequestErrorCode,
        message: String,
    ) -> Self {
        Frame::RequestError(RequestError { id, code, message })
    }

    pub const fn response_start(
        id: Id,
        status: ResponseStatusCode,
        headers: Option<Headers>,
    ) -> Self {
        Frame::ResponseStart(ResponseStart {
            id,
            status,
            headers,
        })
    }

    pub const fn response_body_chunk(id: Id, data: Vec<u8>) -> Self {
        Frame::ResponseBodyChunk(ResponseBodyChunk { id, chunk: data })
    }

    pub const fn response_error(
        id: Id,
        code: ResponseErrorCode,
        message: String,
    ) -> Self {
        Frame::ResponseError(ResponseError { id, code, message })
    }

    pub const fn response_end(id: Id, trailers: Option<Trailers>) -> Self {
        Frame::ResponseEnd(ResponseEnd { id, trailers })
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

// Request structures
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, new)]
pub(crate) struct RequestStart {
    pub id: Id,
    pub path: String,
    pub headers: Option<Headers>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, new)]
pub(crate) struct RequestBodyChunk {
    pub id: Id,
    pub chunk: Vec<u8>, // Consider bytes::Bytes for zero-copy
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, new)]
pub(crate) struct RequestEnd {
    pub id: Id,
    pub trailers: Option<Trailers>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, new)]
pub(crate) struct RequestError {
    pub id: Id,
    pub code: RequestErrorCode,
    pub message: String,
}

// Response structures
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, new)]
pub(crate) struct ResponseStart {
    pub id: Id,
    pub status: ResponseStatusCode,
    pub headers: Option<Headers>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, new)]
pub(crate) struct ResponseBodyChunk {
    pub id: Id,
    #[serde(with = "serde_bytes")]
    pub chunk: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, new)]
pub(crate) struct ResponseEnd {
    pub id: Id,
    pub trailers: Option<Trailers>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, new)]
pub(crate) struct ResponseError {
    pub id: Id,
    pub code: ResponseErrorCode,
    pub message: String,
}
