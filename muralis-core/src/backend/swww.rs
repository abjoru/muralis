use std::path::Path;

use async_trait::async_trait;
use tokio::process::Command;

use crate::config::TransitionConfig;
use crate::error::{MuralisError, Result};

use super::WallpaperBackend;

pub struct SwwwBackend {
    transition: TransitionConfig,
}

impl SwwwBackend {
    pub fn new(transition: TransitionConfig) -> Self {
        Self { transition }
    }

    fn build_command(&self, path: &Path, output: Option<&str>) -> Command {
        let mut cmd = Command::new("swww");
        cmd.arg("img").arg(path);
        cmd.arg("--transition-type").arg(&self.transition.r#type);
        cmd.arg("--transition-duration")
            .arg(self.transition.duration.to_string());
        cmd.arg("--transition-fps")
            .arg(self.transition.fps.to_string());

        if let Some(monitor) = output {
            cmd.arg("--outputs").arg(monitor);
        }

        cmd
    }
}

#[async_trait]
impl WallpaperBackend for SwwwBackend {
    async fn set_wallpaper(&self, path: &Path, monitor: &str) -> Result<()> {
        let output = self
            .build_command(path, Some(monitor))
            .output()
            .await
            .map_err(|e| MuralisError::Backend(format!("failed to run swww: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(MuralisError::Backend(format!("swww failed: {stderr}")));
        }
        Ok(())
    }

    async fn set_wallpaper_all(&self, path: &Path) -> Result<()> {
        let output = self
            .build_command(path, None)
            .output()
            .await
            .map_err(|e| MuralisError::Backend(format!("failed to run swww: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(MuralisError::Backend(format!("swww failed: {stderr}")));
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "swww"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_swww_command_args() {
        let transition = TransitionConfig {
            r#type: "fade".into(),
            duration: 2.0,
            fps: 60,
        };
        let backend = SwwwBackend::new(transition);
        let path = PathBuf::from("/data/wallpapers/abc123.jpg");

        // test command with specific monitor
        let cmd = backend.build_command(&path, Some("DP-1"));
        let prog = cmd.as_std().get_program().to_string_lossy().to_string();
        let args: Vec<String> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        assert_eq!(prog, "swww");
        assert_eq!(args[0], "img");
        assert_eq!(args[1], "/data/wallpapers/abc123.jpg");
        assert!(args.contains(&"--transition-type".to_string()));
        assert!(args.contains(&"fade".to_string()));
        assert!(args.contains(&"--transition-duration".to_string()));
        assert!(args.contains(&"2".to_string()));
        assert!(args.contains(&"--transition-fps".to_string()));
        assert!(args.contains(&"60".to_string()));
        assert!(args.contains(&"--outputs".to_string()));
        assert!(args.contains(&"DP-1".to_string()));

        // test command without monitor (all outputs)
        let cmd = backend.build_command(&path, None);
        let args: Vec<String> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(!args.contains(&"--outputs".to_string()));
    }
}
