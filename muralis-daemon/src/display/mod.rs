pub mod engine;
pub mod scheduler;

use tokio::sync::oneshot;

use muralis_core::ipc::{DaemonStatus, FavoritesPage};
use muralis_core::models::DisplayMode;

pub enum DaemonCommand {
    Status {
        respond: oneshot::Sender<DaemonStatus>,
    },
    Next,
    Prev,
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
