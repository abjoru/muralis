pub mod engine;
pub mod scheduler;

use tokio::sync::oneshot;

use muralis_core::ipc::DaemonStatus;
use muralis_core::models::DisplayMode;

pub enum DaemonCommand {
    Status {
        respond: oneshot::Sender<DaemonStatus>,
    },
    Next,
    Prev,
    SetWallpaper {
        id: String,
        respond: oneshot::Sender<Result<(), String>>,
    },
    SetMode {
        mode: DisplayMode,
    },
    Pause,
    Resume,
    Reload,
    WorkspaceChanged {
        id: u32,
    },
    Quit,
}
