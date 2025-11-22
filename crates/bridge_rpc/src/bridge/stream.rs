use std::fmt::Display;

use derive_new::new;
use serde::Serialize;
use tokio::sync::mpsc;

use crate::{
    Id,
    bridge::{
        frame::{StreamDataFrame, StreamEndFrame},
        utils::send_frame_to_channel,
    },
};

#[derive(new)]
pub struct StreamHandle {
    id: Id,
    tx: mpsc::UnboundedSender<Vec<u8>>,
}

impl Drop for StreamHandle {
    fn drop(&mut self) {
        let tx = self.tx.clone();
        let frame = StreamEndFrame::stream_end_success(self.id);
        tokio::spawn(async move {
            let result = send_frame_to_channel(&tx, &frame).await;

            if let Err(e) = result {
                trace::error!("failed to send stream end frame: {}", e);
            }
        });
    }
}

impl StreamHandle {
    pub fn id(&self) -> Id {
        self.id
    }
}

impl StreamHandle {
    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(stream_id = ?self.id)))]
    pub async fn send<TData: Serialize>(
        &self,
        data: TData,
    ) -> crate::BridgeRpcResult<()> {
        send_frame_to_channel(
            &self.tx,
            &StreamDataFrame::stream_data(self.id, data),
        )
        .await
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(stream_id = ?self.id)))]
    pub async fn end(&self) -> crate::BridgeRpcResult<()> {
        send_frame_to_channel(
            &self.tx,
            &StreamEndFrame::stream_end_success(self.id),
        )
        .await
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(stream_id = ?self.id)))]
    pub async fn end_with_error<TError: Display>(
        &self,
        error: TError,
    ) -> crate::BridgeRpcResult<()> {
        send_frame_to_channel(
            &self.tx,
            &StreamEndFrame::stream_end_error(self.id, error.to_string()),
        )
        .await
    }
}
