use async_trait::async_trait;
pub use bridge_rpc::service::Service;
use bridge_rpc::{
    ResponseStatusCode, service::ServiceContext, service_error::ServiceError,
};
use dashmap::DashMap;

use crate::{Handler, HandlerService};

pub struct Router {
    handlers: DashMap<String, Box<dyn Service>>,
}

// ctors
impl Router {
    pub fn new() -> Self {
        Self {
            handlers: DashMap::new(),
        }
    }
}

impl Router {
    pub fn add_handler<T: Handler>(
        &mut self,
        name: impl AsRef<str>,
        handler: T,
    ) {
        self.handlers.insert(
            name.as_ref().to_string(),
            Box::new(HandlerService::new(handler)),
        );
    }

    pub fn add_service<T: Service>(
        &mut self,
        name: impl AsRef<str>,
        service: T,
    ) {
        self.handlers
            .insert(name.as_ref().to_string(), Box::new(service));
    }
}

#[async_trait]
impl Service for Router {
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let handler = self.handlers.get(context.request.path());
        if let Some(handler) = handler {
            handler.run(context).await
        } else {
            context
                .response
                .start(ResponseStatusCode::NO_HANDLER_FOR_PATH)
                .await
                .map_err(|e| ServiceError::custom_error(e))?
                .end()
                .await
                .map_err(|e| ServiceError::custom_error(e))?;

            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use bridge_rpc::{
        DynMap, Id,
        frame::Frame,
        server::{
            request::{Request, RequestFrameEvent},
            response::PendingResponse,
        },
    };
    use derive_new::new;
    use serde::{Deserialize, Serialize};
    use tokio::sync::{mpsc, oneshot};

    use crate::HandlerContext;

    use super::*;

    #[derive(new)]
    #[allow(unused)]
    struct ResponseAwaiter {
        frame_receiver: mpsc::Receiver<Frame>,
    }

    #[allow(unused)]
    #[derive(new)]
    pub struct FullResponse {
        pub id: Id,
        pub headers: Option<DynMap>,
        pub status: ResponseStatusCode,
        pub body: Vec<u8>,
        pub trailers: Option<DynMap>,
    }

    impl FullResponse {
        pub fn deserialize_body<TData: for<'de> Deserialize<'de>>(
            &self,
        ) -> Result<TData, rmp_serde::decode::Error> {
            rmp_serde::from_slice(&self.body)
        }
    }

    impl ResponseAwaiter {
        pub async fn wait(mut self) -> Result<FullResponse, ServiceError> {
            let start_frame = self
                .frame_receiver
                .recv()
                .await
                .expect("failed to receive response bytes");

            let start_frame = if let Frame::ResponseStart(start) = start_frame {
                start
            } else {
                panic!("expected response start frame")
            };

            let id = start_frame.id;
            let headers = start_frame.headers;
            let status = start_frame.status;

            let mut body_bytes = Vec::new();
            loop {
                let frame = self
                    .frame_receiver
                    .recv()
                    .await
                    .expect("failed to receive response bytes");

                if let Frame::ResponseEnd(end) = frame {
                    let end_frame = FullResponse {
                        id,
                        headers,
                        status,
                        body: body_bytes,
                        trailers: end.trailers,
                    };

                    return Ok(end_frame);
                } else if let Frame::ResponseBodyChunk(chunk) = frame {
                    body_bytes.extend_from_slice(&chunk.chunk);
                } else {
                    panic!("expected response body chunk or end frame");
                }
            }
        }
    }

    async fn create_service_context<TData: Serialize>(
        id: Id,
        request_path: impl AsRef<str>,
        request_headers: Option<DynMap>,
        request_data: TData,
        request_trailers: Option<DynMap>,
    ) -> (ServiceContext, ResponseAwaiter) {
        let (request_frame_tx, request_frame_rx) = mpsc::channel(256);
        let (_, request_error_rx) = oneshot::channel();
        let (response_bytes_tx, response_bytes_rx) = mpsc::channel(256);
        let request = Request::new(
            id,
            request_path.as_ref().to_string(),
            request_headers,
            request_frame_rx,
            request_error_rx,
        );

        let pending_response = PendingResponse::new(id, response_bytes_tx);
        let chunk = rmp_serde::to_vec_named(&request_data).unwrap();

        request_frame_tx
            .send(RequestFrameEvent::BodyChunk { chunk })
            .await
            .expect("failed to send request frame");

        request_frame_tx
            .send(RequestFrameEvent::End { trailers: None })
            .await
            .expect("failed to send request frame");

        request_frame_tx
            .send(RequestFrameEvent::End {
                trailers: request_trailers,
            })
            .await
            .expect("failed to send request frame");

        (
            ServiceContext::new(request, pending_response),
            ResponseAwaiter::new(response_bytes_rx),
        )
    }

    #[tokio::test]
    async fn test_router_handler_exists() {
        let mut router = Router::new();
        let data = "test";

        router.add_handler("test", async |ctx: HandlerContext| {
            let mut response_data_bytes = Vec::new();
            let mut response_data_reader = ctx.request.into_reader();

            while let Some(chunk) =
                response_data_reader.read_body_chunk().await.unwrap()
            {
                response_data_bytes.extend_from_slice(&chunk);
            }

            let mut active_response = ctx
                .response
                .start(ResponseStatusCode::SUCCESS)
                .await
                .unwrap();

            active_response
                .write_body_chunk(response_data_bytes)
                .await
                .unwrap();

            active_response.end().await.unwrap();

            Ok(())
        });

        let id = Id::new();
        let (context, response_awaiter) =
            create_service_context(id, "test", None, data, None).await;

        router.run(context).await.expect("failed to run router");

        let response = response_awaiter
            .wait()
            .await
            .expect("failed to await response");

        assert_eq!(id, response.id);
        assert_eq!(response.status, ResponseStatusCode::SUCCESS);
        assert_eq!(response.deserialize_body::<String>().unwrap(), data);
    }

    #[tokio::test]
    async fn test_router_handler_does_not_exist() {
        let mut router = Router::new();
        let data = "test";

        router.add_handler("test", async |ctx: HandlerContext| {
            ctx.response
                .start(ResponseStatusCode::SUCCESS)
                .await
                .unwrap()
                .end()
                .await
                .unwrap();

            Ok(())
        });

        let id = Id::new();
        let (context, response_awaiter) =
            create_service_context(id, "test2", None, data, None).await;

        router.run(context).await.expect("failed to run router");

        let response = response_awaiter
            .wait()
            .await
            .expect("failed to await response");

        assert_eq!(response.id, id);
        assert_eq!(response.status, ResponseStatusCode::NO_HANDLER_FOR_PATH);
        assert_eq!(response.body, b"");
    }
}
