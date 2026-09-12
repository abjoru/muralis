use serde::{Deserialize, Serialize};

use crate::error::{MuralisError, Result};
use crate::models::{BackendType, DisplayMode};
use crate::paths::MuralisPaths;
use crate::sources::ContentSafety;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub display: DisplayConfig,
    #[serde(default = "default_sources")]
    pub sources: toml::Table,
    #[serde(default)]
    pub workspaces: Vec<WorkspaceConfig>,
    #[serde(default)]
    pub schedules: Vec<ScheduleEntry>,
    pub filter: FilterConfig,
}

fn default_sources() -> toml::Table {
    let mut table = toml::Table::new();
    let mut wh = toml::Table::new();
    wh.insert("enabled".into(), toml::Value::Boolean(true));
    wh.insert("categories".into(), toml::Value::String("100".into()));
    wh.insert("purity".into(), toml::Value::String("100".into()));
    table.insert("wallhaven".into(), toml::Value::Table(wh));
    table
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            display: DisplayConfig::default(),
            sources: default_sources(),
            workspaces: Vec::new(),
            schedules: Vec::new(),
            filter: FilterConfig::default(),
        }
    }
}

impl Config {
    /// The config as it is on disk — or the defaults, when there is no file
    /// yet.
    ///
    /// A machine nobody has configured is not a broken one: a **Source** list
    /// that holds only what ships enabled is the honest answer to `sources
    /// list` on a fresh install, not an error about a file the user was never
    /// asked to write. A file that *exists* and will not parse is still a
    /// failure — defaulting past that would hide the typo that caused it.
    pub fn load(paths: &MuralisPaths) -> Result<Self> {
        let path = paths.config_file();
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => {
                return Err(MuralisError::Config(format!(
                    "failed to read {}: {e}",
                    path.display()
                )))
            }
        };
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn load_or_default(paths: &MuralisPaths) -> Self {
        Self::load(paths).unwrap_or_default()
    }

