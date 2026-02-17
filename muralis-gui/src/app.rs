use std::collections::{HashMap, HashSet};
use std::time::Duration;

use iced::widget::image::Handle as ImageHandle;
use iced::widget::{button, column, container, row, text};
use iced::{Element, Length, Task, Theme};

use muralis_core::config::Config;
use muralis_core::crop_overlay;
use muralis_core::db::Database;
use muralis_core::ipc::{self, DaemonStatus, IpcRequest};
use muralis_core::models::{Wallpaper, WallpaperPreview};
use muralis_core::paths::MuralisPaths;
use muralis_core::sources;
use muralis_core::wallpapers::WallpaperManager;

use crate::message::{AspectRatioFilter, Message, Tab};
use crate::views;

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap_or_default()
}

pub struct App {
    active_tab: Tab,
    search_query: String,
    current_page: u32,
    loading: bool,
    favorites: Vec<Wallpaper>,
    wallhaven_results: Vec<WallpaperPreview>,
    unsplash_results: Vec<WallpaperPreview>,
    pexels_results: Vec<WallpaperPreview>,
    feed_results: Vec<WallpaperPreview>,
    thumbnail_cache: HashMap<String, ImageHandle>,
    selected_index: Option<usize>,
    multi_selected: HashSet<usize>,
    select_anchor: Option<usize>,
    preview_handle: Option<ImageHandle>,
    preview_loading: bool,
    preview_bytes: Option<Vec<u8>>,
    monitor_dims: (u32, u32),
    aspect_ratio_filter: AspectRatioFilter,
    crop_overlay_active: bool,
    crop_overlay_handle: Option<ImageHandle>,
    daemon_status: Option<DaemonStatus>,
    error_message: Option<String>,
    config: Config,
    paths: MuralisPaths,
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let paths = MuralisPaths::new().expect("failed to resolve XDG paths");
        paths.ensure_dirs().expect("failed to create directories");
        let config = Config::load_or_default(&paths);

        let app = Self {
            active_tab: Tab::Favorites,
            search_query: String::new(),
            current_page: 1,
            loading: false,
            favorites: Vec::new(),
            wallhaven_results: Vec::new(),
            unsplash_results: Vec::new(),
            pexels_results: Vec::new(),
            feed_results: Vec::new(),
            thumbnail_cache: HashMap::new(),
            selected_index: None,
            multi_selected: HashSet::new(),
            select_anchor: None,
            preview_handle: None,
            preview_loading: false,
            preview_bytes: None,
            monitor_dims: (1920, 1080),
            aspect_ratio_filter: AspectRatioFilter::All,
            crop_overlay_active: true,
            crop_overlay_handle: None,
            daemon_status: None,
            error_message: None,
            config,
            paths,
        };

        let load = Task::perform(load_favorites(), |result| match result {
            Ok(wps) => Message::FavoritesLoaded(wps),
            Err(e) => Message::Error(e.to_string()),
        });

        let detect = Task::perform(detect_monitors(), |dims| {
            let (w, h) = dims.unwrap_or((1920, 1080));
            Message::MonitorsDetected(w, h)
        });

