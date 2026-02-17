use std::path::PathBuf;

use crate::error::{MuralisError, Result};

#[derive(Debug, Clone)]
pub struct MuralisPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl MuralisPaths {
    pub fn new() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| MuralisError::Config("cannot resolve XDG config dir".into()))?
            .join("muralis");

        let data_dir = dirs::data_dir()
            .ok_or_else(|| MuralisError::Config("cannot resolve XDG data dir".into()))?
            .join("muralis");

        let cache_dir = dirs::cache_dir()
            .ok_or_else(|| MuralisError::Config("cannot resolve XDG cache dir".into()))?
            .join("muralis");

        Ok(Self {
            config_dir,
            data_dir,
            cache_dir,
        })
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("muralis.db")
    }

    pub fn wallpapers_dir(&self) -> PathBuf {
        self.data_dir.join("wallpapers")
    }

    pub fn thumbnails_dir(&self) -> PathBuf {
        self.cache_dir.join("thumbnails")
    }

    pub fn previews_dir(&self) -> PathBuf {
        self.cache_dir.join("previews")
    }

    pub fn socket_path() -> PathBuf {
        let uid = unsafe { libc::getuid() };
        PathBuf::from(format!("/tmp/muralis-{uid}.sock"))
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        for dir in [
            &self.config_dir,
            &self.data_dir,
            &self.cache_dir,
            &self.wallpapers_dir(),
            &self.thumbnails_dir(),
            &self.previews_dir(),
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}
