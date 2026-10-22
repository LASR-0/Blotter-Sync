//! Blotter Sync: a headless Blotter that keeps every desktop's planner in
//! sync through one server.
//!
//! The planner logic (schema, operations, merging) lives in `blotter-core`,
//! shared with the desktop app. This crate adds the HTTP API, pairing,
//! storage for server-only state and the headless duties.

pub mod api;
pub mod config;
pub mod server;
pub mod store;

use axum::extract::State;
use axum::{Json, Router, routing::get};

use crate::api::{ApiError, Hello};
use crate::server::Shared;

/// Blotter Sync's version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Port the server listens on unless told otherwise.
pub const DEFAULT_PORT: u16 = 7458;

/// The server's routes.
pub fn router(shared: Shared) -> Router {
    Router::new()
        .route("/v1/hello", get(hello))
        .with_state(shared)
}

async fn hello(State(shared): State<Shared>) -> Result<Json<Hello>, ApiError> {
    server::with(&shared, |s| s.hello()).await.map(Json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn app() -> Router {
        router(server::Server::open_in_memory().unwrap().shared())
    }

    #[tokio::test]
    async fn hello_names_the_server_its_core_and_its_versions() {
        let response = app()
            .oneshot(Request::get("/v1/hello").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let hello: Hello = serde_json::from_slice(&body).unwrap();
        assert_eq!(hello.name, "blotter-sync");
        assert_eq!(hello.version, VERSION);
        assert_eq!(hello.blotter_core, blotter_core::VERSION);
        assert_eq!(hello.versions, api::Versions::SERVER);
        assert_eq!(hello.server_id.len(), 26);
        assert!(!hello.pairing_open);
        let raw: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(raw["protocol"], 1);
        assert_eq!(raw["schema"], 1);
    }

    #[tokio::test]
    async fn unknown_paths_are_not_found() {
        let response = app()
            .oneshot(Request::get("/v1/nope").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
