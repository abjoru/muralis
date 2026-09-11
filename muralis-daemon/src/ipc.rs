use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{broadcast, mpsc, oneshot};
use tracing::{info, warn};

use muralis_core::ipc::{DaemonEvent, IpcRequest, IpcResponse};
use muralis_core::paths::MuralisPaths;

use crate::display::DaemonCommand;

pub async fn serve_ipc(
    cmd_tx: mpsc::Sender<DaemonCommand>,
    events: broadcast::Sender<DaemonEvent>,
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
                        let events = events.clone();
                        let shutdown = shutdown.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, tx, events, shutdown).await {
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
    events: broadcast::Sender<DaemonEvent>,
    shutdown: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    // Subscribe before the request has even been read. An event fired between
    // parsing `subscribe` and subscribing would land in the gap between the
    // snapshot and the stream and be lost for good; subscribing first can only
    // duplicate an event the snapshot also carries, and a Consumer re-applying
    // the wallpaper it already has is harmless where missing one is not.
    let event_rx = events.subscribe();
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

    // The one request that is not request/response: it keeps the connection
    // and writes events until the Consumer goes away.
    if matches!(request, IpcRequest::Subscribe) {
        return stream_events(writer, event_rx, &cmd_tx, shutdown).await;
    }

    let response = dispatch_request(request, &cmd_tx).await;

    let mut resp_line = serde_json::to_string(&response)?;
    resp_line.push('\n');
    writer.write_all(resp_line.as_bytes()).await?;
    Ok(())
}

