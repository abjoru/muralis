mod display;
mod ipc;
mod tray;
mod workspace;

use tokio::sync::{mpsc, watch};
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

    let config = Config::load_or_default(&paths);
    info!(backend = %config.general.backend, mode = %config.display.mode, "starting muralis-daemon");

    let backend = create_backend(&config);
    let (cmd_tx, cmd_rx) = mpsc::channel(32);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // spawn tray
    tray::spawn_tray(cmd_tx.clone());

    // spawn workspace listener
    let ws_tx = cmd_tx.clone();
    tokio::spawn(async move {
        workspace::listen_workspace_events(ws_tx).await;
    });

    // spawn IPC server
    let ipc_shutdown = shutdown_rx.clone();
    let ipc_tx = cmd_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = ipc::serve_ipc(ipc_tx, ipc_shutdown).await {
            tracing::error!("IPC server error: {e}");
        }
    });

    // spawn display engine
    let engine = DisplayEngine::new(config, paths.clone(), backend);
    let engine_shutdown = shutdown_rx.clone();
    let engine_handle = tokio::spawn(async move {
        engine.run(cmd_rx, engine_shutdown).await;
    });

    // wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    info!("received ctrl+c, shutting down");
    let _ = shutdown_tx.send(true);

    // wait for engine to finish
    let _ = engine_handle.await;

    // clean up socket
    let socket = MuralisPaths::socket_path();
    if socket.exists() {
        let _ = std::fs::remove_file(socket);
    }

    info!("muralis-daemon stopped");
    Ok(())
}
