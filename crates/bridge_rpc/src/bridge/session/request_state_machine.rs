use super::super::{super::Id, Headers, Trailers, frame::*};
use derive_new::new;
use serde_repr::{Deserialize_repr, Serialize_repr};
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant as _};

#[derive(Debug, Default)]
pub(crate) struct RequestStateMachine {
    id: Option<Id>,
    state: RequestState,
}

impl RequestStateMachine {
    pub fn new() -> Self {
        Self {
            id: None,
            state: RequestState::Initial,
        }
    }
}

impl RequestStateMachine {
    pub fn transition(
        &mut self,
        event: RequestEvent,
    ) -> Result<RequestStateTransitionOutput, RequestStateMachineError> {
        if let Some(id) = self.id
            && id != event.id()
        {
            return Err(RequestStateMachineErrorInner::new_invalid_id(
                id,
                event.id(),
            )
            .into());
        }

        match &self.state {
            RequestState::Initial => match event {
                RequestEvent::Start(request_start) => {
                    self.state = RequestState::Started;
                    self.id = Some(request_start.id);
                    Ok(RequestStateTransitionOutput::new_start(
                        request_start.id,
                        request_start.path,
                        request_start.headers,
                    ))
                }
                _ => Err(RequestStateMachineErrorInner::new_invalid_frame(
                    vec![RequestEventType::Start],
                    event.discriminant(),
                )
                .into()),
            },
            RequestState::Started | RequestState::BodyChunksReceiving => {
                match event {
                    RequestEvent::BodyChunk(request_body_chunk) => {
                        if self.state == RequestState::Started {
                            self.state = RequestState::BodyChunksReceiving;
                        }
                        Ok(RequestStateTransitionOutput::new_body_chunk(
                            request_body_chunk.chunk,
                        ))
                    }
                    RequestEvent::Error(error) => {
                        self.state = RequestState::Errored;

                        Ok(RequestStateTransitionOutput::new_error(error))
                    }
                    RequestEvent::End(request_end) => {
                        self.state = RequestState::Ended;
                        Ok(RequestStateTransitionOutput::new_end(
                            request_end.trailers,
                        ))
                    }
                    _ => Err(RequestStateMachineErrorInner::new_invalid_frame(
                        vec![RequestEventType::BodyChunk],
                        event.discriminant(),
                    )
                    .into()),
                }
            }
            RequestState::Ended => {
                Err(RequestStateMachineErrorInner::Ended.into())
            }
            RequestState::Errored => {
                Err(RequestStateMachineErrorInner::Errored.into())
            }
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
enum RequestState {
    #[default]
    Initial,
    Started,
    BodyChunksReceiving,
    Ended,
    Errored,
}

#[derive(Debug, new, PartialEq)]
pub enum RequestStateTransitionOutput {
    #[allow(unused)]
    Wait,
    Start {
        id: Id,
        path: String,
        headers: Option<Headers>,
    },
    BodyChunk {
        chunk: Vec<u8>,
    },
    End {
        trailers: Option<Trailers>,
    },
    Error {
        error: RequestError,
    },
}

#[derive(Debug, Clone, EnumIs, EnumDiscriminants, PartialEq)]
#[strum_discriminants(
    derive(strum::Display, Serialize_repr, Deserialize_repr),
    name(RequestEventType)
)]
#[repr(u8)]
pub(crate) enum RequestEvent {
    Start(RequestStart) = 0, // only create a request state machine with start frame
    BodyChunk(RequestBodyChunk),
    End(RequestEnd),
    Error(RequestError),
}

impl RequestEvent {
    pub fn id(&self) -> Id {
        match self {
            RequestEvent::Start(request_start) => request_start.id,
            RequestEvent::BodyChunk(request_body_chunk) => {
                request_body_chunk.id
            }
            RequestEvent::End(request_end) => request_end.id,
            RequestEvent::Error(request_error) => request_error.id,
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct RequestStateMachineError(pub(crate) RequestStateMachineErrorInner);

impl RequestStateMachineError {
    #[allow(unused)]
    pub fn kind(&self) -> RequestStateMachineErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<RequestStateMachineErrorInner>> From<T>
    for RequestStateMachineError
{
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(name(RequestStateMachineErrorKind), vis(pub))]
pub(crate) enum RequestStateMachineErrorInner {
    #[error("invalid frame type: expected: {expected:?}, actual: {actual}")]
    InvalidFrame {
        expected: Vec<RequestEventType>,
        actual: RequestEventType,
    },

    #[error("invalid id: expected: {expected:?}, actual: {actual}")]
    InvalidId { expected: Id, actual: Id },

    #[error("cannot transition after received error frame")]
    Errored,

    #[error("cannot transition after received end frame")]
    Ended,
}

#[cfg(test)]
mod tests {
    use super::super::super::*;
    use super::*;

    fn new() -> (Id, RequestStateMachine) {
        let id = Id::new();
        (id, RequestStateMachine::new())
    }

    fn start_event(
        id: Id,
        path: String,
        headers: Option<Headers>,
    ) -> RequestEvent {
        RequestEvent::Start(RequestStart { id, path, headers })
    }

    fn body_chunk_event(id: Id, chunk: Vec<u8>) -> RequestEvent {
        RequestEvent::BodyChunk(RequestBodyChunk { id, chunk })
    }

    fn end_event(id: Id, trailers: Option<Trailers>) -> RequestEvent {
        RequestEvent::End(RequestEnd { id, trailers })
    }

    #[test]
    fn test_request_state_machine_normal_path() {
        let (id, mut request_state_machine) = new();

        assert_eq!(
            request_state_machine
                .transition(start_event(id, "/".to_string(), None))
                .expect("should be able to transition"),
            RequestStateTransitionOutput::new_start(id, "/".to_string(), None),
            "should be able to transition"
        );

        assert_eq!(
            request_state_machine
                .transition(body_chunk_event(id, vec![1, 2, 3]))
                .expect("should be able to transition"),
            RequestStateTransitionOutput::new_body_chunk(vec![1, 2, 3])
        );

        assert_eq!(
            request_state_machine
                .transition(body_chunk_event(id, vec![4, 5, 6]))
                .expect("should be able to transition"),
            RequestStateTransitionOutput::new_body_chunk(vec![4, 5, 6])
        );

        let end_event = end_event(id, None);
        assert_eq!(
            request_state_machine
                .transition(end_event.clone())
                .expect("should be able to transition"),
            RequestStateTransitionOutput::new_end(None)
        );

        assert!(
            matches!(
                request_state_machine.transition(end_event),
                Err(RequestStateMachineError(
                    RequestStateMachineErrorInner::Ended
                ))
            ),
            "should not be able to transition after received end frame"
        );
    }

    #[test]
    fn test_request_state_machine_no_body() {
        let (id, mut request_state_machine) = new();

        assert_eq!(
            request_state_machine
                .transition(start_event(id, "/".to_string(), None))
                .expect("should be able to transition"),
            RequestStateTransitionOutput::new_start(id, "/".to_string(), None),
            "should be able to transition"
        );

        assert_eq!(
            request_state_machine
                .transition(end_event(id, None))
                .expect("should be able to transition"),
            RequestStateTransitionOutput::new_end(None)
        );

        assert!(
            matches!(
                request_state_machine
                    .transition(body_chunk_event(id, vec![1, 2, 3])),
                Err(RequestStateMachineError(
                    RequestStateMachineErrorInner::Ended
                ))
            ),
            "should not be able to transition after received end frame"
        );
    }

    #[test]
    fn test_request_state_machine_error_path() {
        let (id, mut request_state_machine) = new();

        assert_eq!(
            request_state_machine
                .transition(start_event(id, "/".to_string(), None))
                .expect("should be able to transition"),
            RequestStateTransitionOutput::new_start(id, "/".to_string(), None),
            "should be able to transition"
        );

        assert_eq!(
            request_state_machine
                .transition(body_chunk_event(id, vec![1, 2, 3]))
                .expect("should be able to transition"),
            RequestStateTransitionOutput::new_body_chunk(vec![1, 2, 3])
        );
        let error = RequestError {
            id,
            code: RequestErrorCode::TIMED_OUT,
            message: "error".to_string(),
        };
        let error_event = RequestEvent::Error(error.clone());
        assert_eq!(
            request_state_machine
                .transition(error_event.clone())
                .expect("should be able to transition"),
            RequestStateTransitionOutput::new_error(error)
        );

        let end_event = RequestEvent::End(RequestEnd { id, trailers: None });

        assert!(
            matches!(
                request_state_machine.transition(end_event),
                Err(RequestStateMachineError(
                    RequestStateMachineErrorInner::Errored
                ))
            ),
            "should not be able to transition after received error frame"
        );
    }

    #[test]
    fn test_request_state_machine_not_matching_id() {
        let (id, mut request_state_machine) = new();

        let incorrect_id = Id::new();

        assert_eq!(
            request_state_machine
                .transition(start_event(id, "/".to_string(), None))
                .expect("should be able to transition"),
            RequestStateTransitionOutput::new_start(id, "/".to_string(), None),
            "should be able to transition"
        );

        assert!(
            matches!(
                request_state_machine
                    .transition(body_chunk_event(incorrect_id, vec![1, 2, 3])),
                Err(RequestStateMachineError(
                    RequestStateMachineErrorInner::InvalidId {
                        expected: _,
                        actual: _
                    }
                ))
            ),
            "should not be able to transition after received error frame"
        );

        assert_eq!(
            request_state_machine
                .transition(body_chunk_event(id, vec![1, 2, 3]))
                .expect("should be able to transition"),
            RequestStateTransitionOutput::new_body_chunk(vec![1, 2, 3]),
            "should not be corrupted after receiving a frame with invalid id"
        );
    }
}
