use serde::Serialize;
use strum::IntoDiscriminant as _;
use tokio::sync::mpsc;

use super::{BridgeRpcErrorInner, BridgeRpcResult, frame::Frame};
use crate::Transport;

pub fn serialize<T>(value: &T) -> BridgeRpcResult<Vec<u8>>
where
    T: Serialize,
{
    Ok(rmp_serde::to_vec_named(value)
        .map_err(BridgeRpcErrorInner::Serialization)?)
}

#[inline(always)]
#[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(bytes_length = ?bytes.len())))]
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
#[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(bytes_length = ?bytes.len())))]
pub async fn send_bytes_to_channel(
    sender: &mpsc::Sender<Vec<u8>>,
    bytes: Vec<u8>,
) -> BridgeRpcResult<()> {
    sender.send(bytes).await.map_err(|e| eyre::Report::new(e))?;
    Ok(())
}

#[inline(always)]
#[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(frame_type = ?frame.discriminant())))]
pub async fn send_frame_to_channel(
    sender: &mpsc::Sender<Vec<u8>>,
    frame: &Frame,
) -> BridgeRpcResult<()> {
    let bytes = serialize(frame)?;

    send_bytes_to_channel(sender, bytes).await?;

    Ok(())
}
