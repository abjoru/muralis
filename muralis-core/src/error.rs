use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum MuralisError {
    #[error("config error: {0}")]
    Config(String),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("toml parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("image error: {0}")]
    Image(#[from] image::ImageError),

    #[error("source not configured: {0}")]
    SourceNotConfigured(String),

    #[error("source error: {0}")]
    Source(String),

    #[error("wallpaper not found: {0}")]
    WallpaperNotFound(String),

    #[error("backend error: {0}")]
    Backend(String),

    #[error("ipc error: {0}")]
    Ipc(String),

    #[error("file not found: {0}")]
    FileNotFound(PathBuf),
}

pub type Result<T> = std::result::Result<T, MuralisError>;
