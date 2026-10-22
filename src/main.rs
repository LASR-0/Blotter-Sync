use std::net::SocketAddr;
use std::process::ExitCode;

use blotter_sync::{DEFAULT_PORT, VERSION, router};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "blotter-sync",
    version,
    about = "Keeps every Blotter desktop's planner in sync"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the server (the default when no command is given).
    Serve(ServeArgs),
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
    let result = match cli.command.unwrap_or(Command::Serve(ServeArgs::default())) {
        Command::Serve(args) => serve(args).await,
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!("{err}");
            ExitCode::FAILURE
        }
    }
}

async fn serve(args: ServeArgs) -> std::io::Result<()> {
    let addr = args
        .listen
        .unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], DEFAULT_PORT)));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(version = VERSION, %addr, "blotter-sync listening");
    axum::serve(listener, router())
        .with_graceful_shutdown(shutdown_signal())
        .await
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