        (app, Task::batch([load, detect]))
    }

    pub fn theme(&self) -> Theme {
        Theme::GruvboxDark
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        if self.selected_index.is_some() {
            iced::event::listen_with(|event, _status, _window| match event {
                iced::event::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    ..
                }) => Some(Message::ClosePreview),
                _ => None,
            })
        } else {
            iced::Subscription::none()
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TabSelected(tab) => {
                self.active_tab = tab;
                self.selected_index = None;
                self.multi_selected.clear();
                self.select_anchor = None;
                self.preview_handle = None;
                self.preview_loading = false;
                self.preview_bytes = None;
                self.crop_overlay_handle = None;
                if self.active_tab == Tab::Favorites {
                    return Task::perform(load_favorites(), |result| match result {
                        Ok(wps) => Message::FavoritesLoaded(wps),
                        Err(e) => Message::Error(e.to_string()),
                    });
                }
                Task::none()
            }

            Message::SearchQueryChanged(query) => {
                self.search_query = query;
                Task::none()
            }

            Message::SearchSubmit => {
                self.loading = true;
                self.error_message = None;
                let query = self.search_query.clone();
                let page = self.current_page;
                let tab = self.active_tab.clone();
                let tab2 = tab.clone();
                let config = self.config.sources.clone();
                let aspect = self.aspect_ratio_filter;

                Task::perform(
                    async move { search_source(&tab, &config, &query, page, aspect).await },
                    move |result| match result {
                        Ok(results) => Message::SearchResults(tab2.clone(), results),
                        Err(e) => Message::SearchError(e.to_string()),
                    },
                )
            }

            Message::SearchLoading(_) => Task::none(),

            Message::SearchResults(tab, results) => {
                self.loading = false;
                let client = http_client();
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

                match tab {
                    Tab::Wallhaven => self.wallhaven_results = results,
                    Tab::Unsplash => self.unsplash_results = results,
                    Tab::Pexels => self.pexels_results = results,
                    Tab::Feeds => self.feed_results = results,
                    _ => {}
                }
                self.selected_index = None;
                self.preview_handle = None;
                self.preview_bytes = None;
                self.crop_overlay_handle = None;

                Task::batch(tasks)
            }

            Message::SearchError(err) => {
                self.loading = false;
                self.error_message = Some(err.clone());
                tracing::warn!("search error: {err}");
                Task::none()
            }

            Message::ClosePreview => {
                self.selected_index = None;
                self.preview_handle = None;
                self.preview_loading = false;
                self.preview_bytes = None;
                self.crop_overlay_handle = None;
                Task::none()
            }

            Message::ThumbnailClicked(idx) => {
                if self.selected_index == Some(idx) {
                    self.selected_index = None;
                    self.preview_handle = None;
                    self.preview_loading = false;
                    self.preview_bytes = None;
                    self.crop_overlay_handle = None;
                    return Task::none();
                }

                self.selected_index = Some(idx);
                self.preview_handle = None;
                self.preview_loading = true;
                self.preview_bytes = None;
                self.crop_overlay_handle = None;

                let url = match &self.active_tab {
                    Tab::Favorites => self.favorites.get(idx).map(|wp| wp.file_path.clone()),
                    Tab::Wallhaven => self.wallhaven_results.get(idx).map(|p| p.full_url.clone()),
                    Tab::Unsplash => self.unsplash_results.get(idx).map(|p| p.full_url.clone()),
                    Tab::Pexels => self.pexels_results.get(idx).map(|p| p.full_url.clone()),
                    Tab::Feeds => self.feed_results.get(idx).map(|p| p.full_url.clone()),
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
                        let client = http_client();
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
                self.thumbnail_cache
                    .insert(id, ImageHandle::from_bytes(bytes));
                Task::none()
            }

            Message::PreviewLoaded(bytes) => {
                self.preview_loading = false;
                self.preview_handle = Some(ImageHandle::from_bytes(bytes.clone()));

                let (img_w, img_h) = self.selected_image_dimensions();
                let (mon_w, mon_h) = self.monitor_dims;

                if crop_overlay::ratios_match(img_w, img_h, mon_w, mon_h, 0.01) {
                    self.preview_bytes = Some(bytes);
                    return Task::none();
                }

                self.preview_bytes = Some(bytes.clone());
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            crop_overlay::generate_crop_overlay(&bytes, mon_w, mon_h, 0.3)
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
                let client = http_client();
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

            Message::Favorited(_id) => Task::perform(load_favorites(), |result| match result {
                Ok(wps) => Message::FavoritesLoaded(wps),
                Err(e) => Message::Error(e.to_string()),
            }),

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
                self.selected_index = None;
                self.preview_handle = None;
                self.preview_bytes = None;
                self.crop_overlay_handle = None;
                Task::perform(load_favorites(), |result| match result {
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
                self.multi_selected.clear();
                self.select_anchor = None;
                Task::none()
            }

            Message::BatchFavorite => {
                let results = self.active_source_results();
                let indices: Vec<usize> = self.multi_selected.iter().copied().collect();
                let client = http_client();
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
                self.multi_selected.clear();
                self.select_anchor = None;
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
                self.multi_selected.clear();
                self.select_anchor = None;
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
                self.multi_selected.clear();
                self.select_anchor = None;
                Task::batch(tasks)
            }

            Message::NextPage => {
                self.current_page += 1;
                self.update(Message::SearchSubmit)
            }

            Message::PrevPage => {
                if self.current_page > 1 {
                    self.current_page -= 1;
                    self.update(Message::SearchSubmit)
                } else {
                    Task::none()
                }
            }

            Message::AspectFilterChanged(filter) => {
                self.aspect_ratio_filter = filter;
                self.selected_index = None;
                self.preview_handle = None;
                self.preview_bytes = None;
                self.crop_overlay_handle = None;
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

            Message::Error(err) => {
                self.loading = false;
                self.preview_loading = false;
                self.error_message = Some(err.clone());
                tracing::error!("error: {err}");
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
                Tab::Wallhaven => self
                    .wallhaven_results
                    .get(idx)
                    .map(|p| (p.width, p.height))
                    .unwrap_or((0, 0)),
                Tab::Unsplash => self
                    .unsplash_results
                    .get(idx)
                    .map(|p| (p.width, p.height))
                    .unwrap_or((0, 0)),
                Tab::Pexels => self
                    .pexels_results
                    .get(idx)
                    .map(|p| (p.width, p.height))
                    .unwrap_or((0, 0)),
                Tab::Feeds => self
                    .feed_results
                    .get(idx)
                    .map(|p| (p.width, p.height))
                    .unwrap_or((0, 0)),
            }
        } else {
            (0, 0)
        }
    }

    fn active_source_results(&self) -> &[WallpaperPreview] {
        match &self.active_tab {
            Tab::Wallhaven => &self.wallhaven_results,
            Tab::Unsplash => &self.unsplash_results,
            Tab::Pexels => &self.pexels_results,
            Tab::Feeds => &self.feed_results,
            _ => &[],
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let tabs = container(
            row(Tab::ALL
                .iter()
                .map(|tab| {
                    let style = if *tab == self.active_tab {
                        button::primary
                    } else {
                        button::text
                    };
                    button(text(tab.label()))
                        .on_press(Message::TabSelected(tab.clone()))
                        .style(style)
                        .padding([8, 20])
                        .into()
                })
                .collect::<Vec<Element<Message>>>())
            .spacing(8)
            .padding([8, 8]),
        )
        .width(Length::Fill);

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
                    &self.preview_handle,
                    self.preview_loading,
                    &self.multi_selected,
                    self.crop_overlay_active,
                    &self.crop_overlay_handle,
                    crop_needed,
                    self.aspect_ratio_filter,
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
            tab => {
                let results = match tab {
                    Tab::Wallhaven => &self.wallhaven_results,
                    Tab::Unsplash => &self.unsplash_results,
                    Tab::Pexels => &self.pexels_results,
                    Tab::Feeds => &self.feed_results,
                    _ => unreachable!(),
                };
                let view = views::source_tab::view(
                    &self.search_query,
                    results,
                    &self.thumbnail_cache,
                    self.selected_index,
                    &self.preview_handle,
                    self.preview_loading,
                    self.loading,
                    self.current_page,
                    &self.multi_selected,
                    self.crop_overlay_active,
                    &self.crop_overlay_handle,
                    crop_needed,
                    self.aspect_ratio_filter,
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
        let content = views::preview_overlay::wrap_with_overlay(content, preview);

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

async fn load_favorites() -> anyhow::Result<Vec<Wallpaper>> {
    tokio::task::spawn_blocking(|| {
        let paths = MuralisPaths::new()?;
        let db = Database::open(&paths.db_path())?;
        let wps = db.list_wallpapers()?;
        Ok(wps)
    })
    .await?
}

async fn search_source(
    tab: &Tab,
    config: &muralis_core::config::SourcesConfig,
    query: &str,
    page: u32,
    aspect: AspectRatioFilter,
) -> anyhow::Result<Vec<WallpaperPreview>> {
    if matches!(tab, Tab::Feeds) {
        let client = muralis_core::sources::feed::FeedClient::new();
        let mut all = Vec::new();
        for feed_cfg in &config.feeds {
            if feed_cfg.enabled {
                match client.fetch_feed(feed_cfg).await {
                    Ok(mut results) => all.append(&mut results),
                    Err(e) => tracing::warn!("feed '{}' failed: {e}", feed_cfg.name),
                }
            }
        }
        return Ok(all);
    }

    let sources = sources::create_sources(config);
    let source_name = match tab {
        Tab::Wallhaven => "wallhaven",
        Tab::Unsplash => "unsplash",
        Tab::Pexels => "pexels",
        _ => return Ok(Vec::new()),
    };

    for source in &sources {
        if source.name() == source_name {
            return source.search(query, page, aspect).await.map_err(|e| e.into());
        }
    }

    anyhow::bail!("source {source_name} not configured")
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
