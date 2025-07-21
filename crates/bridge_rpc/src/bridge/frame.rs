use serde::{Deserialize, Serialize};

use crate::bridge::RequestId;

#[derive(Debug, Clone, Deserialize, Serialize)]
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
    request: BridgeRequest<TRequest>,
) -> BridgeFrame<TRequest> {
    BridgeFrame::Request(request)
}

pub fn f_res<TResponse>(
    response: BridgeResponse<TResponse>,
) -> BridgeFrame<TResponse> {
    BridgeFrame::Response(response)
}

pub const FRAME_CLOSE: BridgeFrame<()> = f_close();

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BridgeRequest<TRequest> {
    pub request_id: RequestId,
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
    pub request_id: RequestId,
    pub data: Option<TResponse>,
    pub error: Option<ErrorData>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InternalOp {
    Close,
}
