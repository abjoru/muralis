use std::path::Path;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::{interval, Instant, MissedTickBehavior};
use tracing::{info, warn};

use muralis_core::backend::WallpaperBackend;
use muralis_core::cache;
use muralis_core::config::Config;
use muralis_core::db::Database;
use muralis_core::ipc::DaemonStatus;
use muralis_core::models::{DisplayMode, Wallpaper};
use muralis_core::paths::MuralisPaths;

use super::scheduler::{next_schedule_trigger, parse_interval};
use super::DaemonCommand;

pub struct DisplayEngine {
    config: Config,
    paths: MuralisPaths,
    backend: Box<dyn WallpaperBackend>,
    mode: DisplayMode,
    paused: bool,
    current_index: usize,
    current_wallpaper: Option<String>,
    wallpapers: Vec<Wallpaper>,
    next_change: Option<Instant>,
}

impl DisplayEngine {
    pub fn new(
        config: Config,
        paths: MuralisPaths,
        backend: Box<dyn WallpaperBackend>,
    ) -> Self {
        let mode = config.display.mode;
        Self {
            config,
            paths,
            backend,
            mode,
            paused: false,
            current_index: 0,
            current_wallpaper: None,
            wallpapers: Vec::new(),
            next_change: None,
        }
    }

    pub async fn run(
        mut self,
        mut cmd_rx: mpsc::Receiver<DaemonCommand>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) {
        self.reload_wallpapers();

        // initial cache prune
        self.prune_cache();

        let tick_duration = parse_interval(&self.config.display.interval)
            .unwrap_or(Duration::from_secs(1800));

        let mut timer = interval(tick_duration);
        timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
        // skip the first immediate tick
        timer.tick().await;

        // cache prune timer: every hour
        let mut cache_timer = interval(Duration::from_secs(3600));
        cache_timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
        cache_timer.tick().await;

        self.update_next_change(tick_duration);

        loop {
            tokio::select! {
                _ = timer.tick() => {
                    if !self.paused {
                        match self.mode {
                            DisplayMode::Random | DisplayMode::Sequential => {
                                self.next().await;
                                self.update_next_change(tick_duration);
                            }
                            DisplayMode::Schedule => {
                                self.handle_schedule().await;
                            }
                            _ => {}
                        }
                    }
                }
                _ = cache_timer.tick() => {
                    self.prune_cache();
                }
                Some(cmd) = cmd_rx.recv() => {
                    match cmd {
                        DaemonCommand::Status { respond } => {
                            let status = self.status();
                            let _ = respond.send(status);
                        }
                        DaemonCommand::Next => {
                            self.next().await;
                            self.update_next_change(tick_duration);
                            timer.reset();
                        }
                        DaemonCommand::Prev => {
                            self.prev().await;
                            self.update_next_change(tick_duration);
                            timer.reset();
                        }
                        DaemonCommand::SetWallpaper { id, respond } => {
                            let result = self.set_wallpaper(&id).await;
                            let _ = respond.send(result.map_err(|e| e.to_string()));
                        }
                        DaemonCommand::SetMode { mode } => {
                            info!(mode = %mode, "display mode changed");
                            self.mode = mode;
                        }
                        DaemonCommand::Pause => {
                            self.paused = true;
                            info!("rotation paused");
                        }
                        DaemonCommand::Resume => {
                            self.paused = false;
                            self.update_next_change(tick_duration);
                            timer.reset();
                            info!("rotation resumed");
                        }
                        DaemonCommand::Reload => {
                            self.config = Config::load_or_default(&self.paths);
                            self.reload_wallpapers();
                            info!("config reloaded");
                        }
                        DaemonCommand::WorkspaceChanged { id } => {
                            self.handle_workspace_change(id).await;
                        }
                        DaemonCommand::Quit => {
                            info!("quit command received");
                            return;
                        }
                    }
                }
                _ = shutdown.changed() => {
                    info!("shutdown signal received");
                    return;
                }
            }
        }
    }

    fn reload_wallpapers(&mut self) {
        match Database::open(&self.paths.db_path()) {
            Ok(db) => match db.list_wallpapers() {
                Ok(wps) => {
                    info!(count = wps.len(), "loaded wallpapers from DB");
                    self.wallpapers = wps;
                }
                Err(e) => warn!("failed to load wallpapers: {e}"),
            },
            Err(e) => warn!("failed to open database: {e}"),
        }
    }

    async fn next(&mut self) {
        if self.wallpapers.is_empty() {
            return;
        }

        match self.mode {
            DisplayMode::Random => {
                use rand::Rng;
                let idx = rand::rng().random_range(0..self.wallpapers.len());
                self.current_index = idx;
            }
            DisplayMode::Sequential => {
                self.current_index = (self.current_index + 1) % self.wallpapers.len();
            }
            _ => return,
        }

        self.apply_current().await;
    }

