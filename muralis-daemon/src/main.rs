mod display;
mod ipc;
mod workspace;

use tokio::sync::{broadcast, mpsc, watch};
use tracing::info;

use muralis_core::backend::create_backend;
use muralis_core::config::Config;
use muralis_core::paths::MuralisPaths;

use display::engine::DisplayEngine;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "muralis_daemon=info".into()),
        )
        .init();

    let paths = MuralisPaths::new()?;
    paths.ensure_dirs()?;
    let _ = paths.install_icon();

    let config = Config::load_or_default(&paths);
    info!(backend = %config.general.backend, mode = %config.display.mode, "starting muralis-daemon");

    let socket_path = MuralisPaths::socket_path();
    let backend = create_backend(&config);
    let (cmd_tx, cmd_rx) = mpsc::channel(32);
    // Wallpaper changes are rare and subscribers few; the buffer only has to
    // absorb a burst while a Consumer is busy repainting. One that falls
    // further behind than this is resynced from a snapshot, not disconnected.
    let (event_tx, _) = broadcast::channel(64);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // spawn workspace listener
    let ws_tx = cmd_tx.clone();
    tokio::spawn(async move {
        workspace::listen_workspace_events(ws_tx).await;
    });

    // spawn IPC server
    let ipc_shutdown = shutdown_rx.clone();
    let ipc_tx = cmd_tx.clone();
    let ipc_events = event_tx.clone();
    let ipc_socket = socket_path.clone();
    tokio::spawn(async move {
        if let Err(e) = ipc::serve_ipc(ipc_socket, ipc_tx, ipc_events, ipc_shutdown).await {
            tracing::error!("IPC server error: {e}");
        }
    });

    // spawn display engine
    let engine = DisplayEngine::new(config, paths.clone(), backend, event_tx);
    let engine_shutdown = shutdown_rx.clone();
    let engine_handle = tokio::spawn(async move {
        engine.run(cmd_rx, engine_shutdown).await;
    });

    // wait for shutdown signal
    let signal = shutdown_signal().await?;
    info!(signal, "shutting down");
    let _ = shutdown_tx.send(true);

    // wait for engine to finish
    let _ = engine_handle.await;

    // Belt and braces: serve_ipc unlinks the socket on the same signal, but
    // it may have died before ever getting there.
    if socket_path.exists() {
        let _ = std::fs::remove_file(&socket_path);
    }

    info!("muralis-daemon stopped");
    Ok(())
}

/// Wait for a signal that means stop, and say which one arrived.
///
/// Answering only ctrl+c meant every ordinary shutdown skipped both cleanup
/// paths: Hyprland's `exec-once` children get SIGTERM at session end, as would
/// anything under systemd, so the socket outlived every logout. It also matters
/// more since subscriptions landed — the watch channel this drives is what ends
/// an open **Subscription** in an orderly way, rather than by process death.
async fn shutdown_signal() -> anyhow::Result<&'static str> {
    use tokio::signal::unix::{signal, SignalKind};

    let mut terminate = signal(SignalKind::terminate())?;

    tokio::select! {
        result = tokio::signal::ctrl_c() => {
            result?;
            Ok("interrupt")
        }
        _ = terminate.recv() => Ok("terminate"),
    }
}
