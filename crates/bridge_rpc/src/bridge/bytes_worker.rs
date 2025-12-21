use derive_new::new;
use tokio::{sync::mpsc, task::AbortHandle};

#[derive(new)]
pub(crate) struct BytesWorker {
    pub sender: mpsc::Sender<Vec<u8>>,
    pub abort_handle: AbortHandle,
}
