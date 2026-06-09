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
pub enum Frame {
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
    pub(crate) fn from_repr(
        repr: FrameRepr<'static, rmpv::Value>,
    ) -> Result<Self, rmpv::ext::Error> {
        let ty = repr.0;
        let data = repr.1;

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
        use serde::ser::SerializeSeq;
        let mut state = serializer.serialize_seq(Some(2))?;
        state.serialize_element(&self.discriminant())?;
        match self {
            Frame::RequestStart(v) => state.serialize_element(v)?,
            Frame::RequestBodyChunk(v) => state.serialize_element(v)?,
            Frame::RequestEnd(v) => state.serialize_element(v)?,
            Frame::RequestError(v) => state.serialize_element(v)?,
            Frame::ResponseStart(v) => state.serialize_element(v)?,
            Frame::ResponseBodyChunk(v) => state.serialize_element(v)?,
            Frame::ResponseEnd(v) => state.serialize_element(v)?,
            Frame::ResponseError(v) => state.serialize_element(v)?,
            Frame::Close | Frame::Ping | Frame::Pong => {
                state.serialize_element::<Option<()>>(&None)?;
            }
        }
        state.end()
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
pub(crate) struct FrameRepr<'a, TData>(
    FrameType,
    TData,
    // Consume the lifetime parameter to prevent compiler from complaining about unused lifetime in `TData`.
    #[serde(skip, default)] std::marker::PhantomData<&'a ()>,
)
where
    TData: 'a;

// Request structures
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, new)]
pub struct RequestStart {
    pub id: Id,
    pub path: String,
    pub headers: Option<Headers>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, new)]
pub struct RequestBodyChunk {
    pub id: Id,
    // `serde_bytes` is required so that `Vec<u8>` is encoded as a
    // msgpack *binary* blob (`bin8`/`bin16`/`bin32`) rather than as a
    // sequence of small integers. Peer implementations (notably the
    // TypeScript `@omni-oss/bridge-rpc-core` package) decode the chunk
    // straight into a `Uint8Array`, which only works for the binary
    // encoding.
    #[serde(with = "serde_bytes")]
    pub chunk: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, new)]
pub struct RequestEnd {
    pub id: Id,
    pub trailers: Option<Trailers>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, new)]
pub struct RequestError {
    pub id: Id,
    pub code: RequestErrorCode,
    pub message: String,
}

// Response structures
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, new)]
pub struct ResponseStart {
    pub id: Id,
    pub status: ResponseStatusCode,
    pub headers: Option<Headers>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, new)]
pub struct ResponseBodyChunk {
    pub id: Id,
    #[serde(with = "serde_bytes")]
    pub chunk: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, new)]
pub struct ResponseEnd {
    pub id: Id,
    pub trailers: Option<Trailers>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, new)]
pub struct ResponseError {
    pub id: Id,
    pub code: ResponseErrorCode,
    pub message: String,
}
