use std::{fmt::Display, sync::Arc};

use derive_new::new;
use serde::Serialize;

use crate::{
    Id, Transport,
    bridge::{
        frame::{StreamDataFrame, StreamEndFrame},
        utils::send_frame,
    },
};

#[derive(new)]
pub struct StreamHandle<TTransport: Transport> {
    id: Id,
    transport: Arc<TTransport>,
}

impl<T: Transport> Drop for StreamHandle<T> {
    fn drop(&mut self) {
        let end_frame = StreamEndFrame::stream_end_success(self.id);
        let transport = self.transport.clone();

        tokio::spawn(async move {
            send_frame(transport.as_ref(), &end_frame).await.ok();
        });
    }
}

impl<TTransport: Transport> StreamHandle<TTransport> {
    pub fn id(&self) -> Id {
        self.id
    }
}

impl<TTransport: Transport> StreamHandle<TTransport> {
    pub async fn send<TData: Serialize>(
        &self,
        data: TData,
    ) -> crate::BridgeRpcResult<()> {
        send_frame(
            self.transport.as_ref(),
            &StreamDataFrame::stream_data(self.id, data),
        )
        .await
    }

    pub async fn end(&self) -> crate::BridgeRpcResult<()> {
        send_frame(
            self.transport.as_ref(),
            &StreamEndFrame::stream_end_success(self.id),
        )
        .await
    }

    pub async fn end_with_error<TError: Display>(
        &self,
        error: TError,
    ) -> crate::BridgeRpcResult<()> {
        send_frame(
            self.transport.as_ref(),
            &StreamEndFrame::stream_end_error(self.id, error.to_string()),
        )
        .await
    }
}
