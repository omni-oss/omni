use axum::{Router, routing::get};

pub fn build_router() -> Router {
    let router = Router::new().route("/", get(index));

    router
}

async fn index() -> &'static str {
    "Hello, world!"
}
