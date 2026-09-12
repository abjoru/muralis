use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum MuralisError {
    #[error("config error: {0}")]
    Config(String),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// An IO failure against one of muralis's own files or directories, with
    /// the operation and the path it concerns.
    ///
    /// A bare errno — `No such file or directory (os error 2)` — names
    /// neither, and has been mistaken for a fault in whatever command
    /// happened to trip it more than once. Anything touching our own XDG
    /// locations reports through here; `Io` is left for IO that has no path
    /// to name, such as a transport failure.
    #[error("cannot {op} {}: {cause}", .path.display())]
    IoAt {
        op: &'static str,
        path: PathBuf,
        // Deliberately not `#[source]`: the message already carries the
        // cause, and chaining it makes anyhow print it a second time under
        // "Caused by".
        cause: std::io::Error,
    },

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

    #[error("{source_type} {op}: {kind}")]
    Source {
        source_type: String,
        op: String,
        kind: String,
    },

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

/// Attach the path an IO call was made against to whatever it failed with.
///
/// `std::fs::write(&p, data).at("write", &p)?` in place of a bare `?`: the
/// one habit that keeps an unqualified "No such file or directory" out of
/// muralis's output.
pub trait IoAt<T> {
    fn at(self, op: &'static str, path: impl Into<PathBuf>) -> Result<T>;
}

impl<T> IoAt<T> for std::result::Result<T, std::io::Error> {
    fn at(self, op: &'static str, path: impl Into<PathBuf>) -> Result<T> {
        self.map_err(|cause| MuralisError::IoAt {
            op,
            path: path.into(),
            cause,
        })
    }
}
