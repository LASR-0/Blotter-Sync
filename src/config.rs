//! Where the server keeps its data.

use std::path::PathBuf;

/// The canonical planner, as the app names its own database.
pub const PLANNER_FILE: &str = blotter_core::db::DB_FILE_NAME;
/// Server-only state: devices, pairing, the change log.
pub const STATE_FILE: &str = "server.db";

/// The default data directory: `$XDG_DATA_HOME/blotter-sync` (or
/// `~/.local/share/blotter-sync`) on Linux and other Unixes,
/// `%ProgramData%\Blotter Sync` on Windows. Docker sets
/// `BLOTTER_SYNC_DATA_DIR=/data` instead.
pub fn default_data_dir() -> PathBuf {
    if cfg!(windows) {
        let base = std::env::var_os("ProgramData")
            .map_or_else(|| PathBuf::from(r"C:\ProgramData"), PathBuf::from);
        return base.join("Blotter Sync");
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(xdg).join("blotter-sync");
    }
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
    home.join(".local").join("share").join("blotter-sync")
}

/// Creates `dir` if needed, readable only by the user running the server
/// on Unix (mode 0700).
pub fn prepare_data_dir(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_ends_in_the_servers_own_folder() {
        let dir = default_data_dir();
        let last = dir.file_name().unwrap().to_string_lossy().into_owned();
        assert!(last == "blotter-sync" || last == "Blotter Sync", "{dir:?}");
    }

    #[test]
    fn preparing_creates_nested_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("a").join("b");
        prepare_data_dir(&dir).unwrap();
        assert!(dir.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
    }
}
