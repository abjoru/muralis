use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    Wallhaven,
    Unsplash,
    Pexels,
    Feed,
    Local,
}

impl std::fmt::Display for SourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Wallhaven => write!(f, "wallhaven"),
            Self::Unsplash => write!(f, "unsplash"),
            Self::Pexels => write!(f, "pexels"),
            Self::Feed => write!(f, "feed"),
            Self::Local => write!(f, "local"),
        }
    }
}

impl std::str::FromStr for SourceType {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "wallhaven" => Ok(Self::Wallhaven),
            "unsplash" => Ok(Self::Unsplash),
            "pexels" => Ok(Self::Pexels),
            "feed" => Ok(Self::Feed),
            "local" => Ok(Self::Local),
            other => Err(format!("unknown source type: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallpaper {
    pub id: String,
    pub source_type: SourceType,
    pub source_id: String,
    pub source_url: Option<String>,
    pub width: u32,
    pub height: u32,
    pub tags: Vec<String>,
    pub file_path: String,
    pub added_at: String,
    pub last_used: Option<String>,
    pub use_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WallpaperPreview {
    pub source_type: SourceType,
    pub source_id: String,
    pub source_url: String,
    pub thumbnail_url: String,
    pub full_url: String,
    pub width: u32,
    pub height: u32,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlacklistEntry {
    pub source_id: String,
    pub source: SourceType,
    pub blacklisted_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    Static,
    Random,
    RandomStartup,
    Sequential,
    Workspace,
    Schedule,
}

impl DisplayMode {
    pub const ALL: &[DisplayMode] = &[
        DisplayMode::Static,
        DisplayMode::Random,
        DisplayMode::RandomStartup,
        DisplayMode::Sequential,
        DisplayMode::Workspace,
        DisplayMode::Schedule,
    ];
}

impl std::fmt::Display for DisplayMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Static => write!(f, "static"),
            Self::Random => write!(f, "random"),
            Self::RandomStartup => write!(f, "random_startup"),
            Self::Sequential => write!(f, "sequential"),
            Self::Workspace => write!(f, "workspace"),
            Self::Schedule => write!(f, "schedule"),
        }
    }
}

impl std::str::FromStr for DisplayMode {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "static" => Ok(Self::Static),
            "random" => Ok(Self::Random),
            "random_startup" => Ok(Self::RandomStartup),
            "sequential" => Ok(Self::Sequential),
            "workspace" => Ok(Self::Workspace),
            "schedule" => Ok(Self::Schedule),
            other => Err(format!("unknown display mode: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackendType {
    Hyprpaper,
    Swww,
}

impl BackendType {
    pub const ALL: &[BackendType] = &[BackendType::Hyprpaper, BackendType::Swww];
}

impl std::fmt::Display for BackendType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hyprpaper => write!(f, "hyprpaper"),
            Self::Swww => write!(f, "swww"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}
