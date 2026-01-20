use std::sync::Arc;

use super::client_handle_error::{
    BridgeRpcClientHandleErrorInner, BridgeRpcClientHandleResult,
};
use derive_new::new;
use tokio::sync::{Mutex, mpsc};

use super::{
    super::Id,
    bytes_worker::BytesWorker,
    client::request::PendingRequest,
    contexts::{RequestSessionContext, ResponseSessionContext},
};

type SessionManager = super::session::SessionManager<
    RequestSessionContext,
    ResponseSessionContext,
>;

#[derive(Clone, new)]
pub struct BridgeRpcClientHandle {
    id: Id,
    session_manager: SessionManager,
    bytes_worker: Arc<Mutex<Option<BytesWorker>>>,
}

impl BridgeRpcClientHandle {
    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(rpc_id = ?self.id, request_id = ?request_id, path = path.as_ref())))]
    pub(crate) async fn request_with_id(
        &self,
        request_id: Id,
        path: impl AsRef<str>,
    ) -> BridgeRpcClientHandleResult<PendingRequest> {
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
    ) -> BridgeRpcClientHandleResult<PendingRequest> {
        self.request_with_id(Id::new(), path).await
    }

    async fn clone_bytes_sender(
        &self,
    ) -> BridgeRpcClientHandleResult<mpsc::Sender<Vec<u8>>> {
        trace::trace!("cloning bytes sender");
        let bytes_worker = self.bytes_worker.lock().await;
        if let Some(bytes_worker) = bytes_worker.as_ref() {
            Ok(bytes_worker.sender.clone())
        } else {
            Err(BridgeRpcClientHandleErrorInner::new_not_running().into())
        }
    }
}
