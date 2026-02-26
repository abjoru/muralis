use anyhow::Result;
use clap::{Parser, Subcommand};
use serde::Serialize;

use muralis_core::config::Config;
use muralis_core::db::Database;
use muralis_core::ipc::{self, IpcRequest, IpcResponse};
use muralis_core::models::DisplayMode;
use muralis_core::paths::MuralisPaths;
use muralis_core::sources::{AspectRatioFilter, SourceRegistry, WallpaperSource};
use muralis_core::wallpapers::WallpaperManager;

#[derive(Parser)]
#[command(name = "muralis", about = "Wallpaper manager for Hyprland")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show daemon status
    Status,
    /// Next wallpaper
    Next,
    /// Previous wallpaper
    Prev,
    /// Set specific wallpaper by ID or path
    Set {
        /// Wallpaper ID or file path
        id: String,
    },
    /// Switch display mode
    Mode {
        /// Mode: static, random, random_startup, sequential, workspace, schedule
        mode: String,
    },
    /// Pause wallpaper rotation
    Pause,
    /// Resume wallpaper rotation
    Resume,
    /// Reload config
    Reload,
    /// Search wallpaper sources
    Search {
        /// Search query (empty for browse-all)
        query: Option<String>,
        /// Source name filter
        #[arg(long)]
        source: Option<String>,
        /// Page number
        #[arg(long, default_value = "1")]
        page: u32,
        /// Results per page
        #[arg(long, default_value = "24")]
        per_page: u32,
        /// Aspect ratio filter (all, 16x9, 21x9, 32x9, 16x10, 4x3, 3x2)
        #[arg(long, default_value = "all")]
        aspect: String,
    },
    /// Manage favorites
    Favorites {
        #[command(subcommand)]
        action: FavoritesAction,
    },
    /// Manage sources
    Sources {
        #[command(subcommand)]
        action: SourcesAction,
    },
    /// Manage cache
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },
    /// Stop the daemon
    Quit,
}

#[derive(Subcommand)]
enum CacheAction {
    /// Show cache size stats
    Stats,
    /// Prune cache to configured max size
    Prune,
}

#[derive(Subcommand)]
enum FavoritesAction {
    /// List all favorites
    List,
    /// Show favorites stats
    Stats,
    /// Add a wallpaper by URL
    Add {
        /// Wallpaper URL (e.g. https://wallhaven.cc/w/abc123)
        url: String,
    },
}

#[derive(Subcommand)]
enum SourcesAction {
    /// List configured sources
    List,
}

#[derive(Serialize)]
struct SearchOutput {
    results: Vec<SearchResult>,
    page: u32,
    per_page: u32,
    has_more: bool,
}

#[derive(Serialize)]
struct SearchResult {
    source_type: String,
    source_id: String,
    source_url: String,
    thumbnail_url: String,
    full_url: String,
    width: u32,
    height: u32,
    tags: Vec<String>,
    is_favorited: bool,
}

#[derive(Serialize)]
struct SourceInfo {
    name: String,
    source_type: String,
}

