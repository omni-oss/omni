use crate::bridge::ResponseStatusCode;

use super::super::{super::Id, Headers, Trailers, frame::*};
use derive_new::new;
use serde_repr::{Deserialize_repr, Serialize_repr};
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant as _};

#[derive(Debug)]
pub(crate) struct ResponseStateMachine {
    id: Option<Id>,
    state: ResponseState,
}

impl ResponseStateMachine {
    pub fn new() -> Self {
        Self {
            id: None,
            state: ResponseState::Initial,
        }
    }
}

impl ResponseStateMachine {
    pub fn transition(
        &mut self,
        event: ResponseEvent,
    ) -> Result<ResponseStateTransitionOutput, ResponseStateMachineError> {
        if let Some(id) = self.id
            && id != event.id()
        {
            return Err(ResponseStateMachineErrorInner::new_invalid_id(
                id,
                event.id(),
            )
            .into());
        }

        match &self.state {
            ResponseState::Initial => match event {
                ResponseEvent::Start(response_start) => {
                    self.state = ResponseState::Started;
                    self.id = Some(response_start.id);
                    Ok(ResponseStateTransitionOutput::new_start(
                        response_start.id,
                        response_start.status,
                        response_start.headers,
                    ))
                }
                _ => Err(ResponseStateMachineErrorInner::new_invalid_frame(
                    vec![ResponseEventType::Start],
                    event.discriminant(),
                )
                .into()),
            },
            ResponseState::Started | ResponseState::BodyChunksReceiving => {
                match event {
                    ResponseEvent::BodyChunk(response_body_chunk) => {
                        if self.state == ResponseState::Started {
                            self.state = ResponseState::BodyChunksReceiving;
                        }
                        Ok(ResponseStateTransitionOutput::new_body_chunk(
                            response_body_chunk.chunk,
                        ))
                    }
                    ResponseEvent::Error(error) => {
                        self.state = ResponseState::Errored;

                        Ok(ResponseStateTransitionOutput::new_error(error))
                    }
                    ResponseEvent::End(response_end) => {
                        self.state = ResponseState::Ended;
                        Ok(ResponseStateTransitionOutput::new_end(
                            response_end.trailers,
                        ))
                    }
                    _ => {
                        Err(ResponseStateMachineErrorInner::new_invalid_frame(
                            vec![ResponseEventType::Start],
                            event.discriminant(),
                        )
                        .into())
                    }
                }
            }
            ResponseState::Ended => {
                Err(ResponseStateMachineErrorInner::Ended.into())
            }
            ResponseState::Errored => {
                Err(ResponseStateMachineErrorInner::Errored.into())
            }
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
enum ResponseState {
    #[default]
    Initial,
    Started,
    BodyChunksReceiving,
    Ended,
    Errored,
}

#[derive(Debug, new, PartialEq)]
pub enum ResponseStateTransitionOutput {
    #[allow(unused)]
    Wait,
    Start {
        id: Id,
        status: ResponseStatusCode,
        headers: Option<Headers>,
    },
    BodyChunk {
        chunk: Vec<u8>,
    },
    End {
        trailers: Option<Trailers>,
    },
    Error {
        error: ResponseError,
    },
}

#[derive(Debug, Clone, EnumIs, EnumDiscriminants, PartialEq)]
#[strum_discriminants(
    derive(strum::Display, Serialize_repr, Deserialize_repr),
    name(ResponseEventType)
)]
#[repr(u8)]
pub(crate) enum ResponseEvent {
    Start(ResponseStart) = 0, // only create a Response state machine with start frame
    BodyChunk(ResponseBodyChunk),
    End(ResponseEnd),
    Error(ResponseError),
}

impl ResponseEvent {
    pub fn id(&self) -> Id {
        match self {
            ResponseEvent::Start(response_start) => response_start.id,
            ResponseEvent::BodyChunk(response_body_chunk) => {
                response_body_chunk.id
            }
            ResponseEvent::End(response_end) => response_end.id,
            ResponseEvent::Error(response_error) => response_error.id,
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ResponseStateMachineError(pub(crate) ResponseStateMachineErrorInner);

impl ResponseStateMachineError {
    #[allow(unused)]
    pub fn kind(&self) -> ResponseStateMachineErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ResponseStateMachineErrorInner>> From<T>
    for ResponseStateMachineError
{
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(name(ResponseStateMachineErrorKind), vis(pub))]
pub(crate) enum ResponseStateMachineErrorInner {
    #[error("invalid frame type: expected: {expected:?}, actual: {actual}")]
    InvalidFrame {
        expected: Vec<ResponseEventType>,
        actual: ResponseEventType,
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

    fn new() -> (Id, ResponseStateMachine) {
        let id = Id::new();
        (id, ResponseStateMachine::new())
    }

    fn start_event(
        id: Id,
        status: ResponseStatusCode,
        headers: Option<Headers>,
    ) -> ResponseEvent {
        ResponseEvent::Start(ResponseStart {
            id,
            status,
            headers,
        })
    }

    fn body_chunk_event(id: Id, chunk: Vec<u8>) -> ResponseEvent {
        ResponseEvent::BodyChunk(ResponseBodyChunk { id, chunk })
    }

    fn end_event(id: Id, trailers: Option<Trailers>) -> ResponseEvent {
        ResponseEvent::End(ResponseEnd { id, trailers })
    }

    #[test]
    fn test_response_state_machine_normal_path() {
        let (id, mut response_state_machine) = new();

        assert_eq!(
            response_state_machine
                .transition(start_event(id, ResponseStatusCode::SUCCESS, None))
                .expect("should be able to transition"),
            ResponseStateTransitionOutput::new_start(
                id,
                ResponseStatusCode::SUCCESS,
                None
            ),
            "should be able to transition"
        );

        assert_eq!(
            response_state_machine
                .transition(body_chunk_event(id, vec![1, 2, 3]))
                .expect("should be able to transition"),
            ResponseStateTransitionOutput::new_body_chunk(vec![1, 2, 3])
        );

        assert_eq!(
            response_state_machine
                .transition(body_chunk_event(id, vec![4, 5, 6]))
                .expect("should be able to transition"),
            ResponseStateTransitionOutput::new_body_chunk(vec![4, 5, 6])
        );

        let end_event = end_event(id, None);
        assert_eq!(
            response_state_machine
                .transition(end_event.clone())
                .expect("should be able to transition"),
            ResponseStateTransitionOutput::new_end(None)
        );

        assert!(
            matches!(
                response_state_machine.transition(end_event),
                Err(ResponseStateMachineError(
                    ResponseStateMachineErrorInner::Ended
                ))
            ),
            "should not be able to transition after received end frame"
        );
    }

    #[test]
    fn test_response_state_machine_no_body() {
        let (id, mut response_state_machine) = new();

        assert_eq!(
            response_state_machine
                .transition(start_event(id, ResponseStatusCode::SUCCESS, None))
                .expect("should be able to transition"),
            ResponseStateTransitionOutput::new_start(
                id,
                ResponseStatusCode::SUCCESS,
                None
            ),
            "should be able to transition"
        );

        assert_eq!(
            response_state_machine
                .transition(end_event(id, None))
                .expect("should be able to transition"),
            ResponseStateTransitionOutput::new_end(None)
        );

        assert!(
            matches!(
                response_state_machine
                    .transition(body_chunk_event(id, vec![1, 2, 3])),
                Err(ResponseStateMachineError(
                    ResponseStateMachineErrorInner::Ended
                ))
            ),
            "should not be able to transition after received end frame"
        );
    }

    #[test]
    fn test_response_state_machine_error_path() {
        let (id, mut response_state_machine) = new();

        assert_eq!(
            response_state_machine
                .transition(start_event(id, ResponseStatusCode::SUCCESS, None))
                .expect("should be able to transition"),
            ResponseStateTransitionOutput::new_start(
                id,
                ResponseStatusCode::SUCCESS,
                None
            ),
            "should be able to transition"
        );

        assert_eq!(
            response_state_machine
                .transition(body_chunk_event(id, vec![1, 2, 3]))
                .expect("should be able to transition"),
            ResponseStateTransitionOutput::new_body_chunk(vec![1, 2, 3])
        );
        let error = ResponseError {
            id,
            code: ResponseErrorCode::UnexpectedFrame,
            message: "error".to_string(),
        };
        let error_event = ResponseEvent::Error(error.clone());
        assert_eq!(
            response_state_machine
                .transition(error_event.clone())
                .expect("should be able to transition"),
            ResponseStateTransitionOutput::new_error(error)
        );

        let end_event = ResponseEvent::End(ResponseEnd { id, trailers: None });

        assert!(
            matches!(
                response_state_machine.transition(end_event),
                Err(ResponseStateMachineError(
                    ResponseStateMachineErrorInner::Errored
                ))
            ),
            "should not be able to transition after received error frame"
        );
    }

    #[test]
    fn test_response_state_machine_not_matching_id() {
        let (id, mut request_state_machine) = new();

        let incorrect_id = Id::new();

        assert_eq!(
            request_state_machine
                .transition(start_event(id, ResponseStatusCode::SUCCESS, None))
                .expect("should be able to transition"),
            ResponseStateTransitionOutput::new_start(
                id,
                ResponseStatusCode::SUCCESS,
                None
            ),
            "should be able to transition"
        );

        assert!(
            matches!(
                request_state_machine
                    .transition(body_chunk_event(incorrect_id, vec![1, 2, 3])),
                Err(ResponseStateMachineError(
                    ResponseStateMachineErrorInner::InvalidId {
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
            ResponseStateTransitionOutput::new_body_chunk(vec![1, 2, 3]),
            "should not be corrupted after receiving a frame with invalid id"
        );
    }
}
