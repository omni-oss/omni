use derive_new::new;
use strum::IntoDiscriminant as _;
use tokio::{sync::mpsc, task::AbortHandle};

use crate::{BridgeRpcErrorInner, BridgeRpcResult, frame::Frame};

#[derive(new)]
pub(crate) struct FrameTransporter {
    pub sender: mpsc::Sender<Frame>,
    pub abort_handle: AbortHandle,
}

impl Drop for FrameTransporter {
    fn drop(&mut self) {
        self.abort_handle.abort();
    }
}

impl FrameTransporter {
    pub async fn transport(&self, frame: Frame) -> BridgeRpcResult<()> {
        let discriminante = frame.discriminant();
        self.sender.send(frame).await.map_err(|_| {
            BridgeRpcErrorInner::new_send(eyre::eyre!(
                "failed to transport frame of type: {}",
                discriminante
            ))
        })?;

        Ok(())
    }
}
