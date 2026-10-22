//! Snapshots of the canonical planner, restoring one, and replacing the
//! planner from a JSON export. Restore and import replace the planner
//! wholesale, so each starts a new epoch: the change log starts over and
//! every device is told to bootstrap again (roadmap §5.3).

use std::path::PathBuf;

use blotter_core::export::{self, Export};
use blotter_core::snapshots::{self, Reason, Snapshot};
use blotter_core::sync::capture;
use serde::{Deserialize, Serialize};

use crate::api::{ApiError, ErrorCode};
use crate::changes;
use crate::server::Server;

/// A snapshot as the API shows it: not where the server keeps the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotInfo {
    pub id: String,
    pub reason: String,
    pub manual: bool,
    pub note: Option<String>,
    pub size_bytes: Option<i64>,
    pub created_at: String,
}

impl From<&Snapshot> for SnapshotInfo {
    fn from(s: &Snapshot) -> Self {
        Self {
            id: s.id.clone(),
            reason: serde_json::to_value(s.reason)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default(),
            manual: s.manual,
            note: s.note.clone(),
            size_bytes: s.size_bytes,
            created_at: blotter_core::time::fmt_instant(s.created_at),
        }
    }
}

/// What a restore or import did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Replaced {
    /// The new epoch every device bootstraps from.
    pub epoch: String,
    /// The snapshot taken just before, so the replace can be undone.
    pub before: SnapshotInfo,
}

fn core(err: blotter_core::CoreError) -> ApiError {
    match err {
        blotter_core::CoreError::Invalid(m) => ApiError::new(ErrorCode::Invalid, m),
        blotter_core::CoreError::NotFound { entity, id } => {
            ApiError::new(ErrorCode::Invalid, format!("no {entity} {id}"))
        }
        other => ApiError::internal(other),
    }
}

fn planner_path(server: &Server) -> Result<PathBuf, ApiError> {
    server
        .planner_path
        .clone()
        .ok_or_else(|| ApiError::new(ErrorCode::Invalid, "this server keeps no snapshots"))
}

/// The server's snapshots, newest first.
pub fn list(server: &Server) -> Result<Vec<SnapshotInfo>, ApiError> {
    let snaps = snapshots::list(server.planner.db().conn()).map_err(core)?;
    Ok(snaps.iter().map(SnapshotInfo::from).collect())
}

/// Takes a manual snapshot (kept until deleted by hand).
pub fn snapshot(server: &Server, note: Option<String>) -> Result<SnapshotInfo, ApiError> {
    let path = planner_path(server)?;
    let conn = server.planner.db().conn();
    let dir = snapshots::dir(conn, &path).map_err(core)?;
    let snap =
        snapshots::create(conn, &dir, Reason::Manual, note, server.planner.now()).map_err(core)?;
    tracing::info!(id = %snap.id, "snapshot taken");
    Ok(SnapshotInfo::from(&snap))
}

/// Restores snapshot `id` (after a "before restore" snapshot) and starts a
/// new epoch.
pub fn restore(server: &mut Server, id: &str) -> Result<Replaced, ApiError> {
    let path = planner_path(server)?;
    let now = server.planner.now();
    let before = snapshots::restore(server.planner.db_mut(), &path, id, now).map_err(core)?;
    let epoch = start_over(server)?;
    tracing::info!(id, %epoch, "snapshot restored; devices will bootstrap");
    Ok(Replaced {
        epoch,
        before: SnapshotInfo::from(&before),
    })
}

/// Replaces the planner with a JSON export (after a "before import"
/// snapshot) and starts a new epoch.
pub fn import(server: &mut Server, e: &Export) -> Result<Replaced, ApiError> {
    let path = planner_path(server)?;
    let now = server.planner.now();
    let before = export::import(server.planner.db_mut(), &path, e, now).map_err(core)?;
    let epoch = start_over(server)?;
    tracing::info!(%epoch, "export imported; devices will bootstrap");
    Ok(Replaced {
        epoch,
        before: SnapshotInfo::from(&before),
    })
}

/// After the planner was replaced: a new epoch, an empty change log (every
/// device bootstraps the new planner whole), nothing left pending, and a
/// notice on the change stream.
fn start_over(server: &mut Server) -> Result<String, ApiError> {
    let conn = server.planner.db().conn();
    let pending = capture::pending(conn).map_err(core)?;
    capture::acknowledge(conn, &pending).map_err(core)?;
    let store = server.store.conn();
    store
        .execute("DELETE FROM log", [])
        .map_err(ApiError::internal)?;
    store
        .execute("DELETE FROM pushes", [])
        .map_err(ApiError::internal)?;
    let epoch = server.store.new_epoch().map_err(ApiError::internal)?;
    server.announce(changes::current_seq(&server.store)?)?;
    Ok(epoch)
}
