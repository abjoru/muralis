use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use iced::widget::image::Handle as ImageHandle;
use iced::widget::{button, column, container, row, text, Svg};
use iced::{Element, Length, Task, Theme};

use muralis_core::config::Config;
use muralis_core::crop_overlay;
use muralis_core::db::Database;
use muralis_core::ipc::{self, DaemonStatus, IpcRequest};
use muralis_core::models::{Wallpaper, WallpaperPreview};
use muralis_core::paths::MuralisPaths;
use muralis_core::sources::SourceRegistry;
use muralis_core::wallpapers::WallpaperManager;

use crate::message::{AspectRatioFilter, Message, Tab};
use crate::views;

fn build_registry(sources: &toml::Table, client: &reqwest::Client) -> SourceRegistry {
    let mut registry = SourceRegistry::new();
    for s in muralis_source_wallhaven::create_sources(sources, client.clone()) {
        registry.register(s);
    }
    for s in muralis_source_unsplash::create_sources(sources, client.clone()) {
        registry.register(s);
    }
    for s in muralis_source_pexels::create_sources(sources, client.clone()) {
        registry.register(s);
    }
    for s in muralis_source_feed::create_sources(sources, client.clone()) {
        registry.register(s);
    }
    registry
}

pub struct App {
    active_tab: Tab,
    search_query: String,
    current_page: u32,
    page_input_str: String,
    loading: bool,
    favorites: Vec<Wallpaper>,
    source_names: Vec<String>,
    source_results: HashMap<String, Vec<WallpaperPreview>>,
    registry: Arc<SourceRegistry>,
    http: reqwest::Client,
    thumbnail_cache: HashMap<String, ImageHandle>,
    thumbnail_order: VecDeque<String>,
    selected_index: Option<usize>,
    multi_selected: HashSet<usize>,
    select_anchor: Option<usize>,
    preview_handle: Option<ImageHandle>,
    preview_loading: bool,
    preview_bytes: Option<Vec<u8>>,
    window_size: (f32, f32),
    monitor_dims: (u32, u32),
    aspect_ratio_filter: AspectRatioFilter,
    crop_overlay_active: bool,
    crop_overlay_handle: Option<ImageHandle>,
    daemon_status: Option<DaemonStatus>,
    error_message: Option<String>,
    thumbnail_zoom: f32,
    zoom_generation: u32,
    settings_open: bool,
    config: Config,
    paths: MuralisPaths,
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let paths = MuralisPaths::new().expect("failed to resolve XDG paths");
        paths.ensure_dirs().expect("failed to create directories");
        let _ = paths.install_icon();
        let config = Config::load_or_default(&paths);

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("muralis/0.1")
            .build()
            .unwrap_or_default();

        let registry = build_registry(&config.sources, &http);
        let source_names: Vec<String> = registry.names().into_iter().map(String::from).collect();
        let registry = Arc::new(registry);

        let thumbnail_zoom = config.general.thumbnail_zoom.clamp(0.5, 2.5);

        let app = Self {
            active_tab: Tab::Favorites,
            search_query: String::new(),
            current_page: 1,
            page_input_str: "1".into(),
            loading: false,
            favorites: Vec::new(),
            source_names,
            source_results: HashMap::new(),
            registry,
            http,
            thumbnail_cache: HashMap::new(),
            thumbnail_order: VecDeque::new(),
            selected_index: None,
            multi_selected: HashSet::new(),
            select_anchor: None,
            preview_handle: None,
            preview_loading: false,
            preview_bytes: None,
            window_size: (1200.0, 800.0),
            monitor_dims: (1920, 1080),
            aspect_ratio_filter: AspectRatioFilter::All,
            crop_overlay_active: true,
            crop_overlay_handle: None,
            daemon_status: None,
            error_message: None,
            thumbnail_zoom,
            zoom_generation: 0,
            settings_open: false,
            config,
            paths,
        };

        let p = app.paths.clone();
        let load = Task::perform(load_favorites(p), |result| match result {
            Ok(wps) => Message::FavoritesLoaded(wps),
            Err(e) => Message::Error(e.to_string()),
        });

