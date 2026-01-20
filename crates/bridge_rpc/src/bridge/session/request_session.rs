use super::{super::super::Id, RequestStateMachine};

pub(crate) struct RequestSession<TContext = ()> {
    id: Id,
    state_machine: RequestStateMachine,
    context: TContext,
}

impl<TContext> RequestSession<TContext> {
    pub fn new(id: Id, context: TContext) -> Self {
        Self {
            id,
            state_machine: RequestStateMachine::new(),
            context,
        }
    }
}

impl<TContext> RequestSession<TContext> {
    pub fn id(&self) -> Id {
        self.id
    }

    #[allow(unused)]
    pub fn context(&self) -> &TContext {
        &self.context
    }

    pub fn context_mut(&mut self) -> &mut TContext {
        &mut self.context
    }

    #[allow(unused)]
    pub fn state(&self) -> &RequestStateMachine {
        &self.state_machine
    }

    pub fn state_mut(&mut self) -> &mut RequestStateMachine {
        &mut self.state_machine
    }

    pub async fn close(&mut self) {
        // TODO: Close the session, do nothing for now
    }
}
