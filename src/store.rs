//! Server-only state, in its own database beside the planner, with its own
//! migrations (so the core's migration list stays the app's).

use std::path::Path;

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

/// Ordered schema migrations. Append new ones; never edit a shipped one.
const MIGRATION_LIST: &[M<'static>] = &[M::up(include_str!("../migrations/0001_meta.sql"))];
const MIGRATIONS: Migrations<'static> = Migrations::from_slice(MIGRATION_LIST);

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("migration failed: {0}")]
    Migration(#[from] rusqlite_migration::Error),
}

pub type Result<T, E = StoreError> = std::result::Result<T, E>;

/// Who this server is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    /// Made once, when the data directory is first used.
    pub server_id: String,
    /// Changes whenever the planner is replaced wholesale (restore, import),
    /// telling every device to bootstrap again.
    pub epoch: String,
}

#[derive(Debug)]
pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        MIGRATIONS.to_latest(&mut conn)?;
        let id = ulid::Ulid::generate().to_string();
        conn.execute(
            "INSERT OR IGNORE INTO meta (id, server_id, epoch, created_at) \
             VALUES (1, ?1, ?2, ?3)",
            (
                &id,
                ulid::Ulid::generate().to_string(),
                chrono::Utc::now().to_rfc3339(),
            ),
        )?;
        Ok(Self { conn })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn meta(&self) -> Result<Meta> {
        Ok(self
            .conn
            .query_row("SELECT server_id, epoch FROM meta WHERE id = 1", [], |r| {
                Ok(Meta {
                    server_id: r.get(0)?,
                    epoch: r.get(1)?,
                })
            })?)
    }

    /// Starts a new epoch (after the planner was replaced) and returns it.
    pub fn new_epoch(&self) -> Result<String> {
        let epoch = ulid::Ulid::generate().to_string();
        self.conn
            .execute("UPDATE meta SET epoch = ?1 WHERE id = 1", [&epoch])?;
        Ok(epoch)
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_valid() {
        MIGRATIONS.validate().unwrap();
    }

    #[test]
    fn identity_is_made_once_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("server.db");
        let first = Store::open(&path).unwrap().meta().unwrap();
        assert_eq!(first.server_id.len(), 26);
        let again = Store::open(&path).unwrap().meta().unwrap();
        assert_eq!(again, first);
    }

    #[test]
    fn a_new_epoch_keeps_the_server_id() {
        let store = Store::open_in_memory().unwrap();
        let before = store.meta().unwrap();
        let epoch = store.new_epoch().unwrap();
        let after = store.meta().unwrap();
        assert_ne!(epoch, before.epoch);
        assert_eq!(after.epoch, epoch);
        assert_eq!(after.server_id, before.server_id);
    }
}
