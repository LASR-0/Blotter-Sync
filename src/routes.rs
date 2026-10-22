//! The HTTP API: routes, version gating, authentication and rate limits.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRequestParts, Path, Query, State};
use axum::http::HeaderMap;
use axum::http::request::Parts;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use blotter_core::export::Export;
use serde::Deserialize;
use std::convert::Infallible;
use tokio_stream::wrappers::WatchStream;
use tokio_stream::{Stream, StreamExt};
use tower_http::compression::CompressionLayer;
use tower_http::decompression::RequestDecompressionLayer;

use crate::admin::{self, Replaced, SnapshotInfo};
use crate::api::{ApiError, ErrorBody, ErrorCode, HEADER_PROTOCOL, HEADER_SCHEMA, Hello, Versions};
use crate::auth::{self, PairRequest, Paired};
use crate::changes::{self, Bootstrap, PullResponse, PushRequest, PushResponse};
use crate::server::{self, Shared};
use crate::status;
use axum::response::Html;

/// Attempts allowed per address per window, for pairing and for failed
/// authentication.
const LIMIT: u32 = 10;
const WINDOW: Duration = Duration::from_secs(60);

/// Per-address counts of attempts that could be guessing.
#[derive(Default)]
pub struct Limiter {
    hits: Mutex<HashMap<IpAddr, (Instant, u32)>>,
}

impl Limiter {
    fn refuse() -> ApiError {
        ApiError::new(
            ErrorCode::RateLimited,
            "too many attempts from this address; wait a minute and try again",
        )
    }

    /// Refuses an address that has used up its attempts.
    fn check(&self, ip: Option<IpAddr>) -> Result<(), ApiError> {
        let Some(ip) = ip else { return Ok(()) };
        let Ok(hits) = self.hits.lock() else {
            return Ok(());
        };
        match hits.get(&ip) {
            Some((since, n)) if since.elapsed() < WINDOW && *n >= LIMIT => Err(Self::refuse()),
            _ => Ok(()),
        }
    }

    /// Counts one attempt against an address.
    fn count(&self, ip: Option<IpAddr>) {
        let Some(ip) = ip else { return };
        let Ok(mut hits) = self.hits.lock() else {
            return;
        };
        let entry = hits.entry(ip).or_insert((Instant::now(), 0));
        if entry.0.elapsed() >= WINDOW {
            *entry = (Instant::now(), 0);
        }
        entry.1 += 1;
        if hits.len() > 10_000 {
            hits.retain(|_, (since, _)| since.elapsed() < WINDOW);
        }
    }
}

/// What every handler can reach.
#[derive(Clone)]
pub struct App {
    pub server: Shared,
    pub limits: Arc<Limiter>,
}

/// The server's routes.
pub fn router(server: Shared) -> Router {
    let app = App {
        server,
        limits: Arc::default(),
    };
    Router::new()
        .route("/", get(status_page))
        .route("/v1/hello", get(hello))
        .route("/v1/pair", post(pair))
        .route("/v1/devices/self", delete(disconnect))
        .route("/v1/push", post(push))
        .route("/v1/pull", get(pull))
        .route("/v1/bootstrap", get(bootstrap))
        .route("/v1/events", get(events))
        .route("/v1/snapshots", get(list_snapshots).post(create_snapshot))
        .route("/v1/snapshots/{id}/restore", post(restore_snapshot))
        .route("/v1/import", post(import))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(RequestDecompressionLayer::new())
        .layer(CompressionLayer::new())
        .with_state(app)
}

/// The largest request body: a first push carries a whole planner.
const MAX_BODY: usize = 32 * 1024 * 1024;

async fn client_ip<S: Send + Sync>(parts: &mut Parts, state: &S) -> Option<IpAddr> {
    ConnectInfo::<SocketAddr>::from_request_parts(parts, state)
        .await
        .ok()
        .map(|c| c.0.ip())
}

/// The client's address, when the server knows it (behind a proxy, the
/// proxy's).
pub struct ClientIp(Option<IpAddr>);

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(client_ip(parts, state).await))
    }
}

