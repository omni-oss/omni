use tokio::sync::oneshot;

use crate::mux_output_presenter::{
    StreamHandle, stream_driver_handle::StreamDriverHandle,
};

pub fn handle() -> (StreamHandle, StreamDriverHandle) {
    let (stop_signal, stop_receiver) = oneshot::channel();
    let (wait_signal, wait_receiver) = oneshot::channel();

    (
        StreamHandle::new(stop_signal, wait_receiver),
        StreamDriverHandle::new(stop_receiver, wait_signal),
    )
}
