use serde::Serialize;

use crate::bridge::ResponseStatusCode;

use super::frame::Frame;
use super::utils::serialize;
use super::{super::Id, BridgeRpcResult};

#[allow(unused)]
pub fn single_data_response_frames(
    id: Id,
    data: &impl Serialize,
) -> BridgeRpcResult<[Frame; 3]> {
    let data = serialize(data)?;

    Ok([
        Frame::response_start(id, ResponseStatusCode::Success, None),
        Frame::response_body_chunk(id, data),
        Frame::response_end(id, None),
    ])
}

#[allow(unused)]
pub fn single_data_request_frames(
    id: Id,
    path: impl Into<String>,
    data: &impl Serialize,
) -> BridgeRpcResult<[Frame; 3]> {
    let data = serialize(data)?;

    Ok([
        Frame::request_start(id, path.into(), None),
        Frame::request_body_chunk(id, data),
        Frame::request_end(id, None),
    ])
}