fn header_number(headers: &HeaderMap, name: &str) -> Result<u32, ApiError> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok())
        .ok_or_else(|| {
            ApiError::new(
                ErrorCode::Invalid,
                format!("the {name} header is missing; is this a Blotter that can sync?"),
            )
        })
}

/// Refuses a device whose sync protocol or schema differs from the
/// server's, saying which side to update (§5.3).
pub fn check_versions(headers: &HeaderMap) -> Result<(), ApiError> {
    let theirs = Versions {
        protocol: header_number(headers, HEADER_PROTOCOL)?,
        schema: header_number(headers, HEADER_SCHEMA)?,
    };
    let ours = Versions::SERVER;
    let (code, device_older) = if theirs.protocol != ours.protocol {
        (ErrorCode::ProtocolMismatch, theirs.protocol < ours.protocol)
    } else if theirs.schema != ours.schema {
        (ErrorCode::SchemaMismatch, theirs.schema < ours.schema)
    } else {
        return Ok(());
    };
    let message = if device_older {
        "this Blotter is older than the server; update Blotter on this device"
    } else {
        "the server is older than this Blotter; update Blotter Sync"
    };
    Err(ApiError(ErrorBody {
        code,
        message: message.into(),
        server: Some(ours),
    }))
}

/// A request from a paired, current device.
pub struct Device {
    pub id: String,
}

impl FromRequestParts<App> for Device {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, ApiError> {
        check_versions(&parts.headers)?;
        let ip = client_ip(parts, app).await;
        app.limits.check(ip)?;
        let token = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|t| t.trim().to_owned());
        let Some(token) = token else {
            app.limits.count(ip);
            return Err(ApiError::new(
                ErrorCode::Unauthorized,
                "no device token was sent",
            ));
        };
        let result = server::with(&app.server, move |s| {
            auth::authenticate(&s.store, &token, chrono::Utc::now())
        })
        .await;
        match result {
            Ok(id) => Ok(Device { id }),
            Err(err) => {
                if matches!(err.code(), ErrorCode::Unauthorized | ErrorCode::Revoked) {
                    app.limits.count(ip);
                }
                Err(err)
            }
        }
    }
}

async fn hello(State(app): State<App>) -> Result<Json<Hello>, ApiError> {
    server::with(&app.server, |s| s.hello()).await.map(Json)
}

async fn pair(
    State(app): State<App>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(req): Json<PairRequest>,
) -> Result<Json<Paired>, ApiError> {
    check_versions(&headers)?;
    app.limits.check(ip)?;
    app.limits.count(ip);
    let paired = server::with(&app.server, move |s| {
        auth::pair(&s.store, &req, chrono::Utc::now())
    })
    .await?;
    tracing::info!(device = %paired.device_id, "device paired");
    Ok(Json(paired))
}

/// Disconnect: the device revokes its own token.
async fn disconnect(State(app): State<App>, device: Device) -> Result<(), ApiError> {
    let id = device.id;
    server::with(&app.server, move |s| {
        auth::revoke(&s.store, &id, chrono::Utc::now()).map(|_| ())
    })
    .await?;
    Ok(())
}

async fn push(
    State(app): State<App>,
    device: Device,
    Json(req): Json<PushRequest>,
) -> Result<Json<PushResponse>, ApiError> {
    let out = server::with(&app.server, move |s| {
        changes::push(s, &device.id, &req, chrono::Utc::now())
    })
    .await?;
    Ok(Json(out))
}

#[derive(Debug, Deserialize)]
struct PullQuery {
    epoch: String,
    #[serde(default)]
    since: i64,
}

async fn pull(
    State(app): State<App>,
    _device: Device,
    Query(q): Query<PullQuery>,
) -> Result<Json<PullResponse>, ApiError> {
    let out = server::with(&app.server, move |s| changes::pull(s, &q.epoch, q.since)).await?;
    Ok(Json(out))
}

async fn bootstrap(State(app): State<App>, _device: Device) -> Result<Json<Bootstrap>, ApiError> {
    let out = server::with(&app.server, |s| changes::bootstrap(s)).await?;
    Ok(Json(out))
}

