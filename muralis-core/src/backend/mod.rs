pub mod hyprpaper;
pub mod monitor;
pub mod swww;

use async_trait::async_trait;
use std::path::Path;

use crate::config::Config;
use crate::error::Result;
use crate::models::BackendType;

#[async_trait]
pub trait WallpaperBackend: Send + Sync {
    async fn set_wallpaper(&self, path: &Path, monitor: &str) -> Result<()>;
    async fn set_wallpaper_all(&self, path: &Path) -> Result<()>;
    fn name(&self) -> &str;
}

pub fn create_backend(config: &Config) -> Box<dyn WallpaperBackend> {
    match config.general.backend {
        BackendType::Hyprpaper => Box::new(hyprpaper::HyprpaperBackend::new()),
        BackendType::Swww => Box::new(swww::SwwwBackend::new(config.display.transition.clone())),
    }
}