        let detect = Task::perform(detect_monitors(), |dims| {
            let (w, h) = dims.unwrap_or((1920, 1080));
            Message::MonitorsDetected(w, h)
        });

        (app, Task::batch([load, detect]))
    }

    fn items_per_page(&self) -> u32 {
        let (w, h) = self.window_size;
        let padding = 16.0;
        let spacing = 8.0;
        let thumb_w = 220.0 * self.thumbnail_zoom;
        let thumb_h = match self.aspect_ratio_filter.ratio_value() {
            Some(r) => thumb_w / r as f32,
            None => thumb_w * 9.0 / 16.0,
        };
        let chrome = 120.0; // tab bar + search bar + pagination
        let cols = ((w - 2.0 * padding) / (thumb_w + spacing)).floor().max(1.0);
        let rows = ((h - chrome) / (thumb_h + spacing)).floor().max(1.0);
        (cols * rows) as u32
    }

    fn clear_preview(&mut self) {
        self.selected_index = None;
        self.preview_handle = None;
        self.preview_loading = false;
        self.preview_bytes = None;
        self.crop_overlay_handle = None;
    }

    fn clear_selection(&mut self) {
        self.multi_selected.clear();
        self.select_anchor = None;
    }

    fn cache_thumbnail(&mut self, id: String, handle: ImageHandle) {
        if !self.thumbnail_cache.contains_key(&id) {
            self.thumbnail_order.push_back(id.clone());
        }
        self.thumbnail_cache.insert(id, handle);
        while self.thumbnail_cache.len() > 500 {
            if let Some(old) = self.thumbnail_order.pop_front() {
                self.thumbnail_cache.remove(&old);
            } else {
                break;
            }
        }
    }

    pub fn theme(&self) -> Theme {
        Theme::GruvboxDark
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        let resize = iced::event::listen_with(|event, _status, _window| match event {
            iced::event::Event::Window(iced::window::Event::Resized(size)) => {
                Some(Message::WindowResized(size.width, size.height))
            }
            _ => None,
        });

        let escape = if self.selected_index.is_some() {
            iced::event::listen_with(|event, _status, _window| match event {
                iced::event::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    ..
                }) => Some(Message::ClosePreview),
                _ => None,
            })
        } else if self.settings_open {
            iced::event::listen_with(|event, _status, _window| match event {
                iced::event::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    ..
                }) => Some(Message::ToggleSettings),
                _ => None,
            })
        } else {
            iced::Subscription::none()
        };

        iced::Subscription::batch([resize, escape])
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TabSelected(tab) => {
                self.active_tab = tab;
                self.clear_preview();
                self.clear_selection();
                if self.active_tab == Tab::Favorites {
                    let p = self.paths.clone();
                    return Task::perform(load_favorites(p), |result| match result {
                        Ok(wps) => Message::FavoritesLoaded(wps),
                        Err(e) => Message::Error(e.to_string()),
                    });
                }
                if let Tab::Source(ref name) = self.active_tab {
                    if self.is_feed_source(name) {
                        return self.update(Message::SearchSubmit);
                    }
                }
                Task::none()
            }

            Message::SearchQueryChanged(query) => {
                self.search_query = query;
                Task::none()
            }

            Message::SearchSubmit => {
                let Tab::Source(ref name) = self.active_tab else {
                    return Task::none();
                };
                self.loading = true;
                self.error_message = None;
                let query = self.search_query.clone();
                let page = self.current_page;
                let per_page = self.items_per_page();
                let tab = self.active_tab.clone();
                let name = name.clone();
                let aspect = self.aspect_ratio_filter;
                let registry = Arc::clone(&self.registry);

                Task::perform(
                    async move { search_source(&registry, &name, &query, page, per_page, aspect).await },
                    move |result| match result {
                        Ok(results) => Message::SearchResults(tab.clone(), results),
                        Err(e) => Message::SearchError(e.to_string()),
                    },
                )
            }

            Message::SearchLoading(_) => Task::none(),

            Message::SearchResults(tab, results) => {
                self.loading = false;
                if results.is_empty() && self.current_page > 1 {
                    self.current_page -= 1;
                    self.page_input_str = self.current_page.to_string();
                    return Task::none();
                }
                let client = self.http.clone();
                let tasks: Vec<Task<Message>> = results
                    .iter()
                    .filter(|p| !self.thumbnail_cache.contains_key(&p.source_id))
                    .map(|p| {
                        let url = p.thumbnail_url.clone();
                        let id = p.source_id.clone();
                        let client = client.clone();
                        Task::perform(
                            async move {
                                let bytes = client.get(&url).send().await?.bytes().await?;
                                Ok::<_, reqwest::Error>((id, bytes.to_vec()))
                            },
                            |result| match result {
                                Ok((id, bytes)) => Message::ThumbnailLoaded(id, bytes),
                                Err(_) => Message::Noop,
                            },
                        )
                    })
                    .collect();

                if let Tab::Source(name) = tab {
                    self.source_results.insert(name, results);
                }
                self.clear_preview();
                self.clear_selection();

                Task::batch(tasks)
            }

            Message::SearchError(err) => {
                self.loading = false;
                tracing::warn!("search error: {err}");
                self.error_message = Some(err);
                Task::none()
            }

            Message::ClosePreview => {
                self.clear_preview();
                Task::none()
            }

            Message::ThumbnailClicked(idx) => {
                if self.selected_index == Some(idx) {
                    self.clear_preview();
                    return Task::none();
                }

                self.clear_preview();
                self.selected_index = Some(idx);
                self.preview_loading = true;

                let url = match &self.active_tab {
                    Tab::Favorites => self.favorites.get(idx).map(|wp| wp.file_path.clone()),
                    Tab::Source(name) => self
                        .source_results
                        .get(name)
                        .and_then(|r| r.get(idx))
                        .map(|p| p.full_url.clone()),
                };

                if let Some(url) = url {
                    if self.active_tab == Tab::Favorites {
                        Task::perform(async move { tokio::fs::read(&url).await.ok() }, |bytes| {
                            match bytes {
                                Some(b) => Message::PreviewLoaded(b),
                                None => Message::Error("failed to read local file".into()),
                            }
                        })
                    } else {
                        let client = self.http.clone();
                        Task::perform(
                            async move {
                                let resp = client.get(&url).send().await?;
                                let content_type = resp
                                    .headers()
                                    .get("content-type")
                                    .and_then(|v| v.to_str().ok())
                                    .unwrap_or("")
                                    .to_string();
                                let bytes = resp.bytes().await?.to_vec();
                                if !content_type.starts_with("image/") && bytes.len() < 100 {
                                    anyhow::bail!("response not an image: {content_type}");
                                }
                                Ok::<Vec<u8>, anyhow::Error>(bytes)
                            },
                            |result| match result {
                                Ok(bytes) => Message::PreviewLoaded(bytes),
                                Err(e) => Message::Error(format!("preview load failed: {e}")),
                            },
                        )
                    }
                } else {
                    self.preview_loading = false;
                    Task::none()
                }
            }

            Message::ThumbnailLoaded(id, bytes) => {
                self.cache_thumbnail(id, ImageHandle::from_bytes(bytes));
                Task::none()
            }

            Message::PreviewLoaded(bytes) => {
                self.preview_loading = false;
                self.preview_handle = Some(ImageHandle::from_bytes(bytes.clone()));
                self.preview_bytes = Some(bytes);

                let (img_w, img_h) = self.selected_image_dimensions();
                let (mon_w, mon_h) = self.monitor_dims;

                if crop_overlay::ratios_match(img_w, img_h, mon_w, mon_h, 0.01) {
                    return Task::none();
                }

                let overlay_bytes = self.preview_bytes.as_ref().unwrap().clone();
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            crop_overlay::generate_crop_overlay(&overlay_bytes, mon_w, mon_h, 0.3)
                        })
                        .await
                        .map_err(|e| anyhow::anyhow!(e))?
                    },
                    |result| match result {
                        Ok(overlay_bytes) => Message::CropOverlayReady(overlay_bytes),
                        Err(e) => {
                            tracing::warn!("crop overlay failed: {e}");
                            Message::Noop
                        }
                    },
                )
            }

            Message::Favorite(preview) => {
                let paths = self.paths.clone();
                let client = self.http.clone();
                Task::perform(
                    async move {
                        let data = client.get(&preview.full_url).send().await?.bytes().await?;
                        let result = tokio::task::spawn_blocking(move || {
                            let db = Database::open(&paths.db_path())?;
                            let mgr = WallpaperManager::new(paths);
                            mgr.favorite(&db, &preview, &data)
                        })
                        .await
                        .map_err(|e| anyhow::anyhow!(e))??;
                        Ok::<String, anyhow::Error>(result)
                    },
                    |result| match result {
                        Ok(id) => Message::Favorited(id),
                        Err(e) => Message::Error(e.to_string()),
                    },
                )
            }

            Message::Favorited(_id) => {
                let p = self.paths.clone();
                Task::perform(load_favorites(p), |result| match result {
                    Ok(wps) => Message::FavoritesLoaded(wps),
                    Err(e) => Message::Error(e.to_string()),
                })
            }

            Message::Unfavorite(id) => {
                let paths = self.paths.clone();
                Task::perform(
                    async move {
                        let id_clone = id.clone();
                        tokio::task::spawn_blocking(move || {
                            let db = Database::open(&paths.db_path())?;
                            let mgr = WallpaperManager::new(paths);
                            mgr.unfavorite(&db, &id_clone)?;
                            Ok::<String, anyhow::Error>(id_clone)
                        })
                        .await
                        .map_err(|e| anyhow::anyhow!(e))?
                    },
                    |result| match result {
                        Ok(id) => Message::Unfavorited(id),
                        Err(e) => Message::Error(e.to_string()),
                    },
                )
            }

            Message::Unfavorited(_id) => {
                self.clear_preview();
                let p = self.paths.clone();
                Task::perform(load_favorites(p), |result| match result {
                    Ok(wps) => Message::FavoritesLoaded(wps),
                    Err(e) => Message::Error(e.to_string()),
                })
            }

            Message::Apply(id) => Task::perform(
                async move {
                    let req = IpcRequest::SetWallpaper { id };
                    ipc::send_request(&req).await
                },
                |result| match result {
                    Ok(_) => Message::Applied,
                    Err(e) => Message::Error(e.to_string()),
                },
            ),

            Message::Applied => Task::none(),

            Message::Blacklist(preview) => {
                let paths = self.paths.clone();
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            let db = Database::open(&paths.db_path())?;
                            db.add_blacklist(&preview.source_id, &preview.source_type)?;
                            Ok::<(), anyhow::Error>(())
                        })
                        .await
                        .map_err(|e| anyhow::anyhow!(e))?
                    },
                    |result| match result {
                        Ok(()) => Message::Blacklisted,
                        Err(e) => Message::Error(e.to_string()),
                    },
                )
            }

            Message::Blacklisted => Task::none(),

            Message::FavoritesLoaded(wallpapers) => {
                let tasks: Vec<Task<Message>> = wallpapers
                    .iter()
                    .filter(|wp| !self.thumbnail_cache.contains_key(&wp.id))
                    .map(|wp| {
                        let id = wp.id.clone();
                        let paths = self.paths.clone();
                        Task::perform(
                            async move {
                                let thumb_path =
                                    paths.thumbnails_dir().join(format!("{id}_thumb.jpg"));
                                tokio::fs::read(&thumb_path)
                                    .await
                                    .map(|bytes| (id, bytes))
                                    .ok()
                            },
                            |result| match result {
                                Some((id, bytes)) => Message::ThumbnailLoaded(id, bytes),
                                None => Message::Noop,
                            },
                        )
                    })
                    .collect();

                self.favorites = wallpapers;
                Task::batch(tasks)
            }

            Message::DaemonStatusUpdate(status) => {
                self.daemon_status = status;
                Task::none()
            }

            Message::ToggleSelect(idx) => {
                if self.multi_selected.contains(&idx) {
                    self.multi_selected.remove(&idx);
                } else {
                    self.multi_selected.insert(idx);
                }
                self.select_anchor = Some(idx);
                Task::none()
            }

            Message::RangeSelect(idx) => {
                if let Some(anchor) = self.select_anchor {
                    let lo = anchor.min(idx);
                    let hi = anchor.max(idx);
                    for i in lo..=hi {
                        self.multi_selected.insert(i);
                    }
                } else {
                    self.multi_selected.insert(idx);
                    self.select_anchor = Some(idx);
                }
                Task::none()
            }

            Message::ClearSelection => {
                self.clear_selection();
                Task::none()
            }

            Message::BatchFavorite => {
                let results = self.active_source_results();
                let indices: Vec<usize> = self.multi_selected.iter().copied().collect();
                let client = self.http.clone();
                let tasks: Vec<Task<Message>> = indices
                    .into_iter()
                    .filter_map(|i| results.get(i).cloned())
                    .map(|preview| {
                        let paths = self.paths.clone();
                        let client = client.clone();
                        Task::perform(
                            async move {
                                let data =
                                    client.get(&preview.full_url).send().await?.bytes().await?;
                                let result = tokio::task::spawn_blocking(move || {
                                    let db = Database::open(&paths.db_path())?;
                                    let mgr = WallpaperManager::new(paths);
                                    mgr.favorite(&db, &preview, &data)
                                })
                                .await
                                .map_err(|e| anyhow::anyhow!(e))??;
                                Ok::<String, anyhow::Error>(result)
                            },
                            |result| match result {
                                Ok(id) => Message::Favorited(id),
                                Err(e) => Message::Error(e.to_string()),
                            },
                        )
                    })
                    .collect();
                self.clear_selection();
                Task::batch(tasks)
            }

            Message::BatchBlacklist => {
                let results = self.active_source_results();
                let indices: Vec<usize> = self.multi_selected.iter().copied().collect();
                let tasks: Vec<Task<Message>> = indices
                    .into_iter()
                    .filter_map(|i| results.get(i).cloned())
                    .map(|preview| {
                        let paths = self.paths.clone();
                        Task::perform(
                            async move {
                                tokio::task::spawn_blocking(move || {
                                    let db = Database::open(&paths.db_path())?;
                                    db.add_blacklist(&preview.source_id, &preview.source_type)?;
                                    Ok::<(), anyhow::Error>(())
                                })
                                .await
                                .map_err(|e| anyhow::anyhow!(e))?
                            },
                            |result| match result {
                                Ok(()) => Message::Blacklisted,
                                Err(e) => Message::Error(e.to_string()),
                            },
                        )
                    })
                    .collect();
                self.clear_selection();
                Task::batch(tasks)
            }

            Message::BatchUnfavorite => {
                let indices: Vec<usize> = self.multi_selected.iter().copied().collect();
                let tasks: Vec<Task<Message>> = indices
                    .into_iter()
                    .filter_map(|i| self.favorites.get(i).map(|wp| wp.id.clone()))
                    .map(|id| {
                        let paths = self.paths.clone();
                        Task::perform(
                            async move {
                                let id_clone = id.clone();
                                tokio::task::spawn_blocking(move || {
                                    let db = Database::open(&paths.db_path())?;
                                    let mgr = WallpaperManager::new(paths);
                                    mgr.unfavorite(&db, &id_clone)?;
                                    Ok::<String, anyhow::Error>(id_clone)
                                })
                                .await
                                .map_err(|e| anyhow::anyhow!(e))?
                            },
                            |result| match result {
                                Ok(id) => Message::Unfavorited(id),
                                Err(e) => Message::Error(e.to_string()),
                            },
                        )
                    })
                    .collect();
                self.clear_selection();
                Task::batch(tasks)
            }

            Message::NextPage => {
                self.current_page += 1;
                self.page_input_str = self.current_page.to_string();
                self.update(Message::SearchSubmit)
            }

            Message::PrevPage => {
                if self.current_page > 1 {
                    self.current_page -= 1;
                    self.page_input_str = self.current_page.to_string();
                    self.update(Message::SearchSubmit)
                } else {
                    Task::none()
                }
            }

            Message::PageInputChanged(s) => {
                self.page_input_str = s.chars().filter(|c| c.is_ascii_digit()).collect();
                Task::none()
            }

            Message::PageInputSubmit => {
                if let Ok(n) = self.page_input_str.parse::<u32>() {
                    if n >= 1 {
                        self.current_page = n;
                        self.page_input_str = self.current_page.to_string();
                        return self.update(Message::SearchSubmit);
                    }
                }
                self.page_input_str = self.current_page.to_string();
                Task::none()
            }

            Message::AspectFilterChanged(filter) => {
                self.aspect_ratio_filter = filter;
                self.clear_preview();
                Task::none()
            }

            Message::MonitorsDetected(w, h) => {
                self.monitor_dims = (w, h);
                self.aspect_ratio_filter = AspectRatioFilter::from_dimensions(w, h);
                Task::none()
            }

            Message::ToggleCropOverlay => {
                self.crop_overlay_active = !self.crop_overlay_active;
                Task::none()
            }

            Message::CropOverlayReady(bytes) => {
                self.crop_overlay_handle = Some(ImageHandle::from_bytes(bytes));
                Task::none()
            }

            Message::ToggleSettings => {
                self.settings_open = !self.settings_open;
                if self.settings_open {
                    self.clear_preview();
                }
                Task::none()
            }

            Message::SettingsModeChanged(mode) => {
                self.config.display.mode = mode;
                let config = self.config.clone();
                let paths = self.paths.clone();
                let mut tasks = vec![Task::perform(
                    async move { save_config(config, paths).await },
                    |result| match result {
                        Ok(()) => Message::ConfigSaved,
                        Err(e) => Message::Error(e.to_string()),
                    },
                )];
                if self.daemon_status.is_some() {
                    tasks.push(Task::perform(
                        async move { ipc::send_request(&IpcRequest::SetMode { mode }).await },
                        |result| Message::DaemonIpcResult(result.map_err(|e| e.to_string())),
                    ));
                }
                Task::batch(tasks)
            }

            Message::SettingsBackendChanged(backend) => {
                self.config.general.backend = backend;
                let config = self.config.clone();
                let paths = self.paths.clone();
                Task::perform(
                    async move { save_config(config, paths).await },
                    |result| match result {
                        Ok(()) => Message::ConfigSaved,
                        Err(e) => Message::Error(e.to_string()),
                    },
                )
            }

            Message::SettingsIntervalChanged(interval) => {
                self.config.display.interval = interval;
                let config = self.config.clone();
                let paths = self.paths.clone();
                let mut tasks = vec![Task::perform(
                    async move { save_config(config, paths).await },
                    |result| match result {
                        Ok(()) => Message::ConfigSaved,
                        Err(e) => Message::Error(e.to_string()),
                    },
                )];
                if self.daemon_status.is_some() {
                    tasks.push(Task::perform(
                        async move { ipc::send_request(&IpcRequest::Reload).await },
                        |result| Message::DaemonIpcResult(result.map_err(|e| e.to_string())),
                    ));
                }
                Task::batch(tasks)
            }

            Message::DaemonNext => Task::perform(
                async move { ipc::send_request(&IpcRequest::Next).await },
                |result| Message::DaemonIpcResult(result.map_err(|e| e.to_string())),
            ),

            Message::DaemonPrev => Task::perform(
                async move { ipc::send_request(&IpcRequest::Prev).await },
                |result| Message::DaemonIpcResult(result.map_err(|e| e.to_string())),
            ),

            Message::DaemonTogglePause => {
                let paused = self
                    .daemon_status
                    .as_ref()
                    .map(|s| s.paused)
                    .unwrap_or(false);
                let req = if paused {
                    IpcRequest::Resume
                } else {
                    IpcRequest::Pause
                };
                Task::perform(async move { ipc::send_request(&req).await }, |result| {
                    Message::DaemonIpcResult(result.map_err(|e| e.to_string()))
                })
            }

            Message::DaemonIpcResult(Err(e)) => {
                self.error_message = Some(e);
                Task::none()
            }

            Message::DaemonIpcResult(Ok(_)) => Task::none(),

            Message::ConfigSaved => Task::none(),

            Message::ZoomChanged(zoom) => {
                self.thumbnail_zoom = zoom.clamp(0.5, 2.5);
                self.config.general.thumbnail_zoom = self.thumbnail_zoom;
                self.zoom_generation += 1;
                let gen = self.zoom_generation;
                Task::perform(
                    async move {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        gen
                    },
                    Message::ZoomSave,
                )
            }

            Message::ZoomSave(gen) => {
                if gen == self.zoom_generation {
                    let config = self.config.clone();
                    let paths = self.paths.clone();
                    Task::perform(async move { save_config(config, paths).await }, |result| {
                        match result {
                            Ok(()) => Message::ConfigSaved,
                            Err(e) => Message::Error(e.to_string()),
                        }
                    })
                } else {
                    Task::none()
                }
            }

            Message::WindowResized(w, h) => {
                self.window_size = (w, h);
                Task::none()
            }

            Message::Error(err) => {
                self.loading = false;
                self.preview_loading = false;
                tracing::error!("error: {err}");
                self.error_message = Some(err);
                Task::none()
            }

            Message::Noop => Task::none(),
        }
    }

    fn selected_image_dimensions(&self) -> (u32, u32) {
        if let Some(idx) = self.selected_index {
            match &self.active_tab {
                Tab::Favorites => self
                    .favorites
                    .get(idx)
                    .map(|wp| (wp.width, wp.height))
                    .unwrap_or((0, 0)),
                Tab::Source(name) => self
                    .source_results
                    .get(name)
                    .and_then(|r| r.get(idx))
                    .map(|p| (p.width, p.height))
                    .unwrap_or((0, 0)),
            }
        } else {
            (0, 0)
        }
    }

    fn is_feed_source(&self, name: &str) -> bool {
        self.registry
            .get(name)
            .map(|s| s.source_type() == "feed")
            .unwrap_or(false)
    }

    fn active_source_results(&self) -> &[WallpaperPreview] {
        match &self.active_tab {
            Tab::Source(name) => self
                .source_results
                .get(name)
                .map(|v| v.as_slice())
                .unwrap_or(&[]),
            _ => &[],
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let logo = Svg::new(iced::widget::svg::Handle::from_memory(
            include_bytes!("../../assets/muralis.svg").as_slice(),
        ))
        .width(28)
        .height(28);

        let gear_style = if self.settings_open {
            button::primary
        } else {
            button::text
        };
        let gear_btn = button(text("\u{2699}").size(18))
            .on_press(Message::ToggleSettings)
            .style(gear_style)
            .padding([4, 8]);

        let mut tab_row = row![
            logo,
            text("Muralis")
                .size(16)
                .color(iced::Color::from_rgb8(0xa8, 0x99, 0x84)),
            gear_btn,
            text("|").color(iced::Color::from_rgb8(0x66, 0x5c, 0x54)),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);

        // Favorites tab
        let fav_style = if self.active_tab == Tab::Favorites {
            button::primary
        } else {
            button::text
        };
        tab_row = tab_row.push(
            button(text("Favorites"))
                .on_press(Message::TabSelected(Tab::Favorites))
                .style(fav_style)
                .padding([8, 20]),
        );

        // Dynamic source tabs
        for name in &self.source_names {
            let tab = Tab::Source(name.clone());
            let style = if self.active_tab == tab {
                button::primary
            } else {
                button::text
            };
            tab_row = tab_row.push(
                button(text(name.as_str()))
                    .on_press(Message::TabSelected(tab))
                    .style(style)
                    .padding([8, 20]),
            );
        }

        let tabs = container(tab_row.padding([8, 8])).width(Length::Fill);

        let content = if self.settings_open {
            views::settings::view(&self.config, &self.daemon_status)
        } else {
            let (img_w, img_h) = self.selected_image_dimensions();
            let (mon_w, mon_h) = self.monitor_dims;
            let crop_needed = self.selected_index.is_some()
                && !crop_overlay::ratios_match(img_w, img_h, mon_w, mon_h, 0.01);

            let (content, preview) = match &self.active_tab {
                Tab::Favorites => {
                    let view = views::favorites::view(
                        &self.favorites,
                        &self.thumbnail_cache,
                        self.selected_index,
                        &self.multi_selected,
                        self.thumbnail_zoom,
                    );
                    let preview = views::favorites::preview_content(
                        &self.favorites,
                        self.selected_index,
                        &self.preview_handle,
                        self.preview_loading,
                        self.crop_overlay_active,
                        &self.crop_overlay_handle,
                        crop_needed,
                    );
                    (view, preview)
                }
                Tab::Source(name) => {
                    let results = self
                        .source_results
                        .get(name)
                        .map(|v| v.as_slice())
                        .unwrap_or(&[]);
                    let is_feed = self.is_feed_source(name);
                    let view = views::source_tab::view(
                        &self.search_query,
                        results,
                        &self.thumbnail_cache,
                        self.selected_index,
                        self.loading,
                        self.current_page,
                        &self.page_input_str,
                        &self.multi_selected,
                        self.aspect_ratio_filter,
                        is_feed,
                        self.thumbnail_zoom,
                    );
                    let preview = views::source_tab::preview_content(
                        results,
                        self.selected_index,
                        &self.preview_handle,
                        self.preview_loading,
                        self.crop_overlay_active,
                        &self.crop_overlay_handle,
                        crop_needed,
                    );
                    (view, preview)
                }
            };
            views::preview_overlay::wrap_with_overlay(content, preview)
        };

        let status_bar = {
            let status_text = if let Some(ref err) = self.error_message {
                format!("Error: {err}")
            } else {
                match &self.daemon_status {
                    Some(s) => format!(
                        "Daemon: {} | Mode: {} | Wallpapers: {}",
                        if s.paused { "paused" } else { "running" },
                        s.mode,
                        s.wallpaper_count
                    ),
                    None => "Daemon: not connected".into(),
                }
            };
            container(text(status_text).size(12))
                .padding([4, 8])
                .width(Length::Fill)
                .style(container::bordered_box)
        };

        column![tabs, content, status_bar]
            .height(Length::Fill)
            .into()
    }
}

