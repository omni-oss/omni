use serde::{Deserialize, Serialize};
use strum::EnumIs;

use crate::bridge::RequestId;

#[derive(Debug, Clone, Deserialize, Serialize, EnumIs)]
#[serde(tag = "type", content = "content", rename_all = "snake_case")]
pub enum BridgeFrame<TData> {
    InternalOp(InternalOp),
    Response(BridgeResponse<TData>),
    Request(BridgeRequest<TData>),
}

pub const fn f_close() -> BridgeFrame<()> {
    BridgeFrame::<()>::InternalOp(InternalOp::Close)
}

pub fn f_req<TRequest>(
    id: RequestId,
    path: impl Into<String>,
    data: TRequest,
) -> BridgeFrame<TRequest> {
    BridgeFrame::Request(BridgeRequest {
        id,
        path: path.into(),
        data,
    })
}

pub fn f_res<TResponse>(
    id: RequestId,
    data: Option<TResponse>,
    error: Option<ErrorData>,
) -> BridgeFrame<TResponse> {
    BridgeFrame::Response(BridgeResponse { id, data, error })
}

pub fn f_res_success<TResponse>(
    id: RequestId,
    data: TResponse,
) -> BridgeFrame<TResponse> {
    f_res(id, Some(data), None)
}

pub fn f_res_error(id: RequestId, error_message: String) -> BridgeFrame<()> {
    f_res(id, None, Some(ErrorData { error_message }))
}

pub const FRAME_CLOSE: BridgeFrame<()> = f_close();

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BridgeRequest<TRequest> {
    pub id: RequestId,
    pub path: String,
    pub data: TRequest,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ErrorData {
    pub error_message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct BridgeResponse<TResponse> {
    pub id: RequestId,
    pub data: Option<TResponse>,
    pub error: Option<ErrorData>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InternalOp {
    Close,
}
