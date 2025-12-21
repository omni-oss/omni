use super::super::Id;
use super::client::request::PendingRequest;
use super::constants::RESPONSE_BUFFER_SIZE;
use super::contexts::*;
use super::session::{SessionManager, SessionManagerError};
use tokio::sync::{mpsc, oneshot};

pub async fn create_request(
    request_id: Id,
    path: impl AsRef<str>,
    bytes_sender: mpsc::Sender<Vec<u8>>,
    session_manager: &SessionManager<
        RequestSessionContext,
        ResponseSessionContext,
    >,
) -> Result<PendingRequest, SessionManagerError> {
    let (response_error_sender, response_error_receiver) = oneshot::channel();

    let (response_frame_sender, response_frame_receiver) =
        mpsc::channel(RESPONSE_BUFFER_SIZE);

    let (response_start_sender, response_start_receiver) = oneshot::channel();

    let response_session_context = ResponseSessionContext::new(
        response_start_sender,
        response_frame_sender,
        response_error_sender,
    );

    session_manager
        .start_response_session(request_id, response_session_context)
        .await?;

    Ok(PendingRequest::new(
        request_id,
        path.as_ref().to_string(),
        bytes_sender,
        response_start_receiver,
        response_frame_receiver,
        response_error_receiver,
    ))
}
