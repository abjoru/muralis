use anyhow::Result;
use clap::{Parser, Subcommand};

use muralis_core::ipc::{self, IpcRequest, IpcResponse};
use muralis_core::models::DisplayMode;

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
        /// Mode: static, random, sequential, workspace, schedule
        mode: String,
    },
    /// Pause wallpaper rotation
    Pause,
    /// Resume wallpaper rotation
    Resume,
    /// Reload config
    Reload,
    /// Manage favorites
    Favorites {
        #[command(subcommand)]
        action: FavoritesAction,
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
        Commands::Cache { action } => {
            let paths = muralis_core::paths::MuralisPaths::new()?;
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
                    let config = muralis_core::config::Config::load(&paths)?;
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
        Commands::Favorites { action } => match action {
            FavoritesAction::List => {
                let paths = muralis_core::paths::MuralisPaths::new()?;
                let db = muralis_core::db::Database::open(&paths.db_path())?;
                let wallpapers = db.list_wallpapers()?;
                let json = serde_json::to_string_pretty(&wallpapers)?;
                println!("{json}");
            }
            FavoritesAction::Stats => {
                let paths = muralis_core::paths::MuralisPaths::new()?;
                let db = muralis_core::db::Database::open(&paths.db_path())?;
                let count = db.wallpaper_count()?;
                let disk_usage = dir_size(&paths.wallpapers_dir());
                println!("favorites: {count}");
                println!("disk usage: {}", format_bytes(disk_usage));
            }
        },
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
