//! The server's state: the canonical planner and the server-only store.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use blotter_core::sync::capture::{self, Role};
use blotter_core::{Database, Planner};

use crate::api::{ApiError, Hello, Versions};
use crate::config::{PLANNER_FILE, STATE_FILE, prepare_data_dir};
use crate::store::{self, Store};

#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("couldn't prepare the data directory {0}: {1}")]
    DataDir(PathBuf, std::io::Error),
    #[error("the planner: {0}")]
    Planner(#[from] blotter_core::CoreError),
    #[error("the server state: {0}")]
    Store(#[from] store::StoreError),
}

/// One server: its planner (a replica like any device's, with the server
/// role) and its own state.
pub struct Server {
    pub planner: Planner,
    pub store: Store,
}

/// Shared by every request. Database work is short; handlers run it on the
/// blocking pool so the async runtime keeps serving.
pub type Shared = Arc<Mutex<Server>>;

impl Server {
    /// Opens (creating if needed) the server in `dir`.
    pub fn open(dir: &Path) -> Result<Self, OpenError> {
        prepare_data_dir(dir).map_err(|e| OpenError::DataDir(dir.to_owned(), e))?;
        let store = Store::open(&dir.join(STATE_FILE))?;
        let db = Database::open(&dir.join(PLANNER_FILE))?;
        Self::with(db, store)
    }

    /// A server held in memory, for tests.
    pub fn open_in_memory() -> Result<Self, OpenError> {
        Self::with(Database::open_in_memory()?, Store::open_in_memory()?)
    }

    fn with(mut db: Database, store: Store) -> Result<Self, OpenError> {
        // The server also captures what it owns (fetch status, the event
        // cache, rollover bookkeeping), so devices receive it.
        capture::set_role(db.conn_mut(), Role::Server)?;
        // The planner's own `timezone` setting (synced from the devices)
        // decides days; UTC only until one has been set.
        let planner = Planner::new(db, chrono_tz::UTC);
        Ok(Self { planner, store })
    }

    pub fn shared(self) -> Shared {
        Arc::new(Mutex::new(self))
    }

    pub fn hello(&self) -> Result<Hello, ApiError> {
        let meta = self.store.meta().map_err(ApiError::internal)?;
        Ok(Hello {
            name: "blotter-sync".into(),
            version: crate::VERSION.into(),
            blotter_core: blotter_core::VERSION.into(),
            server_id: meta.server_id,
            versions: Versions::SERVER,
            epoch: meta.epoch,
            pairing_open: false,
        })
    }
}

/// Runs `work` against the server on the blocking pool.
pub async fn with<T: Send + 'static>(
    shared: &Shared,
    work: impl FnOnce(&mut Server) -> Result<T, ApiError> + Send + 'static,
) -> Result<T, ApiError> {
    let shared = Arc::clone(shared);
    tokio::task::spawn_blocking(move || {
        let mut server = shared
            .lock()
            .map_err(|_| ApiError::internal("the server state was poisoned by an earlier crash"))?;
        work(&mut server)
    })
    .await
    .map_err(ApiError::internal)?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_data_directory_gets_both_databases_and_the_server_role() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let server = Server::open(&data).unwrap();
        assert!(data.join(STATE_FILE).is_file());
        assert!(data.join(PLANNER_FILE).is_file());
        assert_eq!(
            capture::role(server.planner.db().conn()).unwrap(),
            Role::Server
        );
        let first = server.hello().unwrap();
        drop(server);
        let again = Server::open(&data).unwrap().hello().unwrap();
        assert_eq!(again.server_id, first.server_id);
        assert_eq!(again.epoch, first.epoch);
    }
}
