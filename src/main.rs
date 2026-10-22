use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use blotter_sync::auth;
use blotter_sync::server::Server;
use blotter_sync::store::Store;
use blotter_sync::{DEFAULT_PORT, VERSION, config, router};
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
}

#[derive(clap::Args, Default)]
struct ServeArgs {
    /// Address and port to listen on.
    #[arg(long, env = "BLOTTER_SYNC_LISTEN")]
    listen: Option<SocketAddr>,
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("BLOTTER_SYNC_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let data_dir = cli.data_dir.unwrap_or_else(config::default_data_dir);
    let result = match cli.command.unwrap_or(Command::Serve(ServeArgs::default())) {
        Command::Serve(args) => serve(&data_dir, args).await,
        Command::Pair => pair(&data_dir),
        Command::Devices => list_devices(&data_dir),
        Command::Revoke { device } => revoke(&data_dir, &device),
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
    let addr = args
        .listen
        .unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], DEFAULT_PORT)));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(version = VERSION, %addr, data = %data_dir.display(), "blotter-sync listening");
    let app = router(server.shared()).into_make_service_with_connect_info::<SocketAddr>();
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
