use std::sync::Arc;

use crate::frame::Frame;

use super::client_handle_error::{ClientHandleErrorInner, ClientHandleResult};
use derive_new::new;
use tokio::sync::{Mutex, mpsc};

use super::{
    super::Id,
    client::request::PendingRequest,
    contexts::{RequestSessionContext, ResponseSessionContext},
    frame_transporter::FrameTransporter,
};

type SessionManager = super::session::SessionManager<
    RequestSessionContext,
    ResponseSessionContext,
>;

#[derive(Clone, new)]
pub struct ClientHandle {
    id: Id,
    session_manager: SessionManager,
    frame_transporter: Arc<Mutex<Option<FrameTransporter>>>,
}

impl ClientHandle {
    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(rpc_id = ?self.id, request_id = ?request_id, path = path.as_ref())))]
    pub(crate) async fn request_with_id(
        &self,
        request_id: Id,
        path: impl AsRef<str>,
    ) -> ClientHandleResult<PendingRequest> {
        Ok(super::request_utils::create_request(
            request_id,
            path,
            self.clone_bytes_sender().await?,
            &self.session_manager,
        )
        .await?)
    }

    #[inline(always)]
    pub async fn request(
        &self,
        path: impl AsRef<str>,
    ) -> ClientHandleResult<PendingRequest> {
        self.request_with_id(Id::new(), path).await
    }

    async fn clone_bytes_sender(
        &self,
    ) -> ClientHandleResult<mpsc::Sender<Frame>> {
        trace::trace!("cloning bytes sender");
        let frame_transporter = self.frame_transporter.lock().await;
        if let Some(frame_transporter) = frame_transporter.as_ref() {
            Ok(frame_transporter.sender.clone())
        } else {
            Err(ClientHandleErrorInner::new_not_running().into())
        }
    }
}
