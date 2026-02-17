use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};

use muralis_core::ipc::{IpcRequest, IpcResponse};
use muralis_core::paths::MuralisPaths;

use crate::display::DaemonCommand;

pub async fn serve_ipc(
    cmd_tx: mpsc::Sender<DaemonCommand>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let socket_path = MuralisPaths::socket_path();

    // clean up stale socket
    if socket_path.exists() {
        std::fs::remove_file(&socket_path)?;
    }

    let listener = UnixListener::bind(&socket_path)?;
    info!(path = %socket_path.display(), "IPC socket listening");

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _)) => {
                        let tx = cmd_tx.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, tx).await {
                                warn!("IPC connection error: {e}");
                            }
                        });
                    }
                    Err(e) => warn!("IPC accept error: {e}"),
                }
            }
            _ = shutdown.changed() => {
                info!("IPC server shutting down");
                let _ = std::fs::remove_file(&socket_path);
                return Ok(());
            }
        }
    }
}

async fn handle_connection(
    stream: tokio::net::UnixStream,
    cmd_tx: mpsc::Sender<DaemonCommand>,
) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();
    buf_reader.read_line(&mut line).await?;

    let request: IpcRequest = match serde_json::from_str(line.trim()) {
        Ok(r) => r,
        Err(e) => {
            let resp = IpcResponse::error(format!("invalid request: {e}"));
            let mut resp_line = serde_json::to_string(&resp)?;
            resp_line.push('\n');
            writer.write_all(resp_line.as_bytes()).await?;
            return Ok(());
        }
    };

    let response = dispatch_request(request, &cmd_tx).await;

    let mut resp_line = serde_json::to_string(&response)?;
    resp_line.push('\n');
    writer.write_all(resp_line.as_bytes()).await?;
    Ok(())
}

async fn dispatch_request(
    request: IpcRequest,
    cmd_tx: &mpsc::Sender<DaemonCommand>,
) -> IpcResponse {
    match request {
        IpcRequest::Status => {
            let (tx, rx) = oneshot::channel();
            if cmd_tx.send(DaemonCommand::Status { respond: tx }).await.is_err() {
                return IpcResponse::error("engine unavailable");
            }
            match rx.await {
                Ok(status) => {
                    IpcResponse::ok_with_data(serde_json::to_value(status).unwrap_or_default())
                }
                Err(_) => IpcResponse::error("engine dropped response"),
            }
        }
        IpcRequest::Next => {
            let _ = cmd_tx.send(DaemonCommand::Next).await;
            IpcResponse::ok()
        }
        IpcRequest::Prev => {
            let _ = cmd_tx.send(DaemonCommand::Prev).await;
            IpcResponse::ok()
        }
        IpcRequest::SetWallpaper { id } => {
            let (tx, rx) = oneshot::channel();
            if cmd_tx
                .send(DaemonCommand::SetWallpaper { id, respond: tx })
                .await
                .is_err()
            {
                return IpcResponse::error("engine unavailable");
            }
            match rx.await {
                Ok(Ok(())) => IpcResponse::ok(),
                Ok(Err(msg)) => IpcResponse::error(msg),
                Err(_) => IpcResponse::error("engine dropped response"),
            }
        }
        IpcRequest::SetMode { mode } => {
            let _ = cmd_tx.send(DaemonCommand::SetMode { mode }).await;
            IpcResponse::ok()
        }
        IpcRequest::Pause => {
            let _ = cmd_tx.send(DaemonCommand::Pause).await;
            IpcResponse::ok()
        }
        IpcRequest::Resume => {
            let _ = cmd_tx.send(DaemonCommand::Resume).await;
            IpcResponse::ok()
        }
        IpcRequest::Reload => {
            let _ = cmd_tx.send(DaemonCommand::Reload).await;
            IpcResponse::ok()
        }
        IpcRequest::Quit => {
            let _ = cmd_tx.send(DaemonCommand::Quit).await;
            IpcResponse::ok()
        }
    }
}
