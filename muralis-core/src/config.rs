use serde::{Deserialize, Serialize};

use crate::error::{MuralisError, Result};
use crate::models::{BackendType, DisplayMode};
use crate::paths::MuralisPaths;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub display: DisplayConfig,
    pub sources: SourcesConfig,
    #[serde(default)]
    pub workspaces: Vec<WorkspaceConfig>,
    #[serde(default)]
    pub schedules: Vec<ScheduleEntry>,
    pub filter: FilterConfig,
}

impl Config {
    pub fn load(paths: &MuralisPaths) -> Result<Self> {
        let path = paths.config_file();
        let content = std::fs::read_to_string(&path)
            .map_err(|e| MuralisError::Config(format!("failed to read {}: {e}", path.display())))?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn load_or_default(paths: &MuralisPaths) -> Self {
        Self::load(paths).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    pub backend: BackendType,
    pub cache_max_mb: u64,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            backend: BackendType::Hyprpaper,
            cache_max_mb: 500,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayConfig {
    pub mode: DisplayMode,
    pub interval: String,
    pub min_resolution: String,
    pub aspect_ratio: String,
    pub transition: TransitionConfig,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            mode: DisplayMode::Random,
            interval: "30m".into(),
            min_resolution: "auto".into(),
            aspect_ratio: "auto".into(),
            transition: TransitionConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TransitionConfig {
    pub r#type: String,
    pub duration: f64,
    pub fps: u32,
}

impl Default for TransitionConfig {
    fn default() -> Self {
        Self {
            r#type: "fade".into(),
            duration: 2.0,
            fps: 60,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub workspace: u32,
    pub wallpaper: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleEntry {
    pub time: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FilterConfig {
    pub min_width: u32,
    pub min_height: u32,
    pub exclude_tags: Vec<String>,
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            min_width: 1920,
            min_height: 1080,
            exclude_tags: Vec::new(),
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SourcesConfig {
    pub wallhaven: WallhavenConfig,
    pub unsplash: UnsplashConfig,
    pub pexels: PexelsConfig,
    #[serde(default)]
    pub feeds: Vec<FeedConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WallhavenConfig {
    pub enabled: bool,
    pub api_key: Option<String>,
    pub categories: String,
    pub purity: String,
}

impl Default for WallhavenConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            api_key: None,
            categories: "100".into(),
            purity: "100".into(),
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UnsplashConfig {
    pub enabled: bool,
    pub access_key: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PexelsConfig {
    pub enabled: bool,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedConfig {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.general.backend, BackendType::Hyprpaper);
        assert_eq!(config.display.mode, DisplayMode::Random);
        assert_eq!(config.display.interval, "30m");
        assert_eq!(config.filter.min_width, 1920);
        assert!(config.sources.wallhaven.enabled);
        assert!(!config.sources.unsplash.enabled);
    }

    #[test]
    fn test_parse_minimal_toml() {
        let toml_str = r#"
[general]
backend = "swww"

[display]
mode = "static"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.general.backend, BackendType::Swww);
        assert_eq!(config.display.mode, DisplayMode::Static);
        // defaults still applied
        assert_eq!(config.general.cache_max_mb, 500);
        assert_eq!(config.filter.min_width, 1920);
    }

    #[test]
    fn test_parse_full_toml() {
        let toml_str = r#"
[general]
backend = "hyprpaper"
cache_max_mb = 1000

[display]
mode = "random"
interval = "15m"
min_resolution = "2560x1440"
aspect_ratio = "16:9"

[display.transition]
type = "wipe"
duration = 1.5
fps = 30

[filter]
min_width = 2560
min_height = 1440
exclude_tags = ["anime", "cartoon"]

[sources.wallhaven]
enabled = true
api_key = "test_key"
categories = "111"
purity = "110"

[sources.unsplash]
enabled = true
access_key = "unsplash_key"

[sources.pexels]
enabled = false

[[sources.feeds]]
name = "Bing Daily"
url = "https://example.com/feed.rss"
enabled = true

[[workspaces]]
workspace = 1
wallpaper = "nature"

[[schedules]]
time = "08:00"
tags = ["bright", "morning"]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.general.cache_max_mb, 1000);
        assert_eq!(config.display.transition.r#type, "wipe");
        assert_eq!(
            config.sources.wallhaven.api_key.as_deref(),
            Some("test_key")
        );
        assert_eq!(config.sources.feeds.len(), 1);
        assert_eq!(config.workspaces.len(), 1);
        assert_eq!(config.schedules.len(), 1);
        assert_eq!(config.filter.exclude_tags, vec!["anime", "cartoon"]);
    }
}
