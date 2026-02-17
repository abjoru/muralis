use futures_lite::StreamExt;
use hyprland::event_listener::{Event, EventStream};
use tokio::sync::mpsc;
use tracing::{debug, error};

use crate::display::DaemonCommand;

/// Listens for Hyprland workspace change events and forwards them as DaemonCommands.
pub async fn listen_workspace_events(cmd_tx: mpsc::Sender<DaemonCommand>) {
    let mut stream = EventStream::new();

    while let Some(event) = stream.next().await {
        match event {
            Ok(Event::WorkspaceChanged(data)) => {
                let id = data.id;
                if id < 0 {
                    debug!(id, "ignoring special workspace");
                    continue;
                }
                debug!(id, "workspace changed");
                if cmd_tx
                    .send(DaemonCommand::WorkspaceChanged { id: id as u32 })
                    .await
                    .is_err()
                {
                    error!("cmd channel closed, stopping workspace listener");
                    break;
                }
            }
            Ok(_) => {} // ignore other events
            Err(e) => {
                error!("hyprland event error: {e}");
                break;
            }
        }
    }
}
