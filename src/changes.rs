//! The change log devices pull from, and the push, pull and bootstrap
//! exchanges built on it.

use blotter_core::sync::capture;
use blotter_core::sync::state;
use blotter_core::sync::{Rejection, RowState, merge};
use blotter_core::time::{self, Instant};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use crate::api::{ApiError, ErrorCode};
use crate::server::Server;
use crate::store::Store;

/// Rows per pull page.
pub const PAGE: usize = 500;
/// A device clock further ahead of the server's than this gets a warning.
pub const SKEW_WARNING_MS: i64 = 5 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PushRequest {
    /// The epoch the device last bootstrapped from.
    pub epoch: String,
    /// The device's id for this push (a ULID): a resend with the same id
    /// changes nothing and gets the same answer.
    pub batch_id: String,
    pub rows: Vec<RowState>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PushResponse {
    /// The server's newest seq once the push is in.
    pub seq: i64,
    /// Rows the server couldn't take, and why.
    pub rejected: Vec<Rejection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PullResponse {
    pub epoch: String,
    pub rows: Vec<RowState>,
    /// The seq to ask from next time.
    pub seq: i64,
    /// More rows are waiting after this page.
    pub more: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bootstrap {
    pub epoch: String,
    /// Pull from here afterwards.
    pub seq: i64,
    /// Every row the server holds, tombstones included.
    pub rows: Vec<RowState>,
}

fn sql(err: rusqlite::Error) -> ApiError {
    ApiError::internal(err)
}

fn stale(epoch: &str) -> ApiError {
    ApiError::new(
        ErrorCode::StaleEpoch,
        format!(
            "the server's planner was replaced (epoch {epoch}); \
             this device needs to fetch it again"
        ),
    )
}

/// The newest seq in the log (0 when empty).
pub fn current_seq(store: &Store) -> Result<i64, ApiError> {
    store
        .conn()
        // The newest entry is never one a later entry replaced, so the
        // largest seq left is the newest handed out.
        .query_row("SELECT COALESCE(MAX(seq), 0) FROM log", [], |r| r.get(0))
        .map_err(sql)
}

/// Appends rows to the log, each replacing any earlier entry for its row.
/// Returns the newest seq.
pub fn append(
    store: &Store,
    rows: &[RowState],
    from: Option<&str>,
    now: Instant,
) -> Result<i64, ApiError> {
    let conn = store.conn();
    let at = time::fmt_instant(now);
    for row in rows {
        conn.execute(
            "DELETE FROM log WHERE tbl = ?1 AND row_key = ?2",
            (&row.table, &row.key),
        )
        .map_err(sql)?;
        let json = serde_json::to_string(row).map_err(ApiError::internal)?;
        conn.execute(
            "INSERT INTO log (tbl, row_key, row_json, from_device, at) VALUES (?1, ?2, ?3, ?4, ?5)",
            (&row.table, &row.key, json, from, &at),
        )
        .map_err(sql)?;
    }
    current_seq(store)
}

/// Moves the server's own captured changes (its duties, and tombstones a
/// merge worked out) into the log. Returns the newest seq.
pub fn record_local(server: &mut Server, now: Instant) -> Result<i64, ApiError> {
    let (rows, pending) = state::outbox(server.planner.db().conn()).map_err(ApiError::internal)?;
    let seq = append(&server.store, &rows, None, now)?;
    capture::acknowledge(server.planner.db().conn(), &pending).map_err(ApiError::internal)?;
    Ok(seq)
}

fn check_epoch(server: &Server, epoch: &str) -> Result<(), ApiError> {
    let current = server.store.meta().map_err(ApiError::internal)?.epoch;
    if epoch == current {
        Ok(())
    } else {
        Err(stale(&current))
    }
}

/// Merges a device's push, logs what it changed and answers. A batch id
/// seen before gets its first answer back and changes nothing.
pub fn push(
    server: &mut Server,
    device: &str,
    req: &PushRequest,
    now: Instant,
) -> Result<PushResponse, ApiError> {
    check_epoch(server, &req.epoch)?;
    if req.batch_id.trim().is_empty() || req.batch_id.len() > 64 {
        return Err(ApiError::new(
            ErrorCode::Invalid,
            "batch_id must be 1 to 64 characters",
        ));
    }
    let done: Option<String> = server
        .store
        .conn()
        .query_row(
            "SELECT response_json FROM pushes WHERE device_id = ?1 AND batch_id = ?2",
            (device, &req.batch_id),
            |r| r.get(0),
        )
        .optional()
        .map_err(sql)?;
    if let Some(done) = done {
        return serde_json::from_str(&done).map_err(ApiError::internal);
    }

    note_skew(&server.store, device, &req.rows, now)?;
    let merged =
        merge(server.planner.db_mut().conn_mut(), &req.rows, now).map_err(ApiError::internal)?;
    append(&server.store, &merged.changed, Some(device), now)?;
    let seq = record_local(server, now)?;
    let response = PushResponse {
        seq,
        rejected: merged.rejected,
    };
    server
        .store
        .conn()
        .execute(
            "INSERT INTO pushes (device_id, batch_id, response_json, at) VALUES (?1, ?2, ?3, ?4)",
            (
                device,
                &req.batch_id,
                serde_json::to_string(&response).map_err(ApiError::internal)?,
                time::fmt_instant(now),
            ),
        )
        .map_err(sql)?;
    if !response.rejected.is_empty() {
        tracing::warn!(
            device,
            rejected = response.rejected.len(),
            "push had rows the server refused"
        );
    }
    Ok(response)
}

/// Records how far ahead of the server a device's newest stamp is, when
/// past the warning threshold, and clears it otherwise.
fn note_skew(store: &Store, device: &str, rows: &[RowState], now: Instant) -> Result<(), ApiError> {
    let Some(newest) = rows.iter().map(|r| r.newest().wall_ms).max() else {
        return Ok(());
    };
    let ahead = newest - now.timestamp_millis();
    let warn = (ahead > SKEW_WARNING_MS).then_some(ahead);
    store
        .conn()
        .execute(
            "UPDATE devices SET clock_ahead_ms = ?2 WHERE device_id = ?1",
            (device, warn),
        )
        .map_err(sql)?;
    if let Some(ms) = warn {
        tracing::warn!(
            device,
            ahead_s = ms / 1000,
            "device clock is ahead of the server's"
        );
    }
    Ok(())
}

/// The log after `since`, a page at a time.
pub fn pull(server: &Server, epoch: &str, since: i64) -> Result<PullResponse, ApiError> {
    check_epoch(server, epoch)?;
    let current = current_seq(&server.store)?;
    if since > current {
        // The device is ahead of this log: it was replaced under it.
        return Err(stale(epoch));
    }
    let mut stmt = server
        .store
        .conn()
        .prepare("SELECT seq, row_json FROM log WHERE seq > ?1 ORDER BY seq LIMIT ?2")
        .map_err(sql)?;
    let limit = i64::try_from(PAGE + 1).unwrap_or(i64::MAX);
    let entries: Vec<(i64, String)> = stmt
        .query_map((since, limit), |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(sql)?
        .collect::<rusqlite::Result<_>>()
        .map_err(sql)?;
    let more = entries.len() > PAGE;
    let page = &entries[..entries.len().min(PAGE)];
    let rows = page
        .iter()
        .map(|(_, json)| serde_json::from_str(json).map_err(ApiError::internal))
        .collect::<Result<Vec<RowState>, _>>()?;
    let seq = if more {
        page.last().map_or(since, |(s, _)| *s)
    } else {
        current
    };
    Ok(PullResponse {
        epoch: epoch.to_owned(),
        rows,
        seq,
        more,
    })
}

/// The whole planner, for a device joining or starting over.
pub fn bootstrap(server: &Server) -> Result<Bootstrap, ApiError> {
    let meta = server.store.meta().map_err(ApiError::internal)?;
    Ok(Bootstrap {
        epoch: meta.epoch,
        seq: current_seq(&server.store)?,
        rows: state::all(server.planner.db().conn()).map_err(ApiError::internal)?,
    })
}
