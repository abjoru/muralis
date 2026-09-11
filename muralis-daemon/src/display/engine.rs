use std::path::Path;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::{interval, Instant, MissedTickBehavior};
use tracing::{info, warn};

use muralis_core::backend::{wait_until_ready, ReadinessPolicy, WallpaperBackend};
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
    last_error: Option<String>,
    readiness: ReadinessPolicy,
}

impl DisplayEngine {
    pub fn new(config: Config, paths: MuralisPaths, backend: Box<dyn WallpaperBackend>) -> Self {
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
            last_error: None,
            readiness: ReadinessPolicy::default(),
        }
    }

    pub async fn run(
        mut self,
        mut cmd_rx: mpsc::Receiver<DaemonCommand>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) {
        self.reload_wallpapers();

        // RandomStartup: pick one wallpaper at launch, then behave like Static
        if self.mode == DisplayMode::RandomStartup {
            self.startup_apply().await;
        }

        // initial cache prune
        self.prune_cache();

        let tick_duration =
            parse_interval(&self.config.display.interval).unwrap_or(Duration::from_secs(1800));

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

    /// The one-shot `random_startup` apply, gated on the backend's own daemon
    /// being up. The compositor typically launches muralis and the backend
    /// together with no sequencing, and this apply is the only one of the
    /// session — firing it into a socket that is not bound yet means no
    /// wallpaper until the next login.
    async fn startup_apply(&mut self) {
        // Nothing to apply means nothing to wait for — an empty library must
        // not hold the daemon's startup hostage to an absent backend.
        if self.wallpapers.is_empty() {
            return;
        }

        if let Err(e) = wait_until_ready(self.backend.as_ref(), &self.readiness).await {
            warn!(backend = self.backend.name(), "backend not ready: {e}");
            self.last_error = Some(format!("backend not ready: {e}"));
        }

        // Attempt regardless: the probe is a best-effort gate, not a veto, and
        // apply_current records its own failure.
        self.next().await;
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
            DisplayMode::Random | DisplayMode::RandomStartup => {
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
                        self.last_error = None;
                        if let Ok(db) = Database::open(&self.paths.db_path()) {
                            let _ = db.mark_used(&wp.id);
                        }
                        info!(id = %wp.id, "wallpaper set");
                    }
                    // No retry here: a mid-session failure is followed by a tick
                    // or a user command soon enough. It is recorded rather than
                    // only logged, so a Consumer can say the wallpaper on screen
                    // is not the one muralis thinks it set.
                    Err(e) => {
                        warn!("failed to set wallpaper: {e}");
                        self.last_error = Some(e.to_string());
                    }
                }
            } else {
                warn!(path = %path.display(), "wallpaper file missing");
                self.last_error = Some(format!("wallpaper file missing: {}", path.display()));
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
        if let Some(ws_config) = self
            .config
            .workspaces
            .iter()
            .find(|w| w.workspace == workspace_id)
        {
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
                            self.last_error = None;
                            info!(workspace = workspace_id, id = %wp.id, "workspace wallpaper set");
                        }
                        Err(e) => {
                            warn!("failed to set workspace wallpaper: {e}");
                            self.last_error = Some(e.to_string());
                        }
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
            last_error: self.last_error.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use muralis_core::backend::ReadinessPolicy;
    use muralis_core::error::{MuralisError, Result};
    use muralis_core::models::SourceType;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Backend that reports ready from the `ready_at`-th probe onward
    /// (`0` = never) and only then accepts a wallpaper, like awww-daemon.
    struct FakeBackend {
        ready_at: u32,
        probes: AtomicU32,
    }

    impl FakeBackend {
        fn ready_at(ready_at: u32) -> Self {
            Self {
                ready_at,
                probes: AtomicU32::new(0),
            }
        }

        fn is_up(&self) -> bool {
            self.ready_at != 0 && self.probes.load(Ordering::SeqCst) >= self.ready_at
        }
    }

    #[async_trait]
    impl WallpaperBackend for FakeBackend {
        async fn set_wallpaper(&self, _path: &Path, _monitor: &str) -> Result<()> {
            self.set_wallpaper_all(_path).await
        }

        async fn set_wallpaper_all(&self, _path: &Path) -> Result<()> {
            if self.is_up() {
                Ok(())
            } else {
                Err(MuralisError::Backend("socket not bound".into()))
            }
        }

        async fn is_ready(&self) -> Result<()> {
            self.probes.fetch_add(1, Ordering::SeqCst);
            if self.is_up() {
                Ok(())
            } else {
                Err(MuralisError::Backend("socket not bound".into()))
            }
        }

        fn name(&self) -> &str {
            "fake"
        }
    }

    /// An engine over a throwaway XDG root, holding one wallpaper that exists
    /// on disk. Probing is instant so tests assert behaviour, not timing.
    fn engine_with(backend: FakeBackend) -> (DisplayEngine, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let paths = MuralisPaths {
            config_dir: tmp.path().join("config"),
            data_dir: tmp.path().join("data"),
            cache_dir: tmp.path().join("cache"),
        };
        paths.ensure_dirs().unwrap();

        let file = paths.wallpapers_dir().join("wp1.jpg");
        std::fs::write(&file, b"not really a jpeg").unwrap();

        let mut engine = DisplayEngine::new(Config::default(), paths, Box::new(backend));
        engine.readiness = ReadinessPolicy {
            attempts: 5,
            initial_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
        };
        engine.wallpapers = vec![Wallpaper {
            id: "wp1".into(),
            source_type: SourceType::new("test"),
            source_id: "wp1".into(),
            source_url: None,
            width: 5120,
            height: 1440,
            tags: Vec::new(),
            file_path: file.to_string_lossy().into_owned(),
            added_at: "2026-09-10T00:00:00Z".into(),
            last_used: None,
            use_count: 0,
        }];

        (engine, tmp)
    }

    #[tokio::test]
    async fn failed_apply_is_visible_in_status() {
        let (mut engine, _tmp) = engine_with(FakeBackend::ready_at(0));

        engine.apply_current().await;

        let status = engine.status();
        assert!(
            status.current_wallpaper.is_none(),
            "nothing was applied, so nothing is current"
        );
        assert!(
            status
                .last_error
                .as_deref()
                .is_some_and(|e| e.contains("socket not bound")),
            "the backend failure should reach status, got: {:?}",
            status.last_error
        );
    }

    #[tokio::test]
    async fn startup_waits_for_a_backend_that_is_still_coming_up() {
        let (mut engine, _tmp) = engine_with(FakeBackend::ready_at(3));
        engine.mode = DisplayMode::RandomStartup;

        engine.startup_apply().await;

        let status = engine.status();
        assert_eq!(
            status.current_wallpaper.as_deref(),
            Some("wp1"),
            "the startup apply must survive losing the race with the backend daemon"
        );
        assert!(status.last_error.is_none());
    }

    #[tokio::test]
    async fn startup_against_an_absent_backend_says_so_instead_of_going_quiet() {
        let (mut engine, _tmp) = engine_with(FakeBackend::ready_at(0));
        engine.mode = DisplayMode::RandomStartup;

        engine.startup_apply().await;

        let status = engine.status();
        assert!(status.current_wallpaper.is_none());
        assert!(
            status.last_error.is_some(),
            "a lost startup must be reportable, not only a warn! nobody reads"
        );
    }

    #[tokio::test]
    async fn an_empty_library_does_not_wait_on_the_backend() {
        let (mut engine, _tmp) = engine_with(FakeBackend::ready_at(0));
        engine.mode = DisplayMode::RandomStartup;
        engine.wallpapers.clear();

        engine.startup_apply().await;

        assert!(
            engine.status().last_error.is_none(),
            "with nothing to apply there is nothing to wait for, and no failure to report"
        );
    }

    #[tokio::test]
    async fn a_failed_workspace_switch_is_reported_like_any_other() {
        let (mut engine, _tmp) = engine_with(FakeBackend::ready_at(0));
        engine.mode = DisplayMode::Workspace;
        engine.config.workspaces = vec![muralis_core::config::WorkspaceConfig {
            workspace: 1,
            wallpaper: "wp1".into(),
        }];

        engine.handle_workspace_change(1).await;

        assert!(engine.status().current_wallpaper.is_none());
        assert!(engine.status().last_error.is_some());
    }

    #[tokio::test]
    async fn a_later_success_clears_the_recorded_failure() {
        let backend = FakeBackend::ready_at(2);
        let (mut engine, _tmp) = engine_with(backend);

        // first apply happens before any probe, so the backend is still down
        engine.apply_current().await;
        assert!(engine.status().last_error.is_some());

        engine.startup_apply().await;

        let status = engine.status();
        assert_eq!(status.current_wallpaper.as_deref(), Some("wp1"));
        assert!(
            status.last_error.is_none(),
            "a stale failure must not outlive the wallpaper that fixed it"
        );
    }
}
