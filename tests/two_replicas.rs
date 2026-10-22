//! Two Blotter replicas syncing through a real server over real HTTP: the
//! app's side of the protocol, played by blotter-core and reqwest, against
//! the server listening on a local port (roadmap §7).

// Test helpers outside #[test] functions; clippy.toml only covers those.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use blotter_core::sync::{capture, merge, state};
use blotter_core::{Actor, Database, Planner};
use blotter_sync::api::{HEADER_PROTOCOL, HEADER_SCHEMA, Versions};
use blotter_sync::changes::{Bootstrap, PullResponse, PushResponse};
use blotter_sync::server::{Server, Shared};
use blotter_sync::{auth, router};
use reqwest::{Client, RequestBuilder};
use serde_json::{Value, json};

struct Replica {
    planner: Planner,
    http: Client,
    base: String,
    token: String,
    epoch: String,
    seq: i64,
}

fn gated(req: RequestBuilder, token: &str) -> RequestBuilder {
    req.header(HEADER_PROTOCOL, Versions::SERVER.protocol)
        .header(HEADER_SCHEMA, Versions::SERVER.schema)
        .bearer_auth(token)
}

async fn start() -> (String, Shared) {
    let shared = Server::open_in_memory().unwrap().shared();
    let app = router(shared.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (format!("http://{addr}"), shared)
}

impl Replica {
    async fn join(base: &str, shared: &Shared, name: &str) -> Self {
        let mut planner = Planner::new(Database::open_in_memory().unwrap(), chrono_tz::UTC);
        let http = Client::new();
        let device_id = blotter_core::sync::device::id(planner.db().conn()).unwrap();
        let code = {
            let s = shared.lock().unwrap();
            auth::open_pairing(&s.store, chrono::Utc::now())
                .unwrap()
                .code
        };
        let paired: Value = gated(http.post(format!("{base}/v1/pair")), "")
            .json(&json!({ "code": code, "device_id": device_id, "name": name }))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        let token = paired["token"].as_str().unwrap().to_owned();
        let boot: Bootstrap = gated(http.get(format!("{base}/v1/bootstrap")), &token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        merge(planner.db_mut().conn_mut(), &boot.rows, chrono::Utc::now()).unwrap();
        Self {
            planner,
            http,
            base: base.to_owned(),
            token,
            epoch: boot.epoch,
            seq: boot.seq,
        }
    }

    fn run(&mut self, op: &str, args: Value) -> Value {
        self.planner.execute(op, args, &Actor::User).unwrap().result
    }

    async fn push(&mut self) -> PushResponse {
        let (rows, pending) = state::outbox(self.planner.db().conn()).unwrap();
        let out: PushResponse = gated(
            self.http.post(format!("{}/v1/push", self.base)),
            &self.token,
        )
        .json(&json!({
            "epoch": self.epoch,
            "batch_id": ulid::Ulid::generate().to_string(),
            "rows": rows,
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
        assert!(out.rejected.is_empty(), "{:?}", out.rejected);
        capture::acknowledge(self.planner.db().conn(), &pending).unwrap();
        out
    }

    async fn pull(&mut self) {
        loop {
            let page: PullResponse = gated(
                self.http.get(format!(
                    "{}/v1/pull?epoch={}&since={}",
                    self.base, self.epoch, self.seq
                )),
                &self.token,
            )
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
            let merged = merge(
                self.planner.db_mut().conn_mut(),
                &page.rows,
                chrono::Utc::now(),
            )
            .unwrap();
            assert!(merged.rejected.is_empty(), "{:?}", merged.rejected);
            self.seq = page.seq;
            if !page.more {
                return;
            }
        }
    }

    fn item(&mut self, id: &str) -> Value {
        self.run("get_item", json!({ "id": id }))
    }
}

/// Waits on a change stream until it announces a seq of at least `seq`.
async fn wait_for_seq(stream: &mut reqwest::Response, seq: i64) {
    let mut text = String::new();
    loop {
        let chunk = tokio::time::timeout(Duration::from_secs(5), stream.chunk())
            .await
            .expect("no notice within 5 s")
            .unwrap()
            .expect("the stream ended");
        text.push_str(&String::from_utf8_lossy(&chunk));
        let seen = text
            .lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .filter_map(|d| serde_json::from_str::<Value>(d.trim()).ok())
            .filter_map(|v| v["seq"].as_i64())
            .max();
        if seen.is_some_and(|s| s >= seq) {
            return;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn two_replicas_sync_over_http() {
    let (base, shared) = start().await;
    let mut laptop = Replica::join(&base, &shared, "Laptop").await;
    let mut desktop = Replica::join(&base, &shared, "Desktop").await;

    // The desktop listens for changes as the app will.
    let mut stream = gated(
        desktop.http.get(format!("{base}/v1/events")),
        &desktop.token,
    )
    .send()
    .await
    .unwrap()
    .error_for_status()
    .unwrap();

    let id = laptop.run(
        "create_item",
        json!({ "title": "Dentist", "start": "2026-10-23T03:00:00Z", "duration_min": 45, "kind": "event" }),
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let pushed = laptop.push().await;
    wait_for_seq(&mut stream, pushed.seq).await;
    desktop.pull().await;
    assert_eq!(desktop.item(&id)["title"], "Dentist");

    // Offline on both: a reschedule on one, a retitle on the other.
    laptop.run(
        "update_item",
        json!({ "id": id, "start": "2026-10-23T05:00:00Z" }),
    );
    desktop.run(
        "update_item",
        json!({ "id": id, "title": "Dentist (check-up)" }),
    );
    desktop.push().await;
    laptop.push().await;
    laptop.pull().await;
    desktop.pull().await;

    for replica in [&mut laptop, &mut desktop] {
        let item = replica.item(&id);
        assert_eq!(item["title"], "Dentist (check-up)");
        assert_eq!(item["start_at"], "2026-10-23T05:00:00Z");
    }
    let server_rows = {
        let s = shared.lock().unwrap();
        state::all(s.planner.db().conn()).unwrap()
    };
    assert_eq!(state::all(laptop.planner.db().conn()).unwrap(), server_rows);
    assert_eq!(
        state::all(desktop.planner.db().conn()).unwrap(),
        server_rows
    );

    // The laptop's reschedule can't be undone over the desktop's retitle:
    // undo never overwrites another device's change (Blotter #78).
    let refused = laptop
        .planner
        .execute("undo", json!({}), &Actor::User)
        .unwrap_err();
    assert!(
        refused.to_string().contains("has changed since"),
        "{refused}"
    );

    // Its own change to an item no one else touched undoes, and that syncs.
    let gym = laptop.run("create_item", json!({ "title": "Gym" }))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    laptop.push().await;
    desktop.pull().await;
    laptop.run("update_item", json!({ "id": gym, "priority": 2 }));
    laptop.run("undo", json!({}));
    laptop.push().await;
    desktop.pull().await;
    assert_eq!(desktop.item(&gym)["priority"], 0);
    assert_eq!(desktop.item(&gym)["title"], "Gym");
}
