//! Devices syncing through the API: blotter-core replicas pair, bootstrap,
//! push and pull through the router, as the app will.

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use blotter_core::Database;
use blotter_core::sync::{RowState, capture, merge, state};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::api::{HEADER_PROTOCOL, HEADER_SCHEMA, Versions};
use crate::auth;
use crate::changes::{Bootstrap, PAGE, PullResponse, PushResponse};
use crate::router;
use crate::server::{Server, Shared};

const T0: &str = "2026-10-22T00:00:00Z";

struct Device {
    db: Database,
    token: String,
    epoch: String,
    seq: i64,
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let b = Request::builder()
        .method(method)
        .uri(uri)
        .header(HEADER_PROTOCOL, Versions::SERVER.protocol)
        .header(HEADER_SCHEMA, Versions::SERVER.schema)
        .header("authorization", format!("Bearer {token}"));
    let req = match body {
        Some(v) => b
            .header("content-type", "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    };
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn world() -> (Router, Shared) {
    let shared = Server::open_in_memory().unwrap().shared();
    (router(shared.clone()), shared)
}

/// Pairs a fresh device and bootstraps it from the server.
async fn join(app: &Router, shared: &Shared, name: &str) -> Device {
    let mut db = Database::open_in_memory().unwrap();
    let id = blotter_core::sync::device::id(db.conn()).unwrap();
    let code = {
        let s = shared.lock().unwrap();
        auth::open_pairing(&s.store, chrono::Utc::now())
            .unwrap()
            .code
    };
    let (status, paired) = call(
        app,
        "POST",
        "/v1/pair",
        "",
        Some(json!({ "code": code, "device_id": id, "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{paired}");
    let token = paired["token"].as_str().unwrap().to_owned();
    let (status, boot) = call(app, "GET", "/v1/bootstrap", &token, None).await;
    assert_eq!(status, StatusCode::OK, "{boot}");
    let boot: Bootstrap = serde_json::from_value(boot).unwrap();
    let merged = merge(db.conn_mut(), &boot.rows, chrono::Utc::now()).unwrap();
    assert!(merged.rejected.is_empty());
    // The fresh replica has nothing of its own to push.
    let pending = capture::pending(db.conn()).unwrap();
    capture::acknowledge(db.conn(), &pending).unwrap();
    Device {
        db,
        token,
        epoch: boot.epoch,
        seq: boot.seq,
    }
}

impl Device {
    async fn push(&mut self, app: &Router) -> PushResponse {
        let (rows, pending) = state::outbox(self.db.conn()).unwrap();
        let (status, out) = call(
            app,
            "POST",
            "/v1/push",
            &self.token,
            Some(json!({ "epoch": self.epoch, "batch_id": ulid::Ulid::generate().to_string(), "rows": rows })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{out}");
        capture::acknowledge(self.db.conn(), &pending).unwrap();
        serde_json::from_value(out).unwrap()
    }

    async fn pull(&mut self, app: &Router) {
        loop {
            let (status, page) = call(
                app,
                "GET",
                &format!("/v1/pull?epoch={}&since={}", self.epoch, self.seq),
                &self.token,
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{page}");
            let page: PullResponse = serde_json::from_value(page).unwrap();
            let merged = merge(self.db.conn_mut(), &page.rows, chrono::Utc::now()).unwrap();
            assert!(merged.rejected.is_empty(), "{:?}", merged.rejected);
            self.seq = page.seq;
            if !page.more {
                return;
            }
        }
    }

    fn add_item(&self, id: &str, title: &str) {
        self.db
            .conn()
            .execute(
                "INSERT INTO items (id, kind, anchoring, title, created_at, updated_at) \
                 VALUES (?1, 'task', 'flexible', ?2, ?3, ?3)",
                (id, title, T0),
            )
            .unwrap();
    }

    fn title(&self, id: &str) -> Option<String> {
        self.db
            .conn()
            .query_row("SELECT title FROM items WHERE id = ?1", [id], |r| r.get(0))
            .ok()
    }

    fn rows(&self) -> Vec<RowState> {
        state::all(self.db.conn()).unwrap()
    }
}

fn server_rows(shared: &Shared) -> Vec<RowState> {
    let s = shared.lock().unwrap();
    state::all(s.planner.db().conn()).unwrap()
}

#[tokio::test]
async fn two_devices_converge_through_the_server() {
    let (app, shared) = world();
    let mut laptop = join(&app, &shared, "Laptop").await;
    let mut desktop = join(&app, &shared, "Desktop").await;

    laptop.add_item("I1", "Buy milk");
    laptop.push(&app).await;
    desktop.pull(&app).await;
    assert_eq!(desktop.title("I1").as_deref(), Some("Buy milk"));

    // Both edit offline: different groups both survive.
    laptop
        .db
        .conn()
        .execute(
            "UPDATE items SET title = 'Buy oat milk' WHERE id = 'I1'",
            [],
        )
        .unwrap();
    desktop
        .db
        .conn()
        .execute("UPDATE items SET priority = 3 WHERE id = 'I1'", [])
        .unwrap();
    desktop.push(&app).await;
    laptop.push(&app).await;
    laptop.pull(&app).await;
    desktop.pull(&app).await;

    assert_eq!(desktop.title("I1").as_deref(), Some("Buy oat milk"));
    let server = server_rows(&shared);
    assert_eq!(laptop.rows(), server);
    assert_eq!(desktop.rows(), server);
}

#[tokio::test]
async fn a_resent_push_changes_nothing_and_gets_the_same_answer() {
    let (app, shared) = world();
    let laptop = join(&app, &shared, "Laptop").await;
    laptop.add_item("I1", "Buy milk");
    let (rows, _) = state::outbox(laptop.db.conn()).unwrap();
    let body = json!({ "epoch": laptop.epoch, "batch_id": "B1", "rows": rows });
    let (_, first) = call(&app, "POST", "/v1/push", &laptop.token, Some(body.clone())).await;
    let (_, again) = call(&app, "POST", "/v1/push", &laptop.token, Some(body)).await;
    assert_eq!(first, again);
    let entries: i64 = shared
        .lock()
        .unwrap()
        .store
        .conn()
        .query_row("SELECT count(*) FROM log WHERE tbl = 'items'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(entries, 1);
}

#[tokio::test]
async fn a_replaced_planner_sends_devices_back_to_bootstrap() {
    let (app, shared) = world();
    let laptop = join(&app, &shared, "Laptop").await;
    shared.lock().unwrap().store.new_epoch().unwrap();
    let (status, err) = call(
        &app,
        "GET",
        &format!("/v1/pull?epoch={}&since=0", laptop.epoch),
        &laptop.token,
        None,
    )
    .await;
    assert_eq!(
        (status, err["code"].as_str()),
        (StatusCode::GONE, Some("stale_epoch"))
    );
    let (status, err) = call(
        &app,
        "POST",
        "/v1/push",
        &laptop.token,
        Some(json!({ "epoch": laptop.epoch, "batch_id": "B1", "rows": [] })),
    )
    .await;
    assert_eq!(
        (status, err["code"].as_str()),
        (StatusCode::GONE, Some("stale_epoch"))
    );
}

#[tokio::test]
async fn a_cursor_ahead_of_the_log_means_start_again() {
    let (app, shared) = world();
    let laptop = join(&app, &shared, "Laptop").await;
    let (status, err) = call(
        &app,
        "GET",
        &format!("/v1/pull?epoch={}&since=999999", laptop.epoch),
        &laptop.token,
        None,
    )
    .await;
    assert_eq!(
        (status, err["code"].as_str()),
        (StatusCode::GONE, Some("stale_epoch"))
    );
}

#[tokio::test]
async fn pulls_come_in_pages() {
    let (app, shared) = world();
    let mut laptop = join(&app, &shared, "Laptop").await;
    let mut desktop = join(&app, &shared, "Desktop").await;
    let tx = laptop.db.conn_mut().transaction().unwrap();
    for i in 0..(PAGE + 20) {
        tx.execute(
            "INSERT INTO items (id, kind, anchoring, title, created_at, updated_at) \
             VALUES (?1, 'task', 'flexible', 'x', ?2, ?2)",
            (format!("I{i:04}"), T0),
        )
        .unwrap();
    }
    tx.commit().unwrap();
    laptop.push(&app).await;

    let (_, first) = call(
        &app,
        "GET",
        &format!("/v1/pull?epoch={}&since={}", desktop.epoch, desktop.seq),
        &desktop.token,
        None,
    )
    .await;
    assert_eq!(first["rows"].as_array().unwrap().len(), PAGE);
    assert_eq!(first["more"], true);
    desktop.pull(&app).await;
    let items: i64 = desktop
        .db
        .conn()
        .query_row("SELECT count(*) FROM items", [], |r| r.get(0))
        .unwrap();
    assert_eq!(items, i64::try_from(PAGE + 20).unwrap());
}

#[tokio::test]
async fn refused_rows_come_back_in_the_answer() {
    let (app, shared) = world();
    let laptop = join(&app, &shared, "Laptop").await;
    laptop.add_item("I1", "Buy milk");
    let (mut rows, _) = state::outbox(laptop.db.conn()).unwrap();
    let mut bad = rows[0].clone();
    bad.table = "chat_threads".into();
    rows.push(bad);
    let (status, out) = call(
        &app,
        "POST",
        "/v1/push",
        &laptop.token,
        Some(json!({ "epoch": laptop.epoch, "batch_id": "B1", "rows": rows })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let out: PushResponse = serde_json::from_value(out).unwrap();
    assert_eq!(out.rejected.len(), 1);
    assert_eq!(out.rejected[0].table, "chat_threads");
}

#[tokio::test]
async fn a_clock_far_ahead_is_noted() {
    let (app, shared) = world();
    let mut laptop = join(&app, &shared, "Laptop").await;
    blotter_core::sync::hlc::tick(
        laptop.db.conn(),
        chrono::Utc::now() + chrono::Duration::hours(1),
    )
    .unwrap();
    laptop.add_item("I1", "From the future");
    laptop.push(&app).await;
    let devices = auth::devices(&shared.lock().unwrap().store).unwrap();
    let ahead = devices[0].clock_ahead_ms.unwrap();
    assert!(ahead > 50 * 60 * 1000, "{ahead}");
}

#[tokio::test]
async fn pulling_needs_a_token() {
    let (app, _) = world();
    let (status, _) = call(&app, "GET", "/v1/pull?epoch=x&since=0", "nope", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = call(&app, "GET", "/v1/bootstrap", "nope", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// The next `seq` notice on a change stream, within a few seconds.
async fn next_notice(body: &mut Body) -> Value {
    loop {
        let frame = tokio::time::timeout(std::time::Duration::from_secs(5), body.frame())
            .await
            .expect("no notice within 5 s")
            .expect("the stream ended")
            .unwrap();
        let Ok(data) = frame.into_data() else {
            continue;
        };
        let text = String::from_utf8(data.to_vec()).unwrap();
        if let Some(json) = text
            .lines()
            .find_map(|l| l.strip_prefix("data:"))
            .filter(|_| text.contains("event: seq"))
        {
            return serde_json::from_str(json.trim()).unwrap();
        }
    }
}

#[tokio::test]
async fn the_change_stream_says_when_to_pull() {
    let (app, shared) = world();
    let mut laptop = join(&app, &shared, "Laptop").await;
    let desktop = join(&app, &shared, "Desktop").await;

    let req = Request::get("/v1/events")
        .header(HEADER_PROTOCOL, Versions::SERVER.protocol)
        .header(HEADER_SCHEMA, Versions::SERVER.schema)
        .header("authorization", format!("Bearer {}", desktop.token))
        .header("accept-encoding", "gzip")
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "text/event-stream");
    assert!(
        res.headers().get("content-encoding").is_none(),
        "never compressed or buffered"
    );
    let mut body = res.into_body();

    let first = next_notice(&mut body).await;
    assert_eq!(first["seq"], desktop.seq);
    assert_eq!(first["epoch"], desktop.epoch);

    laptop.add_item("I1", "Buy milk");
    let pushed = laptop.push(&app).await;
    let next = next_notice(&mut body).await;
    assert_eq!(next["seq"], pushed.seq);
    assert!(pushed.seq > desktop.seq);
}

#[tokio::test]
async fn the_change_stream_needs_a_token() {
    let (app, _) = world();
    let (status, _) = call(&app, "GET", "/v1/events", "nope", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