async fn load_favorites(paths: MuralisPaths) -> anyhow::Result<Vec<Wallpaper>> {
    tokio::task::spawn_blocking(move || {
        let db = Database::open(&paths.db_path())?;
        let wps = db.list_wallpapers()?;
        Ok(wps)
    })
    .await?
}

async fn search_source(
    registry: &SourceRegistry,
    name: &str,
    query: &str,
    page: u32,
    per_page: u32,
    aspect: AspectRatioFilter,
) -> anyhow::Result<Vec<WallpaperPreview>> {
    let source = registry
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("source {name} not configured"))?;
    source
        .search(query, page, per_page, aspect)
        .await
        .map_err(|e| e.into())
}

async fn save_config(config: Config, paths: MuralisPaths) -> anyhow::Result<()> {
    tokio::task::spawn_blocking(move || config.save(&paths))
        .await
        .map_err(|e| anyhow::anyhow!(e))?
        .map_err(|e| anyhow::anyhow!(e))
}

async fn detect_monitors() -> Option<(u32, u32)> {
    // Try hyprctl first (Hyprland)
    let output = tokio::process::Command::new("hyprctl")
        .args(["monitors", "-j"])
        .output()
        .await
        .ok()?;

    if output.status.success() {
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
        let monitors = json.as_array()?;
        // Use first monitor
        let mon = monitors.first()?;
        let w = mon.get("width")?.as_u64()? as u32;
        let h = mon.get("height")?.as_u64()? as u32;
        return Some((w, h));
    }

    None
}
