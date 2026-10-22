//! The planner's automatic duties, which the server owns once devices pair
//! (D-S2): the day's rollover, holiday pruning, public-holiday upkeep,
//! subscription refreshes and the daily backup. Each runs through the core
//! as the system actor, and what it changes reaches devices like any other
//! change. Devices in Home server mode skip them (Blotter 16b).
//!
//! A tick does the database work under the server's lock and says what to
//! fetch; the fetching happens outside it; then the results go in.

use std::time::Duration;

use blotter_core::public_holidays::{self, Upkeep};
use blotter_core::snapshots;
use blotter_core::subscriptions;
use chrono::NaiveDate;
use reqwest::Client;

use crate::api::ApiError;
use crate::changes;
use crate::server::{self, Server, Shared};

/// How often duties are looked at.
pub const EVERY: Duration = Duration::from_secs(60);
/// The first look waits a little after start.
const FIRST_AFTER: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// Largest calendar or holiday list accepted (as the app's).
const MAX_BYTES: usize = 20 * 1024 * 1024;

/// The local days the once-a-day duties last ran or were tried.
#[derive(Debug, Default)]
pub struct Days {
    pruned: Option<NaiveDate>,
    /// A failed holiday fetch (offline) waits until tomorrow.
    holidays_tried: Option<NaiveDate>,
}

/// What a tick found to fetch.
#[derive(Debug, Default, PartialEq)]
pub struct Fetches {
    /// Due subscriptions: id, and the address to fetch or why there is none.
    pub subscriptions: Vec<(String, Result<String, String>)>,
    pub holidays: Option<Upkeep>,
}

/// What came back from fetching.
#[derive(Debug, Default)]
pub struct Fetched {
    pub subscriptions: Vec<(String, Result<String, String>)>,
    pub holidays: Option<(Upkeep, Result<String, String>)>,
}

fn log_err<T>(what: &str, r: blotter_core::Result<T>) -> Option<T> {
    r.map_err(|e| tracing::warn!("{what} failed: {e}")).ok()
}

/// The database half of a tick: runs what's due, logs it for devices, and
/// returns what needs fetching.
pub fn tick(server: &mut Server) -> Result<Fetches, ApiError> {
    let planner = &mut server.planner;
    let now = planner.now();
    let tz = planner.settings().map_err(ApiError::internal)?.tz;
    let today = now.with_timezone(&tz).date_naive();

    if let Some(Some(outcome)) = log_err("rollover", planner.rollover_if_due())
        && outcome.batch_id.is_some()
    {
        tracing::info!(changes = outcome.changes.len(), "rollover ran");
    }
    if server.days.pruned != Some(today) {
        server.days.pruned = Some(today);
        log_err(
            "pruning holidays",
            planner.execute(
                "prune_holidays",
                serde_json::json!({}),
                &blotter_core::Actor::System,
            ),
        );
    }
    if let Some(path) = &server.planner_path {
        let conn = planner.db().conn();
        let backup =
            snapshots::dir(conn, path).and_then(|dir| snapshots::daily_if_due(conn, &dir, tz, now));
        if let Some(Some(snap)) = log_err("daily backup", backup) {
            tracing::info!(path = %snap.path, "daily backup taken");
        }
    }

    let mut fetches = Fetches::default();
    let conn = planner.db().conn();
    for sub in subscriptions::due(conn, now).map_err(ApiError::internal)? {
        let url = subscriptions::fetch_url(&sub.url).map_err(|e| e.to_string());
        fetches.subscriptions.push((sub.id, url));
    }
    if server.days.holidays_tried != Some(today)
        && let Some(due) = public_holidays::upkeep(planner).map_err(ApiError::internal)?
    {
        server.days.holidays_tried = Some(today);
        fetches.holidays = Some(due);
    }

    changes::record_local(server, now)?;
    Ok(fetches)
}

/// The second half: records what the fetches brought back.
pub fn record(server: &mut Server, fetched: Fetched) -> Result<(), ApiError> {
    let planner = &mut server.planner;
    let now = planner.now();
    let tz = planner.settings().map_err(ApiError::internal)?.tz;
    for (id, body) in &fetched.subscriptions {
        let body = body.as_deref().map_err(Clone::clone);
        let conn = planner.db_mut().conn_mut();
        if let Some(s) = log_err(
            "recording a calendar",
            subscriptions::record_fetch(conn, tz, id, body, now),
        ) {
            tracing::info!(subscription = %s.name, status = ?s.last_status, "calendar refreshed");
        }
    }
    if let Some((due, body)) = &fetched.holidays {
        match body {
            Ok(json) => {
                if log_err("importing public holidays", due.apply(planner, json)).is_some() {
                    tracing::info!(year = due.year, country = %due.country, "public holidays imported");
                }
            }
            Err(e) => tracing::warn!(year = due.year, "fetching public holidays failed: {e}"),
        }
    }
    changes::record_local(server, now)?;
    Ok(())
}