/// Serve one **subscription**: a snapshot of daemon state as events, then
/// every event the engine emits from here on.
///
/// A write that fails is the Consumer having hung up — the ordinary end of a
/// subscription, not an error worth logging.
async fn stream_events(
    mut writer: tokio::net::unix::OwnedWriteHalf,
    mut event_rx: broadcast::Receiver<DaemonEvent>,
    cmd_tx: &mpsc::Sender<DaemonCommand>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<()> {
    info!("subscriber connected");
    if !write_snapshot(&mut writer, cmd_tx).await? {
        return Ok(());
    }

    loop {
        tokio::select! {
            received = event_rx.recv() => match received {
                Ok(event) => {
                    if !write_event(&mut writer, &event).await? {
                        break;
                    }
                }
                // The subscriber fell far enough behind that the channel
                // dropped events on its behalf. Its view is now stale in ways
                // it cannot know, so resend the snapshot rather than carry on
                // from a gap.
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    warn!(missed, "subscriber fell behind; resyncing");
                    if !write_snapshot(&mut writer, cmd_tx).await? {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
            _ = shutdown.changed() => break,
        }
    }

    info!("subscriber disconnected");
    Ok(())
}

/// Ask the engine for its current state and write it out as events. `false`
/// means the connection is gone (or the engine is), and the caller should stop.
async fn write_snapshot(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    cmd_tx: &mpsc::Sender<DaemonCommand>,
) -> anyhow::Result<bool> {
    let (tx, rx) = oneshot::channel();
    if cmd_tx
        .send(DaemonCommand::Snapshot { respond: tx })
        .await
        .is_err()
    {
        return Ok(false);
    }
    let Ok(snapshot) = rx.await else {
        return Ok(false);
    };

    for event in snapshot {
        if !write_event(writer, &event).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// One newline-terminated JSON event. `false` means the peer is gone.
async fn write_event(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    event: &DaemonEvent,
) -> anyhow::Result<bool> {
    let mut line = serde_json::to_string(event)?;
    line.push('\n');
    Ok(writer.write_all(line.as_bytes()).await.is_ok())
}

async fn dispatch_request(
    request: IpcRequest,
    cmd_tx: &mpsc::Sender<DaemonCommand>,
) -> IpcResponse {
    match request {
        IpcRequest::Status => {
            let (tx, rx) = oneshot::channel();
            if cmd_tx
                .send(DaemonCommand::Status { respond: tx })
                .await
                .is_err()
            {
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
        IpcRequest::Favorites { offset, limit } => {
            let (tx, rx) = oneshot::channel();
            if cmd_tx
                .send(DaemonCommand::Favorites {
                    offset,
                    limit,
                    respond: tx,
                })
                .await
                .is_err()
            {
                return IpcResponse::error("engine unavailable");
            }
            match rx.await {
                Ok(Ok(page)) => {
                    IpcResponse::ok_with_data(serde_json::to_value(page).unwrap_or_default())
                }
                Ok(Err(msg)) => IpcResponse::error(msg),
                Err(_) => IpcResponse::error("engine dropped response"),
            }
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
            let (tx, rx) = oneshot::channel();
            if cmd_tx
                .send(DaemonCommand::SetMode { mode, respond: tx })
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
        // Handled by `handle_connection` before dispatch: it is the one
        // request that never produces a response.
        IpcRequest::Subscribe => IpcResponse::error("subscribe is a streaming request"),
        IpcRequest::Quit => {
            let _ = cmd_tx.send(DaemonCommand::Quit).await;
            IpcResponse::ok()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use muralis_core::ipc::FavoritesPage;
    use muralis_core::models::{SourceType, Wallpaper};

    /// Drives one request through the dispatcher against a stand-in engine that
    /// answers `Favorites` with a fixed page.
    async fn dispatch_line(line: &str) -> IpcResponse {
        let (cmd_tx, mut cmd_rx) = mpsc::channel(1);
        tokio::spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                if let DaemonCommand::Favorites {
                    offset, respond, ..
                } = cmd
                {
                    let _ = respond.send(Ok(FavoritesPage {
                        wallpapers: Vec::new(),
                        total: 7,
                        offset: offset.unwrap_or(0),
                    }));
                }
            }
        });
        let request: IpcRequest = serde_json::from_str(line).unwrap();
        dispatch_request(request, &cmd_tx).await
    }

    #[tokio::test]
    async fn favorites_answers_a_page_on_the_wire() {
        let resp = dispatch_line(r#"{"command":"favorites","offset":16,"limit":16}"#).await;

        let data = match resp {
            IpcResponse::Ok { data: Some(data) } => data,
            other => panic!("expected a served page, got {other:?}"),
        };
        let page: FavoritesPage = serde_json::from_value(data).unwrap();
        assert_eq!(page.total, 7);
        assert_eq!(
            page.offset, 16,
            "the window the Consumer asked for comes back"
        );
    }

    fn a_wallpaper(id: &str) -> Wallpaper {
        Wallpaper {
            id: id.into(),
            source_type: SourceType::new("test"),
            source_id: id.into(),
            source_url: None,
            width: 5120,
            height: 1440,
            tags: Vec::new(),
            file_path: format!("/tmp/{id}.jpg"),
            added_at: "2026-09-10T00:00:00Z".into(),
            last_used: None,
            use_count: 0,
        }
    }

    /// A stand-in engine whose snapshot is one wallpaper plus its mode and
    /// pause state, like the real one's.
    fn engine_answering_snapshot() -> mpsc::Sender<DaemonCommand> {
        let (cmd_tx, mut cmd_rx) = mpsc::channel(4);
        tokio::spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                if let DaemonCommand::Snapshot { respond } = cmd {
                    let _ = respond.send(vec![
                        DaemonEvent::WallpaperChanged {
                            wallpaper: Box::new(a_wallpaper("on-screen")),
                        },
                        DaemonEvent::ModeChanged {
                            mode: muralis_core::models::DisplayMode::Random,
                        },
                        DaemonEvent::PauseChanged { paused: false },
                    ]);
                }
            }
        });
        cmd_tx
    }

    /// Serves one `subscribe` connection and hands back the client's read half.
    async fn subscribed(
        events: broadcast::Sender<DaemonEvent>,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> BufReader<tokio::net::unix::OwnedReadHalf> {
        let (client, server) = tokio::net::UnixStream::pair().unwrap();
        let cmd_tx = engine_answering_snapshot();
        tokio::spawn(async move {
            handle_connection(server, cmd_tx, events, shutdown)
                .await
                .unwrap();
        });

        let (reader, mut writer) = client.into_split();
        writer
            .write_all(b"{\"command\":\"subscribe\"}\n")
            .await
            .unwrap();
        BufReader::new(reader)
    }

    async fn next_event(reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>) -> DaemonEvent {
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        serde_json::from_str(line.trim()).unwrap_or_else(|e| panic!("not an event: {line:?} ({e})"))
    }

    #[tokio::test]
    async fn a_subscriber_is_caught_up_before_it_is_streamed_to() {
        let (events, _) = broadcast::channel(16);
        let (_shutdown_tx, shutdown) = tokio::sync::watch::channel(false);
        let mut reader = subscribed(events.clone(), shutdown).await;

        // The snapshot, in full, before anything new happens.
        assert!(matches!(
            next_event(&mut reader).await,
            DaemonEvent::WallpaperChanged { wallpaper } if wallpaper.id == "on-screen"
        ));
        assert!(matches!(
            next_event(&mut reader).await,
            DaemonEvent::ModeChanged { .. }
        ));
        assert!(matches!(
            next_event(&mut reader).await,
            DaemonEvent::PauseChanged { paused: false }
        ));

        // Then the same connection carries what the engine does next — this is
        // the framing change: no shutdown(), no second connection.
        events
            .send(DaemonEvent::WallpaperChanged {
                wallpaper: Box::new(a_wallpaper("rotated-to")),
            })
            .unwrap();
        events
            .send(DaemonEvent::PauseChanged { paused: true })
            .unwrap();

        assert!(matches!(
            next_event(&mut reader).await,
            DaemonEvent::WallpaperChanged { wallpaper } if wallpaper.id == "rotated-to"
        ));
        assert!(matches!(
            next_event(&mut reader).await,
            DaemonEvent::PauseChanged { paused: true }
        ));
    }

    #[tokio::test]
    async fn a_one_shot_command_still_gets_one_line_and_a_close() {
        // Holding subscriptions open must not have changed the shape every
        // script and keybind depends on.
        let (client, server) = tokio::net::UnixStream::pair().unwrap();
        let (events, _) = broadcast::channel(16);
        let (_shutdown_tx, shutdown) = tokio::sync::watch::channel(false);
        let cmd_tx = engine_answering_snapshot();
        tokio::spawn(async move {
            handle_connection(server, cmd_tx, events, shutdown)
                .await
                .unwrap();
        });

        let (reader, mut writer) = client.into_split();
        writer.write_all(b"{\"command\":\"next\"}\n").await.unwrap();

        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert_eq!(line.trim(), r#"{"status":"ok"}"#);

        line.clear();
        assert_eq!(
            reader.read_line(&mut line).await.unwrap(),
            0,
            "the daemon hangs up after one response"
        );
    }

    #[tokio::test]
    async fn a_daemon_shutting_down_ends_the_subscription() {
        let (events, _) = broadcast::channel(16);
        let (shutdown_tx, shutdown) = tokio::sync::watch::channel(false);
        let mut reader = subscribed(events, shutdown).await;
        for _ in 0..3 {
            next_event(&mut reader).await;
        }

        shutdown_tx.send(true).unwrap();

        let mut line = String::new();
        assert_eq!(
            reader.read_line(&mut line).await.unwrap(),
            0,
            "a subscriber must see the socket close, not hang on a dead daemon"
        );
    }
}