fn build_registry(config: &Config) -> Result<(SourceRegistry, reqwest::Client)> {
    let client = reqwest::Client::new();
    let sources = &config.sources;
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

    Ok((registry, client))
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Status => {
            let resp = send(IpcRequest::Status).await?;
            print_response(resp);
        }
        Commands::Next => {
            let resp = send(IpcRequest::Next).await?;
            print_response(resp);
        }
        Commands::Prev => {
            let resp = send(IpcRequest::Prev).await?;
            print_response(resp);
        }
        Commands::Set { id } => {
            let resp = send(IpcRequest::SetWallpaper { id }).await?;
            print_response(resp);
        }
        Commands::Mode { mode } => {
            let display_mode: DisplayMode = mode.parse().map_err(|e: String| anyhow::anyhow!(e))?;
            let resp = send(IpcRequest::SetMode { mode: display_mode }).await?;
            print_response(resp);
        }
        Commands::Pause => {
            let resp = send(IpcRequest::Pause).await?;
            print_response(resp);
        }
        Commands::Resume => {
            let resp = send(IpcRequest::Resume).await?;
            print_response(resp);
        }
        Commands::Reload => {
            let resp = send(IpcRequest::Reload).await?;
            print_response(resp);
        }
        Commands::Search {
            query,
            source,
            page,
            per_page,
            aspect,
        } => {
            let paths = MuralisPaths::new()?;
            let config = Config::load(&paths)?;
            let (registry, _) = build_registry(&config)?;
            let db = Database::open(&paths.db_path())?;
            let aspect: AspectRatioFilter =
                aspect.parse().map_err(|e: String| anyhow::anyhow!(e))?;

            let mut all_results = Vec::new();

            // Search matching sources
            let sources: Vec<&dyn WallpaperSource> = if let Some(ref name) = source {
                registry
                    .iter()
                    .filter(|s| s.name() == name.as_str())
                    .collect()
            } else {
                registry.iter().collect()
            };

            let query = query.unwrap_or_default();

            for src in &sources {
                match src.search(&query, page, per_page, aspect).await {
                    Ok(previews) => {
                        for p in previews {
                            // Client-side aspect filter for sources that don't support it natively
                            if !aspect.matches(p.width, p.height) {
                                continue;
                            }
                            let is_favorited = db
                                .is_favorited_by_source(p.source_type.as_str(), &p.source_id)
                                .unwrap_or(false);
                            all_results.push(SearchResult {
                                source_type: p.source_type.to_string(),
                                source_id: p.source_id,
                                source_url: p.source_url,
                                thumbnail_url: p.thumbnail_url,
                                full_url: p.full_url,
                                width: p.width,
                                height: p.height,
                                tags: p.tags,
                                is_favorited,
                            });
                        }
                    }
                    Err(e) => {
                        eprintln!("warning: {} search failed: {e}", src.name());
                    }
                }
            }

            let has_more = all_results.len() >= per_page as usize;
            let output = SearchOutput {
                results: all_results,
                page,
                per_page,
                has_more,
            };
            println!("{}", serde_json::to_string(&output)?);
        }
        Commands::Favorites { action } => match action {
            FavoritesAction::List => {
                let paths = MuralisPaths::new()?;
                let db = Database::open(&paths.db_path())?;
                let wallpapers = db.list_wallpapers()?;
                println!("{}", serde_json::to_string(&wallpapers)?);
            }
            FavoritesAction::Stats => {
                let paths = MuralisPaths::new()?;
                let db = Database::open(&paths.db_path())?;
                let count = db.wallpaper_count()?;
                let disk_usage = dir_size(&paths.wallpapers_dir());
                println!("favorites: {count}");
                println!("disk usage: {}", format_bytes(disk_usage));
            }
            FavoritesAction::Add { url } => {
                let paths = MuralisPaths::new()?;
                let config = Config::load(&paths)?;
                let (registry, _) = build_registry(&config)?;
                let db = Database::open(&paths.db_path())?;
                let manager = WallpaperManager::new(paths);

                // Try each source's resolve_url
                let mut resolved = None;
                for src in registry.iter() {
                    match src.resolve_url(&url).await {
                        Ok(Some(preview)) => {
                            // Download the image
                            let data = src.download(&preview).await?;
                            let id = manager.favorite(&db, &preview, &data)?;
                            resolved = Some((id, preview));
                            break;
                        }
                        Ok(None) => continue,
                        Err(e) => {
                            eprintln!("warning: {} resolve failed: {e}", src.name());
                        }
                    }
                }

                match resolved {
                    Some((id, preview)) => {
                        let out = serde_json::json!({
                            "id": id,
                            "source_type": preview.source_type.to_string(),
                            "source_id": preview.source_id,
                            "source_url": preview.source_url,
                        });
                        println!("{}", serde_json::to_string(&out)?);
                    }
                    None => {
                        eprintln!("error: no source could resolve URL: {url}");
                        std::process::exit(1);
                    }
                }
            }
        },
        Commands::Sources { action } => match action {
            SourcesAction::List => {
                let paths = MuralisPaths::new()?;
                let config = Config::load(&paths)?;
                let (registry, _) = build_registry(&config)?;

                let sources: Vec<SourceInfo> = registry
                    .iter()
                    .map(|s| SourceInfo {
                        name: s.name().to_string(),
                        source_type: s.source_type().to_string(),
                    })
                    .collect();
                println!("{}", serde_json::to_string(&sources)?);
            }
        },
        Commands::Cache { action } => {
            let paths = MuralisPaths::new()?;
            match action {
                CacheAction::Stats => {
                    let stats = muralis_core::cache::cache_stats(&paths);
                    println!(
                        "thumbnails: {} ({} files)",
                        format_bytes(stats.thumbnails_size),
                        stats.thumbnail_count
                    );
                    println!(
                        "previews:   {} ({} files)",
                        format_bytes(stats.previews_size),
                        stats.preview_count
                    );
                    println!("total:      {}", format_bytes(stats.total_size));
                }
                CacheAction::Prune => {
                    let config = Config::load(&paths)?;
                    let max_bytes = config.general.cache_max_mb * 1024 * 1024;
                    let freed = muralis_core::cache::prune_cache(&paths, max_bytes)?;
                    if freed > 0 {
                        println!("freed {}", format_bytes(freed));
                    } else {
                        println!("cache within limit");
                    }
                }
            }
        }
        Commands::Quit => {
            let resp = send(IpcRequest::Quit).await?;
            print_response(resp);
        }
    }

    Ok(())
}

async fn send(request: IpcRequest) -> Result<IpcResponse> {
    ipc::send_request(&request)
        .await
        .map_err(|e| anyhow::anyhow!("daemon not running. start with: muralis-daemon\n  ({e})"))
}

fn print_response(resp: IpcResponse) {
    match resp {
        IpcResponse::Ok { data: Some(data) } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&data).unwrap_or_default()
            );
        }
        IpcResponse::Ok { data: None } => {
            println!("ok");
        }
        IpcResponse::Error { message } => {
            eprintln!("error: {message}");
            std::process::exit(1);
        }
    }
}

fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}