/// How often a quiet stream sends a comment, so proxies keep it open.
const KEEP_ALIVE: Duration = Duration::from_secs(20);

/// The change stream: the newest seq and epoch now, then again whenever
/// either moves. It says only "pull now"; the rows come from `pull`.
async fn events(
    State(app): State<App>,
    _device: Device,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let notices = server::with(&app.server, |s| Ok(s.notices())).await?;
    let stream = WatchStream::new(notices).map(|notice| {
        Ok(Event::default()
            .event("seq")
            .json_data(&notice)
            .unwrap_or_else(|_| Event::default().event("seq")))
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(KEEP_ALIVE)))
}

async fn list_snapshots(
    State(app): State<App>,
    _device: Device,
) -> Result<Json<Vec<SnapshotInfo>>, ApiError> {
    server::with(&app.server, |s| admin::list(s))
        .await
        .map(Json)
}

#[derive(Debug, Default, Deserialize)]
struct SnapshotRequest {
    #[serde(default)]
    note: Option<String>,
}

async fn create_snapshot(
    State(app): State<App>,
    _device: Device,
    body: Option<Json<SnapshotRequest>>,
) -> Result<Json<SnapshotInfo>, ApiError> {
    let note = body.and_then(|Json(b)| b.note);
    server::with(&app.server, move |s| admin::snapshot(s, note))
        .await
        .map(Json)
}

async fn restore_snapshot(
    State(app): State<App>,
    device: Device,
    Path(id): Path<String>,
) -> Result<Json<Replaced>, ApiError> {
    tracing::info!(device = %device.id, snapshot = %id, "restore asked for");
    server::with(&app.server, move |s| admin::restore(s, &id))
        .await
        .map(Json)
}

async fn import(
    State(app): State<App>,
    device: Device,
    Json(export): Json<Export>,
) -> Result<Json<Replaced>, ApiError> {
    tracing::info!(device = %device.id, "import asked for");
    server::with(&app.server, move |s| admin::import(s, &export))
        .await
        .map(Json)
}