    /// Why `mode` cannot run under this config, or `None` when it can.
    ///
    /// The single viability predicate: `SetMode` refuses on `Some` rather than
    /// reporting success for a mode whose handler would no-op forever, and the
    /// usable set a **Consumer** reads off `status` is the modes answering
    /// `None`. Both read it here so the rule exists once.
    pub fn mode_unavailable_reason(&self, mode: DisplayMode) -> Option<&'static str> {
        match mode {
            DisplayMode::Schedule if self.schedules.is_empty() => Some("no schedules configured"),
            DisplayMode::Workspace if self.workspaces.is_empty() => {
                Some("no workspaces configured")
            }
            _ => None,
        }
    }

    /// The modes this config can actually run — `DisplayMode::ALL` minus the
    /// ones `mode_unavailable_reason` has an answer for.
    ///
    /// Derived, never re-encoded: a **Consumer** reads this off `status` to
    /// offer only what will work, and `SetMode` refuses on the same predicate,
    /// so the offer and the refusal cannot drift apart.
    pub fn available_modes(&self) -> Vec<DisplayMode> {
        DisplayMode::ALL
            .iter()
            .copied()
            .filter(|mode| self.mode_unavailable_reason(*mode).is_none())
            .collect()
    }

    /// Write `mode` through to `config.toml`, in memory and on disk.
    ///
    /// A mode is a deliberate choice, and the daemon re-reads this file at every
    /// login — leaving it in memory only is how `muralis mode static` evaporates
    /// at the next reboot with nothing to say it ever happened.
    ///
    /// Pause is deliberately *not* written through. Pausing rotation reads as a
    /// temporary act in a way that choosing a mode does not, so it stays
    /// ephemeral; the asymmetry is intended, not this fix left half-done.
    ///
    /// The edit is surgical: `config.toml` is hand-written, so only the `mode`
    /// value moves and every comment, key order and bit of spacing around it
    /// survives. A file that does not parse is refused rather than overwritten.
    pub fn persist_mode(&mut self, paths: &MuralisPaths, mode: DisplayMode) -> Result<()> {
        let path = paths.config_file();

        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            // No config file yet is not an error: the daemon runs on defaults,
            // and the choice still has to outlive the process.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => {
                return Err(MuralisError::Config(format!(
                    "failed to read {}: {e}",
                    path.display()
                )))
            }
        };

        let mut doc = content.parse::<toml_edit::DocumentMut>().map_err(|e| {
            MuralisError::Config(format!("failed to parse {}: {e}", path.display()))
        })?;

        doc["display"]["mode"] = toml_edit::value(mode.to_string());

        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| {
                MuralisError::Config(format!("failed to create {}: {e}", dir.display()))
            })?;
        }
        std::fs::write(&path, doc.to_string()).map_err(|e| {
            MuralisError::Config(format!("failed to write {}: {e}", path.display()))
        })?;

        self.display.mode = mode;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    pub backend: BackendType,
    pub cache_max_mb: u64,
    pub thumbnail_zoom: f32,
    /// Global content-safety ceiling (ADR 0002). Default `safe` — the panic
    /// switch that forces every source to its safest native setting.
    #[serde(default)]
    pub content_safety: ContentSafety,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            backend: BackendType::Hyprpaper,
            cache_max_mb: 500,
            thumbnail_zoom: 1.0,
            content_safety: ContentSafety::default(),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway XDG root holding `content` as config.toml (none when `None`).
    fn paths_with(content: Option<&str>) -> (MuralisPaths, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let paths = MuralisPaths {
            config_dir: tmp.path().join("config"),
            data_dir: tmp.path().join("data"),
            cache_dir: tmp.path().join("cache"),
        };
        paths.ensure_dirs().unwrap();
        if let Some(content) = content {
            std::fs::write(paths.config_file(), content).unwrap();
        }
        (paths, tmp)
    }

    #[test]
    fn a_persisted_mode_survives_a_restart() {
        let (paths, _tmp) = paths_with(Some("[display]\nmode = \"random_startup\"\n"));
        let mut config = Config::load(&paths).unwrap();

        config.persist_mode(&paths, DisplayMode::Static).unwrap();

        assert_eq!(
            Config::load(&paths).unwrap().display.mode,
            DisplayMode::Static,
            "the daemon reads config.toml at next login; the choice has to be in it"
        );
        assert_eq!(
            config.display.mode,
            DisplayMode::Static,
            "the in-memory config must not disagree with the file it was just written to"
        );
    }

    #[test]
    fn persisting_a_mode_leaves_the_hand_edits_around_it_alone() {
        let original = r#"# my wallpaper setup
[general]
backend = "swww"   # trailing note

[display]
mode  = "random"
interval = "15m"   # every quarter hour
"#;
        let (paths, _tmp) = paths_with(Some(original));
        let mut config = Config::load(&paths).unwrap();

        config
            .persist_mode(&paths, DisplayMode::Sequential)
            .unwrap();

        let written = std::fs::read_to_string(paths.config_file()).unwrap();
        assert!(
            written.contains("# my wallpaper setup"),
            "a whole-file reserialize would drop the comments: {written}"
        );
        assert!(written.contains("backend = \"swww\"   # trailing note"));
        assert!(written.contains("interval = \"15m\"   # every quarter hour"));
        assert!(
            written.contains("mode  = \"sequential\""),
            "only the value changes, not the spacing the user chose: {written}"
        );
    }

    #[test]
    fn a_config_with_no_display_section_gains_one() {
        let (paths, _tmp) = paths_with(Some("[general]\nbackend = \"swww\"\n"));
        let mut config = Config::load(&paths).unwrap();

        config.persist_mode(&paths, DisplayMode::Workspace).unwrap();

        let reloaded = Config::load(&paths).unwrap();
        assert_eq!(reloaded.display.mode, DisplayMode::Workspace);
        assert_eq!(
            reloaded.general.backend,
            BackendType::Swww,
            "the section we added must not disturb the one that was there"
        );
    }

    #[test]
    fn a_missing_config_file_is_created_rather_than_losing_the_choice() {
        let (paths, _tmp) = paths_with(None);
        let mut config = Config::default();

        config.persist_mode(&paths, DisplayMode::Static).unwrap();

        assert_eq!(
            Config::load(&paths).unwrap().display.mode,
            DisplayMode::Static
        );
    }

    #[test]
    fn a_config_that_does_not_parse_is_left_untouched() {
        let broken = "[display\nmode = \"random\"\n";
        let (paths, _tmp) = paths_with(Some(broken));
        let mut config = Config::default();

        let result = config.persist_mode(&paths, DisplayMode::Static);

        assert!(
            result.is_err(),
            "a file we cannot parse is not one we can edit"
        );
        assert_eq!(
            std::fs::read_to_string(paths.config_file()).unwrap(),
            broken,
            "refusing must not cost the user the config they have"
        );
    }

    #[test]
    fn schedule_is_unusable_without_schedules() {
        let config = Config::default();
        assert_eq!(
            config.mode_unavailable_reason(DisplayMode::Schedule),
            Some("no schedules configured"),
            "schedule mode no-ops forever on an empty schedule list"
        );
    }

    #[test]
    fn schedule_becomes_usable_once_one_is_configured() {
        let mut config = Config::default();
        config.schedules.push(ScheduleEntry {
            time: "08:00".into(),
            tags: vec!["morning".into()],
        });
        assert_eq!(config.mode_unavailable_reason(DisplayMode::Schedule), None);
    }

    #[test]
    fn workspace_is_unusable_without_workspaces() {
        let config = Config::default();
        assert_eq!(
            config.mode_unavailable_reason(DisplayMode::Workspace),
            Some("no workspaces configured"),
        );
    }

    #[test]
    fn workspace_becomes_usable_once_one_is_configured() {
        let mut config = Config::default();
        config.workspaces.push(WorkspaceConfig {
            workspace: 1,
            wallpaper: "wp1".into(),
        });
        assert_eq!(config.mode_unavailable_reason(DisplayMode::Workspace), None);
    }

    #[test]
    fn the_usable_set_leaves_out_what_the_config_does_not_support() {
        let config = Config::default();
        assert_eq!(
            config.available_modes(),
            vec![
                DisplayMode::Static,
                DisplayMode::Random,
                DisplayMode::RandomStartup,
                DisplayMode::Sequential,
            ],
            "a Consumer offering schedule or workspace here offers a mode that cannot run"
        );
    }

    #[test]
    fn a_configured_mode_joins_the_usable_set() {
        let mut config = Config::default();
        config.schedules.push(ScheduleEntry {
            time: "08:00".into(),
            tags: vec!["morning".into()],
        });

        let modes = config.available_modes();

        assert!(
            modes.contains(&DisplayMode::Schedule),
            "one schedule is all schedule mode ever needed"
        );
        assert!(
            !modes.contains(&DisplayMode::Workspace),
            "workspaces are still empty, so workspace mode is still inert"
        );
    }

    #[test]
    fn the_rotation_modes_need_no_config_to_run() {
        let config = Config::default();
        for mode in [
            DisplayMode::Static,
            DisplayMode::Random,
            DisplayMode::RandomStartup,
            DisplayMode::Sequential,
        ] {
            assert_eq!(
                config.mode_unavailable_reason(mode),
                None,
                "{mode} has no config precondition"
            );
        }
    }

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.general.backend, BackendType::Hyprpaper);
        assert_eq!(config.display.mode, DisplayMode::Random);
        assert_eq!(config.display.interval, "30m");
        assert_eq!(config.filter.min_width, 1920);
        // wallhaven enabled by default
        let wh = config.sources.get("wallhaven").unwrap().as_table().unwrap();
        assert_eq!(wh.get("enabled").unwrap().as_bool(), Some(true));
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
        // sources is now a raw table
        let wh = config.sources.get("wallhaven").unwrap().as_table().unwrap();
        assert_eq!(wh.get("api_key").unwrap().as_str(), Some("test_key"));
        let feeds = config.sources.get("feeds").unwrap().as_array().unwrap();
        assert_eq!(feeds.len(), 1);
        assert_eq!(config.workspaces.len(), 1);
        assert_eq!(config.schedules.len(), 1);
        assert_eq!(config.filter.exclude_tags, vec!["anime", "cartoon"]);
    }
}
