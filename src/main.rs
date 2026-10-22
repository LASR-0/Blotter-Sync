use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use blotter_sync::server::Server;
use blotter_sync::store::Store;
use blotter_sync::{DEFAULT_PORT, VERSION, config, router};
use blotter_sync::{admin, auth, duties};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

type Failure = Box<dyn std::error::Error>;

#[derive(Parser)]
#[command(
    name = "blotter-sync",
    version,
    about = "Keeps every Blotter desktop's planner in sync"
)]
struct Cli {
    /// Where the planner and the server's state are kept.
    #[arg(long, global = true, env = "BLOTTER_SYNC_DATA_DIR")]
    data_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the server (the default when no command is given).
    Serve(ServeArgs),
    /// Open a pairing code for a new device (single use, 10 minutes).
    Pair,
    /// List the devices paired with this server.
    Devices,
    /// Disconnect a device, by name or id; it can pair again later.
    Revoke {
        /// The device's name (as `devices` lists it) or id.
        device: String,
    },
    /// Take a snapshot of the planner now (kept until you delete it).
    Snapshot {
        /// A few words to tell it apart later.
        #[arg(long)]
        note: Option<String>,
    },
    /// List the planner's snapshots.
    Snapshots,
    /// Restore a snapshot; every device then fetches the planner again.
    Restore {
        /// The snapshot's id, as `snapshots` lists it.
        id: String,
    },
}

#[derive(clap::Args, Default)]
struct ServeArgs {
    /// Address and port to listen on.
    #[arg(long, env = "BLOTTER_SYNC_LISTEN")]
    listen: Option<SocketAddr>,
    /// Daily backups of the planner to keep (manual snapshots are always kept).
    #[arg(long, env = "BLOTTER_SYNC_KEEP_BACKUPS")]
    keep_backups: Option<u32>,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    // The server logs its work; the admin commands print their answer and
    // only warnings besides.
    let serving = matches!(cli.command, None | Some(Command::Serve(_)));
    let default = if serving { "info" } else { "warn" };
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("BLOTTER_SYNC_LOG").unwrap_or_else(|_| EnvFilter::new(default)),
        )
        .init();

    let data_dir = cli.data_dir.unwrap_or_else(config::default_data_dir);
    let result = match cli.command.unwrap_or(Command::Serve(ServeArgs::default())) {
        Command::Serve(args) => serve(&data_dir, args).await,
        Command::Pair => pair(&data_dir),
        Command::Devices => list_devices(&data_dir),
        Command::Revoke { device } => revoke(&data_dir, &device),
        Command::Snapshot { note } => snapshot(&data_dir, note),
        Command::Snapshots => list_snapshots(&data_dir),
        Command::Restore { id } => restore(&data_dir, &id),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!("{err}");
            ExitCode::FAILURE
        }
    }
}

async fn serve(data_dir: &Path, args: ServeArgs) -> Result<(), Failure> {
    let server = Server::open(data_dir)?;
    if let Some(keep) = args.keep_backups {
        blotter_core::store::settings::set(
            server.planner.db().conn(),
            blotter_core::settings::KEY_SNAPSHOT_KEEP,
            &keep,
            chrono::Utc::now(),
        )?;
    }
    let shared = server.shared();
    tokio::spawn(duties::run(shared.clone()));
    let addr = args
        .listen
        .unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], DEFAULT_PORT)));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(version = VERSION, %addr, data = %data_dir.display(), "blotter-sync listening");
    let app = router(shared).into_make_service_with_connect_info::<SocketAddr>();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// The server's own state, for the admin commands; they work while the
/// server runs (both open the same file in WAL mode).
fn store(data_dir: &Path) -> Result<Store, Failure> {
    config::prepare_data_dir(data_dir)?;
    Ok(Store::open(&data_dir.join(config::STATE_FILE))?)
}

fn pair(data_dir: &Path) -> Result<(), Failure> {
    let open = auth::open_pairing(&store(data_dir)?, chrono::Utc::now())?;
    let local = open.expires_at.with_timezone(&chrono::Local);
    println!("Pairing code: {}", open.code);
    println!(
        "Enter it in Blotter (Settings → Sync) before {}. It works once.",
        local.format("%H:%M")
    );
    Ok(())
}

fn list_devices(data_dir: &Path) -> Result<(), Failure> {
    let devices = auth::devices(&store(data_dir)?)?;
    if devices.is_empty() {
        println!("No devices yet. Run `blotter-sync pair` to add one.");
        return Ok(());
    }
    let day = |t: &Option<String>| {
        t.as_deref()
            .map_or("—", |t| t.get(..10).unwrap_or(t))
            .to_owned()
    };
    println!(
        "{:<24} {:<26} {:<11} {:<11} STATUS",
        "NAME", "ID", "PAIRED", "LAST SEEN"
    );
    for d in devices {
        let status = match (&d.revoked_at, d.clock_ahead_ms) {
            (Some(_), _) => "revoked".to_owned(),
            (None, Some(ms)) => format!("clock {}s ahead", ms / 1000),
            (None, None) => "ok".to_owned(),
        };
        println!(
            "{:<24} {:<26} {:<11} {:<11} {status}",
            d.name,
            d.device_id,
            day(&Some(d.paired_at.clone())),
            day(&d.last_seen_at),
        );
    }
    Ok(())
}

fn revoke(data_dir: &Path, which: &str) -> Result<(), Failure> {
    let device = auth::revoke(&store(data_dir)?, which, chrono::Utc::now())?;
    println!("Disconnected {} ({}).", device.name, device.device_id);
    Ok(())
}

/// Resolves on Ctrl+C everywhere, and on SIGTERM on Unix (docker stop, systemd).
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutting down");
}

fn snapshot(data_dir: &Path, note: Option<String>) -> Result<(), Failure> {
    let snap = admin::snapshot(&Server::open(data_dir)?, note)?;
    println!("Snapshot {} taken.", snap.id);
    Ok(())
}

fn list_snapshots(data_dir: &Path) -> Result<(), Failure> {
    let snaps = admin::list(&Server::open(data_dir)?)?;
    if snaps.is_empty() {
        println!("No snapshots yet; the first daily one comes soon after the server starts.");
        return Ok(());
    }
    println!(
        "{:<26} {:<20} {:<15} {:>9} NOTE",
        "ID", "TAKEN (UTC)", "REASON", "SIZE"
    );
    for s in snaps {
        let size = s
            .size_bytes
            .map_or_else(|| "—".to_owned(), |b| format!("{} KB", b / 1024));
        println!(
            "{:<26} {:<20} {:<15} {:>9} {}",
            s.id,
            s.created_at.replace('T', " ").trim_end_matches('Z'),
            s.reason,
            size,
            s.note.unwrap_or_default()
        );
    }
    Ok(())
}

fn restore(data_dir: &Path, id: &str) -> Result<(), Failure> {
    let done = admin::restore(&mut Server::open(data_dir)?, id)?;
    println!(
        "Restored. Every device will fetch the planner again (epoch {}). \
         The planner as it was is snapshot {}.",
        done.epoch, done.before.id
    );
    Ok(())
}
