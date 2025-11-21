use serde::Serialize;

use crate::{
    BridgeRpcErrorInner, BridgeRpcResult, Transport, bridge::frame::Frame,
};

pub fn serialize<T>(value: &T) -> BridgeRpcResult<Vec<u8>>
where
    T: Serialize,
{
    Ok(rmp_serde::to_vec_named(value)
        .map_err(BridgeRpcErrorInner::Serialization)?)
}

#[inline(always)]
async fn send_bytes_as_frame<TTransport: Transport>(
    transport: &TTransport,
    bytes: Vec<u8>,
) -> BridgeRpcResult<()> {
    transport.send(bytes.into()).await.map_err(|e| {
        BridgeRpcErrorInner::new_transport(eyre::Report::msg(e.to_string()))
    })?;
    Ok(())
}

#[inline(always)]
pub async fn send_frame<TTransport: Transport, TData>(
    transport: &TTransport,
    frame: &Frame<TData>,
) -> BridgeRpcResult<()>
where
    TData: Serialize,
{
    let bytes = serialize(frame)?;

    send_bytes_as_frame(transport, bytes).await?;
    Ok(())
}