async fn status_page(State(app): State<App>) -> Result<Html<String>, ApiError> {
    server::with(&app.server, |s| status::render(s))
        .await
        .map(Html)
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::extract::connect_info::MockConnectInfo;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use super::*;
    use crate::server::Server;

    const DEVICE: &str = "01M5GHBBDWMQRX2Q9VC2GR82XE";

    fn app() -> (Router, Shared) {
        let shared = Server::open_in_memory().unwrap().shared();
        (router(shared.clone()), shared)
    }

    fn code(shared: &Shared) -> String {
        let s = shared.lock().unwrap();
        auth::open_pairing(&s.store, chrono::Utc::now())
            .unwrap()
            .code
    }

    fn request(
        method: &str,
        uri: &str,
        versions: Option<(u32, u32)>,
        token: Option<&str>,
        body: Option<Value>,
    ) -> Request<Body> {
        let mut b = Request::builder().method(method).uri(uri);
        if let Some((p, s)) = versions {
            b = b.header(HEADER_PROTOCOL, p).header(HEADER_SCHEMA, s);
        }
        if let Some(t) = token {
            b = b.header("authorization", format!("Bearer {t}"));
        }
        match body {
            Some(v) => b
                .header("content-type", "application/json")
                .body(Body::from(v.to_string()))
                .unwrap(),
            None => b.body(Body::empty()).unwrap(),
        }
    }

    fn current() -> Option<(u32, u32)> {
        Some((Versions::SERVER.protocol, Versions::SERVER.schema))
    }

    async fn send(app: &Router, req: Request<Body>) -> (StatusCode, Value) {
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    fn pair_body(code: &str) -> Option<Value> {
        Some(json!({ "code": code, "device_id": DEVICE, "name": "Laptop" }))
    }

    #[tokio::test]
    async fn hello_shows_when_pairing_is_open() {
        let (app, shared) = app();
        let (_, hello) = send(&app, request("GET", "/v1/hello", None, None, None)).await;
        assert_eq!(hello["pairing_open"], false);
        code(&shared);
        let (_, hello) = send(&app, request("GET", "/v1/hello", None, None, None)).await;
        assert_eq!(hello["pairing_open"], true);
    }

    #[tokio::test]
    async fn a_device_pairs_and_its_token_works_until_it_disconnects() {
        let (app, shared) = app();
        let c = code(&shared);
        let (status, paired) = send(
            &app,
            request("POST", "/v1/pair", current(), None, pair_body(&c)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{paired}");
        let token = paired["token"].as_str().unwrap().to_owned();
        assert_eq!(paired["device_id"], DEVICE);

        let (status, _) = send(
            &app,
            request("DELETE", "/v1/devices/self", current(), Some(&token), None),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, err) = send(
            &app,
            request("DELETE", "/v1/devices/self", current(), Some(&token), None),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(err["code"], "revoked");
    }

    #[tokio::test]
    async fn pairing_refusals_are_typed() {
        let (app, shared) = app();
        let (status, err) = send(
            &app,
            request("POST", "/v1/pair", current(), None, pair_body("ABCD-EFGH")),
        )
        .await;
        assert_eq!(
            (status, err["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("pairing_closed"))
        );
        code(&shared);
        let (_, err) = send(
            &app,
            request("POST", "/v1/pair", current(), None, pair_body("ABCD-EFGH")),
        )
        .await;
        assert_eq!(err["code"], "bad_code");
    }

    #[tokio::test]
    async fn version_mismatches_say_which_side_to_update() {
        let (app, shared) = app();
        let c = code(&shared);
        let ours = Versions::SERVER;
        let cases = [
            (
                (ours.protocol + 1, ours.schema),
                "protocol_mismatch",
                "update Blotter Sync",
            ),
            (
                (ours.protocol, ours.schema + 1),
                "schema_mismatch",
                "update Blotter Sync",
            ),
            (
                (ours.protocol, ours.schema - 1),
                "schema_mismatch",
                "update Blotter on this device",
            ),
        ];
        for (versions, code, says) in cases {
            let (status, err) = send(
                &app,
                request("POST", "/v1/pair", Some(versions), None, pair_body(&c)),
            )
            .await;
            assert_eq!(status, StatusCode::CONFLICT);
            assert_eq!(err["code"], code);
            assert!(err["message"].as_str().unwrap().contains(says), "{err}");
            assert_eq!(err["server"]["schema"], ours.schema);
        }
        let (status, err) =
            send(&app, request("POST", "/v1/pair", None, None, pair_body(&c))).await;
        assert_eq!(
            (status, err["code"].as_str()),
            (StatusCode::BAD_REQUEST, Some("invalid"))
        );
    }

    #[tokio::test]
    async fn requests_without_a_good_token_are_unauthorized() {
        let (app, _) = app();
        for token in [None, Some("not-a-token")] {
            let (status, err) = send(
                &app,
                request("DELETE", "/v1/devices/self", current(), token, None),
            )
            .await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            assert_eq!(err["code"], "unauthorized");
        }
    }

    #[tokio::test]
    async fn guessing_from_one_address_is_slowed_down() {
        let shared = Server::open_in_memory().unwrap().shared();
        let app =
            router(shared.clone()).layer(MockConnectInfo(SocketAddr::from(([10, 0, 0, 7], 5000))));
        for _ in 0..LIMIT {
            let (status, _) = send(
                &app,
                request("DELETE", "/v1/devices/self", current(), Some("guess"), None),
            )
            .await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
        }
        let (status, err) = send(
            &app,
            request("DELETE", "/v1/devices/self", current(), Some("guess"), None),
        )
        .await;
        assert_eq!(
            (status, err["code"].as_str()),
            (StatusCode::TOO_MANY_REQUESTS, Some("rate_limited"))
        );
        let c = code(&shared);
        let (status, _) = send(
            &app,
            request("POST", "/v1/pair", current(), None, pair_body(&c)),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::TOO_MANY_REQUESTS,
            "pairing from there waits too"
        );
    }
}
