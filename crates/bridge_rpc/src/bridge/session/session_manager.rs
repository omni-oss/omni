use std::sync::Arc;

use dashmap::DashMap;
use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use tokio::sync::Mutex;

use super::super::super::Id;
use super::*;

pub type Concurrent<T> = Arc<Mutex<T>>;

#[derive(Clone)]
pub(crate) struct SessionManager<TRequestContext = (), TResponseContext = ()> {
    request_sessions:
        Arc<DashMap<Id, Concurrent<RequestSession<TRequestContext>>>>,
    response_sessions:
        Arc<DashMap<Id, Concurrent<ResponseSession<TResponseContext>>>>,
}

impl<TRequestContext, TResponseContext>
    SessionManager<TRequestContext, TResponseContext>
{
    pub fn new() -> Self {
        Self {
            request_sessions: Arc::new(DashMap::new()),
            response_sessions: Arc::new(DashMap::new()),
        }
    }
}

impl<TRequestContext, TResponseContext>
    SessionManager<TRequestContext, TResponseContext>
{
    fn err_if_session_id_in_use(
        &self,
        id: Id,
    ) -> Result<(), SessionManagerError> {
        if self.request_sessions.contains_key(&id)
            || self.response_sessions.contains_key(&id)
        {
            return Err(
                SessionManagerErrorInner::new_session_id_in_use(id).into()
            );
        }

        Ok(())
    }

    pub async fn start_request_session(
        &self,
        id: Id,
        context: TRequestContext,
    ) -> Result<Concurrent<RequestSession<TRequestContext>>, SessionManagerError>
    {
        self.err_if_session_id_in_use(id)?;

        let request_session = concurrent(RequestSession::new(id, context));
        self.request_sessions.insert(id, request_session.clone());

        Ok(request_session)
    }

    pub async fn get_request_session(
        &self,
        id: Id,
    ) -> Option<Concurrent<RequestSession<TRequestContext>>> {
        self.request_sessions.get(&id).map(|v| v.clone())
    }

    pub async fn close_request_session(&self, id: Id) {
        if let Some((_, request_session)) = self.request_sessions.remove(&id) {
            request_session.lock().await.close().await;
        }
    }

    pub async fn start_response_session(
        &self,
        id: Id,
        context: TResponseContext,
    ) -> Result<
        Concurrent<ResponseSession<TResponseContext>>,
        SessionManagerError,
    > {
        self.err_if_session_id_in_use(id)?;

        let response_session = concurrent(ResponseSession::new(id, context));
        self.response_sessions.insert(id, response_session.clone());

        Ok(response_session)
    }

    #[allow(unused)]
    pub fn has_response_session(&self, id: Id) -> bool {
        self.response_sessions.contains_key(&id)
    }

    pub async fn get_response_session(
        &self,
        id: Id,
    ) -> Option<Concurrent<ResponseSession<TResponseContext>>> {
        self.response_sessions.get(&id).map(|v| v.clone())
    }

    pub async fn close_response_session(&self, id: Id) {
        if let Some((_, response_session)) = self.response_sessions.remove(&id)
        {
            response_session.lock().await.close().await;
        }
    }
}

fn concurrent<T>(value: T) -> Concurrent<T> {
    Arc::new(Mutex::new(value))
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct SessionManagerError(SessionManagerErrorInner);

impl SessionManagerError {
    #[allow(unused)]
    pub fn kind(&self) -> SessionManagerErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<SessionManagerErrorInner>> From<T> for SessionManagerError {
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(name(SessionManagerErrorKind), vis(pub))]
pub(crate) enum SessionManagerErrorInner {
    #[error("session id is use: {id}")]
    SessionIdInUse { id: Id },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_manager() {
        let session_manager = SessionManager::<(), ()>::new();
        let request_id_1 = Id::new();
        let request_id_2 = Id::new();

        let request_session_1 = session_manager
            .start_request_session(request_id_1, ())
            .await
            .unwrap();
        let request_session_2 = session_manager
            .start_request_session(request_id_2, ())
            .await
            .unwrap();

        assert_eq!(request_session_1.lock().await.id(), request_id_1);
        assert_eq!(request_session_2.lock().await.id(), request_id_2);

        session_manager.close_request_session(request_id_1).await;
        session_manager.close_request_session(request_id_2).await;
    }

    #[tokio::test]
    async fn test_session_manager_duplicate_id() {
        let session_manager = SessionManager::<(), ()>::new();
        let request_id = Id::new();

        let _ = session_manager
            .start_request_session(request_id, ())
            .await
            .unwrap();
        let request_session_2 =
            session_manager.start_request_session(request_id, ()).await;

        assert!(
            matches!(request_session_2, Err(SessionManagerError(SessionManagerErrorInner::SessionIdInUse { id })) if id == request_id),
            "request session 2 should be in use"
        );
    }
}
