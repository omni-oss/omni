use serde::Serialize;
use tokio::sync::mpsc;

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
pub async fn send_bytes_to_transport<TTransport: Transport>(
    transport: &TTransport,
    bytes: Vec<u8>,
) -> BridgeRpcResult<()> {
    transport.send(bytes.into()).await.map_err(|e| {
        BridgeRpcErrorInner::new_transport(eyre::Report::msg(e.to_string()))
    })?;
    Ok(())
}

#[inline(always)]
pub async fn send_bytes_to_channel(
    sender: &mpsc::UnboundedSender<Vec<u8>>,
    bytes: Vec<u8>,
) -> BridgeRpcResult<()> {
    sender.send(bytes).map_err(|_| {
        BridgeRpcErrorInner::new_send(eyre::eyre!(
            "failed to send frame to channel"
        ))
    })?;
    Ok(())
}

#[inline(always)]
pub async fn send_frame_to_channel<TData>(
    sender: &mpsc::UnboundedSender<Vec<u8>>,
    frame: &Frame<TData>,
) -> BridgeRpcResult<()>
where
    TData: Serialize,
{
    let bytes = serialize(frame)?;

    send_bytes_to_channel(sender, bytes).await?;

    Ok(())
}
