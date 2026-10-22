//! Blotter Sync: a headless Blotter that keeps every desktop's planner in
//! sync through one server.
//!
//! The planner logic (schema, operations, merging) lives in `blotter-core`,
//! shared with the desktop app. This crate adds the HTTP API, pairing,
//! storage for server-only state and the headless duties.

use axum::{Json, Router, routing::get};
use serde::Serialize;

/// Blotter Sync's version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Port the server listens on unless told otherwise.
pub const DEFAULT_PORT: u16 = 7458;

/// What `GET /v1/hello` answers, without authentication. Pairing state, the
/// epoch and the sync protocol and schema versions join it as they land.
#[derive(Debug, Serialize)]
pub struct Hello {
    pub name: &'static str,
    pub version: &'static str,
    /// The Blotter release whose `blotter-core` this server was built with.
    pub blotter_core: &'static str,
}

/// The server's routes.
pub fn router() -> Router {
    Router::new().route("/v1/hello", get(hello))
}

async fn hello() -> Json<Hello> {
    Json(Hello {
        name: "blotter-sync",
        version: VERSION,
        blotter_core: blotter_core::VERSION,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn hello_names_the_server_and_its_core() {
        let response = router()
            .oneshot(Request::get("/v1/hello").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["name"], "blotter-sync");
        assert_eq!(json["version"], VERSION);
        assert_eq!(json["blotter_core"], blotter_core::VERSION);
    }

    #[tokio::test]
    async fn unknown_paths_are_not_found() {
        let response = router()
            .oneshot(Request::get("/v1/nope").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