/// The HTTP client duties fetch with.
pub fn client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .user_agent(concat!("BlotterSync/", env!("CARGO_PKG_VERSION")))
        .build()
}

/// Fetches `url` as text, refusing anything too large.
pub async fn fetch(client: &Client, url: &str, accept: &str) -> Result<String, String> {
    let describe = |e: reqwest::Error| {
        if e.is_timeout() {
            "the calendar took too long to answer".to_owned()
        } else if e.is_connect() {
            "couldn't connect (check the address and the server's connection)".to_owned()
        } else {
            e.without_url().to_string()
        }
    };
    let mut resp = client
        .get(url)
        .header(reqwest::header::ACCEPT, accept)
        .send()
        .await
        .map_err(describe)?;
    let status = resp.status();
    if !status.is_success() {
        return Err(match status.canonical_reason() {
            Some(reason) => format!("HTTP {} {reason}", status.as_u16()),
            None => format!("HTTP {}", status.as_u16()),
        });
    }
    let too_big = || format!("larger than {} MB", MAX_BYTES / (1024 * 1024));
    if resp.content_length().is_some_and(|n| n > MAX_BYTES as u64) {
        return Err(too_big());
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(describe)? {
        if body.len() + chunk.len() > MAX_BYTES {
            return Err(too_big());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// One full round: tick, fetch, record.
pub async fn round(shared: &Shared, client: &Client) -> Result<(), ApiError> {
    let fetches = server::with(shared, tick).await?;
    if fetches == Fetches::default() {
        return Ok(());
    }
    let mut fetched = Fetched::default();
    for (id, url) in fetches.subscriptions {
        let body = match url {
            Ok(url) => fetch(client, &url, "text/calendar, */*;q=0.5").await,
            Err(why) => Err(why),
        };
        fetched.subscriptions.push((id, body));
    }
    if let Some(due) = fetches.holidays {
        let body = fetch(client, &due.url(), "application/json").await;
        fetched.holidays = Some((due, body));
    }
    server::with(shared, move |s| record(s, fetched)).await
}

/// Runs duties every minute until the process ends.
pub async fn run(shared: Shared) {
    let client = match client() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("duties can't fetch anything: {e}");
            return;
        }
    };
    tokio::time::sleep(FIRST_AFTER).await;
    let mut every = tokio::time::interval(EVERY);
    loop {
        every.tick().await;
        if let Err(e) = round(&shared, &client).await {
            tracing::warn!("duties: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use blotter_core::time::{Clock, Instant};
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    use super::*;

    /// A clock the test can move.
    #[derive(Clone)]
    struct TestClock(Arc<Mutex<Instant>>);

    impl Clock for TestClock {
        fn now(&self) -> Instant {
            *self.0.lock().unwrap()
        }
    }

    fn at(y: i32, m: u32, d: u32, h: u32) -> Instant {
        Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap()
    }

    fn server_at(when: Instant) -> (Server, TestClock) {
        let clock = TestClock(Arc::new(Mutex::new(when)));
        (
            Server::open_in_memory_at(Box::new(clock.clone())).unwrap(),
            clock,
        )
    }

    fn logged(server: &Server, table: &str) -> Vec<String> {
        server
            .store
            .conn()
            .prepare("SELECT row_key FROM log WHERE tbl = ?1 ORDER BY seq")
            .unwrap()
            .query_map([table], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    #[test]
    fn rollover_runs_once_a_day_and_reaches_the_log() {
        let (mut server, clock) = server_at(at(2026, 10, 22, 10));
        server
            .planner
            .db()
            .conn()
            .execute(
                "INSERT INTO items (id, kind, anchoring, title, start_at, end_at, duration_min, \
                 created_at, updated_at) VALUES ('I1', 'task', 'flexible', 'Yesterday', \
                 '2026-10-21T09:00:00Z', '2026-10-21T10:00:00Z', 60, '2026-10-20T00:00:00Z', \
                 '2026-10-20T00:00:00Z')",
                [],
            )
            .unwrap();
        tick(&mut server).unwrap();
        let start: Option<String> = server
            .planner
            .db()
            .conn()
            .query_row("SELECT start_at FROM items WHERE id = 'I1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(start, None, "back to the Inbox");
        assert!(logged(&server, "items").contains(&"I1".to_owned()));
        assert!(logged(&server, "settings").contains(&"last_rollover_date".to_owned()));

        let seq = changes::current_seq(&server.store).unwrap();
        *clock.0.lock().unwrap() = at(2026, 10, 22, 11);
        tick(&mut server).unwrap();
        assert_eq!(
            changes::current_seq(&server.store).unwrap(),
            seq,
            "not again today"
        );
    }

    #[test]
    fn ended_holidays_are_pruned_and_devices_hear_of_it() {
        let (mut server, _) = server_at(at(2026, 10, 22, 10));
        server
            .planner
            .execute(
                "create_holiday",
                json!({ "label": "Break", "start_date": "2026-10-01", "end_date": "2026-10-05" }),
                &blotter_core::Actor::User,
            )
            .unwrap();
        changes::record_local(&mut server, Utc::now()).unwrap();
        tick(&mut server).unwrap();
        let left: i64 = server
            .planner
            .db()
            .conn()
            .query_row("SELECT count(*) FROM holidays", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0);
        let log: String = server
            .store
            .conn()
            .query_row("SELECT row_json FROM log WHERE tbl = 'holidays'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(log.contains("\"deleted\":true"), "{log}");
    }

    #[test]
    fn public_holidays_due_are_fetched_once_a_day_and_imported() {
        let (mut server, _) = server_at(at(2026, 10, 22, 10));
        let year = 2026;
        server
            .planner
            .execute(
                "import_public_holidays",
                json!({ "country": "AU", "region": "AU-VIC", "years": [year], "holidays": [] }),
                &blotter_core::Actor::User,
            )
            .unwrap();
        let fetches = tick(&mut server).unwrap();
        let due = fetches.holidays.unwrap();
        assert_eq!(due.year, year + 1);
        assert!(
            tick(&mut server).unwrap().holidays.is_none(),
            "one try a day"
        );

        let answer = json!([{ "date": "2027-01-01", "localName": "New Year's Day", "global": true, "types": ["Public"] }]);
        record(
            &mut server,
            Fetched {
                holidays: Some((due, Ok(answer.to_string()))),
                ..Fetched::default()
            },
        )
        .unwrap();
        assert_eq!(logged(&server, "holidays").len(), 1);
        assert_eq!(
            public_holidays::upkeep(&server.planner).unwrap(),
            None,
            "next year is in"
        );
    }

    const ICS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//test//EN\r\nX-WR-CALNAME:Work\r\n\
        BEGIN:VEVENT\r\nUID:e1\r\nDTSTAMP:20261020T000000Z\r\nDTSTART:20261023T010000Z\r\n\
        DTEND:20261023T020000Z\r\nSUMMARY:Standup\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    async fn calendar_server() -> String {
        let app = axum::Router::new().route("/work.ics", axum::routing::get(|| async { ICS }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}/work.ics")
    }

    fn subscribe(shared: &Shared, url: &str) -> String {
        let s = shared.lock().unwrap();
        subscriptions::add(
            s.planner.db().conn(),
            subscriptions::NewSubscription {
                url: url.to_owned(),
                name: None,
                refresh_min: Some(60),
                color: None,
                as_blockout: false,
            },
            Utc::now(),
        )
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn due_calendars_are_fetched_and_their_events_reach_the_log() {
        let shared = Server::open_in_memory().unwrap().shared();
        let url = calendar_server().await;
        let id = subscribe(&shared, &url);
        round(&shared, &client().unwrap()).await.unwrap();

        {
            let s = shared.lock().unwrap();
            let (status, events): (String, i64) = s
                .planner
                .db()
                .conn()
                .query_row(
                    "SELECT last_status, (SELECT count(*) FROM ics_events) FROM ics_subscriptions \
                 WHERE id = ?1",
                    [&id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!((status.as_str(), events), ("ok", 1));
            assert_eq!(logged(&s, "ics_events").len(), 1);
            let sub: String = s
                .store
                .conn()
                .query_row(
                    "SELECT row_json FROM log WHERE tbl = 'ics_subscriptions'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(
                sub.contains("fetch_status") && sub.contains("\"ok\""),
                "{sub}"
            );
        }

        // Not due again for an hour.
        assert!(
            server::with(&shared, tick)
                .await
                .unwrap()
                .subscriptions
                .is_empty()
        );
    }

    #[tokio::test]
    async fn a_failed_fetch_is_recorded_not_fatal() {
        let shared = Server::open_in_memory().unwrap().shared();
        let id = subscribe(&shared, "http://127.0.0.1:9/nothing.ics");
        round(&shared, &client().unwrap()).await.unwrap();
        let s = shared.lock().unwrap();
        let (status, error): (String, String) = s
            .planner
            .db()
            .conn()
            .query_row(
                "SELECT last_status, last_error FROM ics_subscriptions WHERE id = ?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "error");
        assert!(error.contains("connect"), "{error}");
    }

    #[test]
    fn a_daily_backup_is_taken_beside_the_planner() {
        let dir = tempfile::tempdir().unwrap();
        let mut server = Server::open(dir.path()).unwrap();
        tick(&mut server).unwrap();
        tick(&mut server).unwrap();
        let backups: Vec<_> = std::fs::read_dir(dir.path().join("snapshots"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(backups.len(), 1, "{backups:?}");
        assert!(backups[0].ends_with("-daily.db"));
    }
}
