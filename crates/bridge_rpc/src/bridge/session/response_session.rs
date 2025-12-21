use super::{super::super::Id, ResponseStateMachine};

pub(crate) struct ResponseSession<TContext = ()> {
    id: Id,
    state_machine: ResponseStateMachine,
    context: TContext,
}

impl<TContext> ResponseSession<TContext> {
    pub fn new(id: Id, context: TContext) -> Self {
        Self {
            id,
            state_machine: ResponseStateMachine::new(),
            context,
        }
    }
}

impl<TContext> ResponseSession<TContext> {
    #[allow(unused)]
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
    pub fn state(&self) -> &ResponseStateMachine {
        &self.state_machine
    }

    pub fn state_mut(&mut self) -> &mut ResponseStateMachine {
        &mut self.state_machine
    }

    pub async fn close(&mut self) {
        // TODO: Close the session, do nothing for now
    }
}
