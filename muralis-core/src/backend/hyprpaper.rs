use std::path::Path;

use async_trait::async_trait;
use tokio::process::Command;

use crate::error::{MuralisError, Result};

use super::WallpaperBackend;

pub struct HyprpaperBackend;

impl HyprpaperBackend {
    pub fn new() -> Self {
        Self
    }

    async fn hyprctl(args: &[&str]) -> Result<String> {
        let output = Command::new("hyprctl")
            .args(args)
            .output()
            .await
            .map_err(|e| MuralisError::Backend(format!("failed to run hyprctl: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(MuralisError::Backend(format!("hyprctl failed: {stderr}")));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

#[async_trait]
impl WallpaperBackend for HyprpaperBackend {
    async fn set_wallpaper(&self, path: &Path, monitor: &str) -> Result<()> {
        let path_str = path.to_string_lossy();

        // preload the wallpaper
        Self::hyprctl(&["hyprpaper", "preload", &path_str]).await?;

        // set wallpaper on specific monitor
        let wallpaper_arg = format!("{monitor},{path_str}");
        Self::hyprctl(&["hyprpaper", "wallpaper", &wallpaper_arg]).await?;

        // unload all unused wallpapers to free memory
        Self::hyprctl(&["hyprpaper", "unload", "all"]).await?;

        Ok(())
    }

    async fn set_wallpaper_all(&self, path: &Path) -> Result<()> {
        let path_str = path.to_string_lossy();

        Self::hyprctl(&["hyprpaper", "preload", &path_str]).await?;

        let wallpaper_arg = format!(",{path_str}");
        Self::hyprctl(&["hyprpaper", "wallpaper", &wallpaper_arg]).await?;

        Self::hyprctl(&["hyprpaper", "unload", "all"]).await?;

        Ok(())
    }

    fn name(&self) -> &str {
        "hyprpaper"
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    #[test]
    fn test_hyprpaper_command_args() {
        // Verify the argument format for hyprpaper commands
        let path = PathBuf::from("/data/wallpapers/abc123.jpg");
        let monitor = "DP-1";
        let path_str = path.to_string_lossy();

        let wallpaper_arg = format!("{monitor},{path_str}");
        assert_eq!(wallpaper_arg, "DP-1,/data/wallpapers/abc123.jpg");

        let all_arg = format!(",{path_str}");
        assert_eq!(all_arg, ",/data/wallpapers/abc123.jpg");
    }
}
