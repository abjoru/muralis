pub mod engine;
pub mod scheduler;

use tokio::sync::oneshot;

use muralis_core::ipc::{DaemonEvent, DaemonStatus, FavoritesPage};
use muralis_core::models::DisplayMode;

pub enum DaemonCommand {
    Status {
        respond: oneshot::Sender<DaemonStatus>,
    },
    Next,
    Prev,
    /// The daemon's current state rendered as events, for a **Consumer** that
    /// just opened a subscription. Without it a subscriber learns nothing until
    /// the next rotation, and a reconnect after a `DankSocket` backoff would
    /// silently miss whatever changed while it was away.
    Snapshot {
        respond: oneshot::Sender<Vec<DaemonEvent>>,
    },
    Favorites {
        offset: Option<u32>,
        limit: Option<u32>,
        respond: oneshot::Sender<Result<FavoritesPage, String>>,
    },
    SetWallpaper {
        id: String,
        respond: oneshot::Sender<Result<(), String>>,
    },
    SetMode {
        mode: DisplayMode,
        respond: oneshot::Sender<Result<(), String>>,
    },
    Pause,
    Resume,
    Reload,
    WorkspaceChanged {
        id: u32,
    },
    Quit,
}