    async fn prev(&mut self) {
        if self.wallpapers.is_empty() {
            return;
        }

        if self.current_index == 0 {
            self.current_index = self.wallpapers.len() - 1;
        } else {
            self.current_index -= 1;
        }

        self.apply_current().await;
    }

    async fn apply_current(&mut self) {
        if let Some(wp) = self.wallpapers.get(self.current_index) {
            let path = Path::new(&wp.file_path);
            if path.exists() {
                match self.backend.set_wallpaper_all(path).await {
                    Ok(()) => {
                        self.current_wallpaper = Some(wp.id.clone());
                        if let Ok(db) = Database::open(&self.paths.db_path()) {
                            let _ = db.mark_used(&wp.id);
                        }
                        info!(id = %wp.id, "wallpaper set");
                    }
                    Err(e) => warn!("failed to set wallpaper: {e}"),
                }
            } else {
                warn!(path = %path.display(), "wallpaper file missing");
            }
        }
    }

    async fn set_wallpaper(&mut self, id: &str) -> muralis_core::error::Result<()> {
        let wp = self
            .wallpapers
            .iter()
            .find(|w| w.id == id)
            .cloned()
            .or_else(|| {
                Database::open(&self.paths.db_path())
                    .ok()
                    .and_then(|db| db.get_wallpaper(id).ok())
            });

        match wp {
            Some(wp) => {
                let path = Path::new(&wp.file_path);
                self.backend.set_wallpaper_all(path).await?;
                self.current_wallpaper = Some(wp.id.clone());
                if let Ok(db) = Database::open(&self.paths.db_path()) {
                    let _ = db.mark_used(&wp.id);
                }
                Ok(())
            }
            None => Err(muralis_core::error::MuralisError::WallpaperNotFound(
                id.to_string(),
            )),
        }
    }

    /// Handle workspace change: look up workspace->wallpaper mapping from config.
    async fn handle_workspace_change(&mut self, workspace_id: u32) {
        if self.mode != DisplayMode::Workspace {
            return;
        }

        // find matching workspace config
        if let Some(ws_config) = self.config.workspaces.iter().find(|w| w.workspace == workspace_id) {
            let wallpaper_key = &ws_config.wallpaper;

            // try to find by tag or ID
            let wp = self
                .wallpapers
                .iter()
                .find(|w| w.id == *wallpaper_key || w.tags.iter().any(|t| t == wallpaper_key));

            if let Some(wp) = wp {
                let path = Path::new(&wp.file_path);
                if path.exists() {
                    match self.backend.set_wallpaper_all(path).await {
                        Ok(()) => {
                            self.current_wallpaper = Some(wp.id.clone());
                            info!(workspace = workspace_id, id = %wp.id, "workspace wallpaper set");
                        }
                        Err(e) => warn!("failed to set workspace wallpaper: {e}"),
                    }
                }
            } else {
                warn!(workspace = workspace_id, key = %wallpaper_key, "no wallpaper found for workspace");
            }
        }
    }

    /// Handle schedule mode: pick random wallpaper matching schedule tags.
    async fn handle_schedule(&mut self) {
        if let Some((_, tags)) = next_schedule_trigger(&self.config.schedules) {
            let matching: Vec<usize> = self
                .wallpapers
                .iter()
                .enumerate()
                .filter(|(_, wp)| tags.iter().any(|t| wp.tags.contains(t)))
                .map(|(i, _)| i)
                .collect();

            if !matching.is_empty() {
                use rand::Rng;
                let idx = matching[rand::rng().random_range(0..matching.len())];
                self.current_index = idx;
                self.apply_current().await;
            }
        }
    }

    fn prune_cache(&self) {
        let max_bytes = self.config.general.cache_max_mb * 1024 * 1024;
        match cache::prune_cache(&self.paths, max_bytes) {
            Ok(freed) if freed > 0 => {
                info!(freed_mb = freed / (1024 * 1024), "cache pruned");
            }
            Ok(_) => {}
            Err(e) => warn!("cache prune error: {e}"),
        }
    }

    fn status(&self) -> DaemonStatus {
        DaemonStatus {
            running: true,
            mode: self.mode,
            paused: self.paused,
            current_wallpaper: self.current_wallpaper.clone(),
            wallpaper_count: self.wallpapers.len() as u32,
            next_change: self.next_change.map(|t| {
                let remaining = t.saturating_duration_since(Instant::now());
                format!("{}s", remaining.as_secs())
            }),
        }
    }

    fn update_next_change(&mut self, duration: Duration) {
        if !self.paused && matches!(self.mode, DisplayMode::Random | DisplayMode::Sequential) {
            self.next_change = Some(Instant::now() + duration);
        } else {
            self.next_change = None;
        }
    }
}
