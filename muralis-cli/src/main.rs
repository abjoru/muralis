use anyhow::Result;
use clap::{Parser, Subcommand};
use serde::Serialize;

use muralis_core::config::Config;
use muralis_core::db::Database;
use muralis_core::ipc::{self, FavoritesPage, IpcRequest, IpcResponse};
use muralis_core::models::{DisplayMode, Wallpaper};
use muralis_core::paths::MuralisPaths;
use muralis_core::sources::{
    select_category, AspectRatioFilter, RetrievalMode, SourceCategory, SourceContext,
    SourceRegistry, WallpaperSource,
};
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
    /// Browse a source that takes no query (e.g. a feed)
    Browse {
        /// Source name, as reported by `muralis sources list`
        source: String,
        /// Category slug; omit for a source that publishes none
        #[arg(long)]
        category: Option<String>,
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
    /// Keep a result from `search` or `browse`, as the JSON object it emitted
    Keep {
        /// One result object; read from stdin when omitted
        preview: Option<String>,
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

/// One row of `sources list`. `retrieval_mode` and `categories` are additive:
/// existing fields keep their names and meanings, so a consumer that only
/// reads `name`/`source_type` is unaffected.
#[derive(Serialize)]
struct SourceInfo {
    name: String,
    source_type: String,
    retrieval_mode: RetrievalMode,
    categories: Vec<SourceCategory>,
}

/// Render **Previews** into the `search` JSON shape. `browse` emits the same
/// shape through the same function, so every existing consumer of a search
/// result works unchanged against a browse result.
fn search_output(
    previews: Vec<muralis_core::models::WallpaperPreview>,
    page: u32,
    per_page: u32,
    is_favorited: impl Fn(&str, &str) -> bool,
) -> SearchOutput {
    let results: Vec<SearchResult> = previews
        .into_iter()
        .map(|p| SearchResult {
            is_favorited: is_favorited(p.source_type.as_str(), &p.source_id),
            source_type: p.source_type.to_string(),
            source_id: p.source_id,
            source_url: p.source_url,
            thumbnail_url: p.thumbnail_url,
            full_url: p.full_url,
            width: p.width,
            height: p.height,
            tags: p.tags,
        })
        .collect();

    SearchOutput {
        has_more: results.len() >= per_page as usize,
        results,
        page,
        per_page,
    }
}

/// One result as `search` / `browse` emit it, read back. Every field a
/// **Preview** needs is required except the cosmetic ones; `is_favorited` and
/// anything else the emitter adds later is ignored, so a consumer can hand a
/// result straight back without stripping it.
#[derive(serde::Deserialize)]
struct PreviewInput {
    source_type: String,
    source_id: String,
    #[serde(default)]
    source_url: String,
    #[serde(default)]
    thumbnail_url: String,
    full_url: String,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(default)]
    tags: Vec<String>,
}

/// The **Preview** a single emitted result describes.
///
/// This is the keep-by-Preview path's whole input contract: a **Consumer**
/// that already holds a result does not serialise it down to a URL for a
/// **Source** to parse back out — a round-trip no **Browsed Source** can make,
/// because the `source_url` it publishes names a page, not an image.
fn preview_from_json(raw: &str) -> Result<muralis_core::models::WallpaperPreview> {
    let raw = raw.trim();
    if raw.is_empty() {
        anyhow::bail!("no preview given: pass one result object, or pipe it on stdin");
    }
    // A whole page handed over instead of one of its results is the likeliest
    // mistake, and its serde error ("missing field `source_type`") would not
    // say so.
    if let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(raw) {
        if map.contains_key("results") && !map.contains_key("source_type") {
            anyhow::bail!(
                "that is a whole page, not a result: pipe one element of `results` \
                 (e.g. jq -c '.results[0]')"
            );
        }
    }

    let input: PreviewInput = serde_json::from_str(raw)
        .map_err(|e| anyhow::anyhow!("not a search or browse result: {e}"))?;
    for (field, value) in [
        ("source_type", &input.source_type),
        ("source_id", &input.source_id),
        ("full_url", &input.full_url),
    ] {
        if value.trim().is_empty() {
            anyhow::bail!("preview is missing `{field}`");
        }
    }

    Ok(muralis_core::models::WallpaperPreview {
        source_type: muralis_core::models::SourceType::new(input.source_type),
        source_id: input.source_id,
        source_url: input.source_url,
        thumbnail_url: input.thumbnail_url,
        full_url: input.full_url,
        width: input.width,
        height: input.height,
        tags: input.tags,
    })
}

/// What keeping a **Preview** answers with, whichever path kept it — both
/// `favorites add` and `favorites keep` render through here, so a wallpaper is
/// indistinguishable afterwards from one kept the other way.
fn kept(id: &str, preview: &muralis_core::models::WallpaperPreview) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "source_type": preview.source_type.to_string(),
        "source_id": preview.source_id,
        "source_url": preview.source_url,
    })
}

/// What to tell a user whose pasted URL resolved to nothing.
///
/// A **Source** that recognises the URL as its own but cannot make an image of
/// it explains itself; only a URL no Source claims at all keeps the bare
/// "nothing resolved" message. The two are different mistakes and deserve
/// different words.
fn unresolvable_error(registry: &SourceRegistry, url: &str) -> String {
    registry
        .iter()
        .find_map(|s| s.explain_unresolvable(url))
        .unwrap_or_else(|| format!("no source could resolve URL: {url}"))
}

fn source_info(s: &dyn WallpaperSource) -> SourceInfo {
    SourceInfo {
        name: s.name().to_string(),
        source_type: s.source_type().to_string(),
        retrieval_mode: s.retrieval_mode(),
        categories: s.categories(),
    }
}

/// A **Source** name as it must be typed to name that Source again. Built-in
/// names are single words and are handed over verbatim; a user-chosen name — a
/// feed entry, a booru host — may hold spaces, and only then is it quoted, so
/// every command muralis prints runs as written.
fn as_typed(name: &str) -> std::borrow::Cow<'_, str> {
    if !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        std::borrow::Cow::Borrowed(name)
    } else {
        std::borrow::Cow::Owned(format!("'{}'", name.replace('\'', r"'\''")))
    }
}

/// The **Sources** a `search` invocation actually asks.
///
/// Unscoped, that is every **Searched** Source and no **Browsed** one — a feed
/// handed a query returns its whole contents unfiltered, so fanning out over
/// it interleaves non-answers with genuine matches. Naming a Browsed Source is
/// refused outright rather than answered with a query-ignoring result set.
fn sources_for_search<'a>(
    registry: &'a SourceRegistry,
    source: Option<&str>,
) -> Result<Vec<&'a dyn WallpaperSource>> {
    let Some(name) = source else {
        return Ok(registry.searched().collect());
    };
    let src = registry
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("unknown source: {name}"))?;
    if src.retrieval_mode() == RetrievalMode::Browsed {
        anyhow::bail!(
            "'{name}' is a browsed source and takes no query; use: muralis browse {typed}",
            typed = as_typed(name)
        );
    }
    Ok(vec![src])
}

/// The **Source** a `browse` invocation retrieves from. Browsing is by name
/// only — there is no fan-out, because a Browsed Source is not an answer to
/// anything a caller did not explicitly ask for.
fn browse_target<'a>(registry: &'a SourceRegistry, name: &str) -> Result<&'a dyn WallpaperSource> {
    let src = registry
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("unknown source: {name}"))?;
    if src.retrieval_mode() != RetrievalMode::Browsed {
        anyhow::bail!(
            "'{name}' is a searched source; use: muralis search <query> --source {typed}",
            typed = as_typed(name)
        );
    }
    Ok(src)
}

fn build_registry(config: &Config) -> Result<(SourceRegistry, reqwest::Client)> {
    let client = reqwest::Client::builder()
        .user_agent(muralis_core::http::user_agent())
        .build()?;
    let sources = &config.sources;
    // Build the global cross-cutting context once (ADR 0003): content-safety
    // ceiling from [general], min dimensions from [filter].
    let ctx = SourceContext {
        content_safety: config.general.content_safety,
        min_width: config.filter.min_width,
        min_height: config.filter.min_height,
    };
    let mut registry = SourceRegistry::new();

    for s in muralis_source_wallhaven::create_sources(sources, client.clone(), &ctx) {
        registry.register(s);
    }
    for s in muralis_source_unsplash::create_sources(sources, client.clone(), &ctx) {
        registry.register(s);
    }
    for s in muralis_source_pexels::create_sources(sources, client.clone(), &ctx) {
        registry.register(s);
    }
    for s in muralis_source_pixabay::create_sources(sources, client.clone(), &ctx) {
        registry.register(s);
    }
    for s in muralis_source_booru::create_sources(sources, client.clone(), &ctx) {
        registry.register(s);
    }
    for s in muralis_source_feed::create_sources(sources, client.clone(), &ctx) {
        registry.register(s);
    }
    for s in muralis_source_ultrawide::create_sources(sources, client.clone(), &ctx) {
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

            let sources = sources_for_search(&registry, source.as_deref())?;
            let query = query.unwrap_or_default();

            let mut previews = Vec::new();

            for src in &sources {
                match src.search(&query, page, per_page, aspect).await {
                    // Sources honor the aspect filter themselves now.
                    Ok(found) => previews.extend(found),
                    Err(e) => eprintln!("warning: {} search failed: {e}", src.name()),
                }
            }

            let output = search_output(previews, page, per_page, |ty, id| {
                db.is_favorited_by_source(ty, id).unwrap_or(false)
            });
            println!("{}", serde_json::to_string(&output)?);
        }
        Commands::Browse {
            source,
            category,
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

            let src = browse_target(&registry, &source)?;
            let category = select_category(src, category.as_deref())?;
            let previews = src
                .browse(category.as_deref(), page, per_page, aspect)
                .await?;

            let output = search_output(previews, page, per_page, |ty, id| {
                db.is_favorited_by_source(ty, id).unwrap_or(false)
            });
            println!("{}", serde_json::to_string(&output)?);
        }
        Commands::Favorites { action } => match action {
            FavoritesAction::List => {
                // Ask the daemon first — one backing store, one answer — and
                // read the database ourselves when it cannot answer.
                let served = ipc::send_request(&IpcRequest::Favorites {
                    offset: None,
                    limit: None,
                })
                .await;
                let wallpapers = match library_from_daemon(served) {
                    Some(wallpapers) => wallpapers,
                    None => {
                        let paths = MuralisPaths::new()?;
                        let db = Database::open(&paths.db_path())?;
                        db.list_wallpapers()?
                    }
                };
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
                        println!("{}", serde_json::to_string(&kept(&id, &preview))?)
                    }
                    None => {
                        eprintln!("error: {}", unresolvable_error(&registry, &url));
                        std::process::exit(1);
                    }
                }
            }
            FavoritesAction::Keep { preview } => {
                let raw = match preview {
                    Some(raw) => raw,
                    None => {
                        use std::io::Read;
                        let mut buf = String::new();
                        std::io::stdin().read_to_string(&mut buf)?;
                        buf
                    }
                };
                let preview = preview_from_json(&raw)?;

                let paths = MuralisPaths::new()?;
                let config = Config::load(&paths)?;
                let (registry, _) = build_registry(&config)?;
                let db = Database::open(&paths.db_path())?;
                let manager = WallpaperManager::new(paths);

                // The Preview names its Source outright, so nothing has to
                // recognise a URL: `download` already takes a whole Preview.
                let src = registry
                    .by_source_type(preview.source_type.as_str())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "no configured source of type '{}' — enable it in config.toml",
                            preview.source_type
                        )
                    })?;
                let data = src.download(&preview).await?;
                let id = manager.favorite(&db, &preview, &data)?;

                println!("{}", serde_json::to_string(&kept(&id, &preview))?);
            }
        },
        Commands::Sources { action } => match action {
            SourcesAction::List => {
                let paths = MuralisPaths::new()?;
                let config = Config::load(&paths)?;
                let (registry, _) = build_registry(&config)?;

                let sources: Vec<SourceInfo> = registry.iter().map(source_info).collect();
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

/// The **Library** as the daemon served it, or `None` when it did not — no
/// socket, an error response, or a payload that will not parse. `None` sends
/// `favorites list` to the database instead: answering with the daemon down is
/// a **CLI contract** promise the **IPC contract** deliberately does not make
/// (ADR 0002), so every way the daemon can fail to answer falls back, not just
/// a refused connection.
fn library_from_daemon(
    response: muralis_core::error::Result<IpcResponse>,
) -> Option<Vec<Wallpaper>> {
    match response {
        Ok(IpcResponse::Ok { data: Some(data) }) => serde_json::from_value::<FavoritesPage>(data)
            .ok()
            .map(|page| page.wallpapers),
        _ => None,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use muralis_core::error::Result as CoreResult;
    use muralis_core::ipc::FavoritesPage;
    use muralis_core::models::{SourceType, Wallpaper, WallpaperPreview};
    use muralis_core::sources::{RetrievalMode, SourceCategory};

    struct Stub {
        name: &'static str,
        mode: RetrievalMode,
        categories: Vec<SourceCategory>,
    }

    impl Stub {
        fn searched(name: &'static str) -> Box<dyn WallpaperSource> {
            Box::new(Stub {
                name,
                mode: RetrievalMode::Searched,
                categories: Vec::new(),
            })
        }
        fn browsed(name: &'static str, slugs: &[&str]) -> Box<dyn WallpaperSource> {
            Box::new(Stub {
                name,
                mode: RetrievalMode::Browsed,
                categories: slugs
                    .iter()
                    .map(|s| SourceCategory::new(*s, s.to_uppercase()))
                    .collect(),
            })
        }
    }

    #[async_trait]
    impl WallpaperSource for Stub {
        fn name(&self) -> &str {
            self.name
        }
        fn source_type(&self) -> &str {
            self.name
        }
        fn retrieval_mode(&self) -> RetrievalMode {
            self.mode
        }
        fn categories(&self) -> Vec<SourceCategory> {
            self.categories.clone()
        }
        async fn search(
            &self,
            _q: &str,
            _p: u32,
            _pp: u32,
            _a: AspectRatioFilter,
        ) -> CoreResult<Vec<WallpaperPreview>> {
            Ok(Vec::new())
        }
        async fn download(&self, _p: &WallpaperPreview) -> CoreResult<bytes::Bytes> {
            Ok(bytes::Bytes::new())
        }
    }

    fn names(sources: &[&dyn WallpaperSource]) -> Vec<String> {
        sources.iter().map(|s| s.name().to_string()).collect()
    }

    fn registry() -> SourceRegistry {
        let mut r = SourceRegistry::new();
        r.register(Stub::searched("wallhaven"));
        r.register(Stub::browsed("daily feed", &[]));
        r.register(Stub::browsed("ultrawide", &["space", "nature"]));
        r
    }

    #[test]
    fn an_unscoped_search_fans_out_over_searched_sources_only() {
        let registry = registry();

        let targets = sources_for_search(&registry, None).unwrap();

        assert_eq!(
            names(&targets),
            vec!["wallhaven"],
            "a feed's contents are not an answer to a query"
        );
    }

    #[test]
    fn searching_a_browsed_source_by_name_is_refused_and_names_the_browse_verb() {
        let err = sources_for_search(&registry(), Some("daily feed"))
            .map(|t| names(&t))
            .expect_err("browsed sources do not answer queries");
        let msg = err.to_string();

        assert!(msg.contains("browse"), "{msg}");
        assert!(msg.contains("daily feed"), "{msg}");
    }

    #[test]
    fn the_way_out_of_a_refused_search_runs_as_written_however_the_source_is_named() {
        let one_word = sources_for_search(&registry(), Some("ultrawide"))
            .map(|t| names(&t))
            .expect_err("browsed sources do not answer queries")
            .to_string();
        assert!(
            one_word.contains("muralis browse ultrawide"),
            "a one-word name needs no quoting, so it gets none: {one_word}"
        );

        let user_named = sources_for_search(&registry(), Some("daily feed"))
            .map(|t| names(&t))
            .expect_err("browsed sources do not answer queries")
            .to_string();
        assert!(
            user_named.contains("muralis browse 'daily feed'"),
            "a user-chosen name with a space still has to run as written: {user_named}"
        );
    }

    #[test]
    fn searching_a_named_searched_source_still_scopes_to_it() {
        let registry = registry();
        let targets = sources_for_search(&registry, Some("wallhaven")).unwrap();

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].name(), "wallhaven");
    }

    #[test]
    fn searching_an_unknown_source_is_refused() {
        assert!(sources_for_search(&registry(), Some("nope")).is_err());
    }

    #[test]
    fn browsing_a_searched_source_is_refused_and_names_the_search_verb() {
        let err = browse_target(&registry(), "wallhaven")
            .map(|s| s.name().to_string())
            .expect_err("a searched source has nothing to browse");

        let msg = err.to_string();
        assert!(msg.contains("search"), "{msg}");
        assert!(
            msg.contains("--source wallhaven"),
            "the way back must run as written too: {msg}"
        );
    }

    #[test]
    fn browsing_an_unknown_source_is_refused() {
        assert!(browse_target(&registry(), "nope").map(|_| ()).is_err());
    }

    #[test]
    fn sources_list_reports_a_retrieval_mode_for_every_source() {
        let registry = registry();
        let listed: Vec<SourceInfo> = registry.iter().map(source_info).collect();
        let json = serde_json::to_value(&listed).unwrap();

        assert_eq!(json[0]["name"], "wallhaven");
        assert_eq!(json[0]["source_type"], "wallhaven");
        assert_eq!(json[0]["retrieval_mode"], "searched");
        assert_eq!(json[0]["categories"], serde_json::json!([]));

        // A feed is browsed with an empty category list — selecting it is the
        // selection.
        assert_eq!(json[1]["retrieval_mode"], "browsed");
        assert_eq!(json[1]["categories"], serde_json::json!([]));

        assert_eq!(json[2]["retrieval_mode"], "browsed");
        assert_eq!(
            json[2]["categories"],
            serde_json::json!([
                {"slug": "space", "label": "SPACE"},
                {"slug": "nature", "label": "NATURE"},
            ])
        );
    }

    fn preview(id: &str) -> WallpaperPreview {
        WallpaperPreview {
            source_type: SourceType::new("feed"),
            source_id: id.into(),
            source_url: format!("https://example.com/{id}"),
            thumbnail_url: format!("https://example.com/{id}-thumb.jpg"),
            full_url: format!("https://example.com/{id}.jpg"),
            width: 3840,
            height: 1080,
            tags: vec!["wide".into()],
        }
    }

    #[test]
    fn browse_and_search_render_one_shape_including_is_favorited() {
        let out = search_output(vec![preview("a"), preview("b")], 1, 24, |_ty, id| id == "b");
        let json = serde_json::to_value(&out).unwrap();

        assert_eq!(json["page"], 1);
        assert_eq!(json["per_page"], 24);
        assert_eq!(json["has_more"], false);
        assert_eq!(json["results"][0]["source_id"], "a");
        assert_eq!(json["results"][0]["source_type"], "feed");
        assert_eq!(json["results"][0]["width"], 3840);
        assert_eq!(json["results"][0]["is_favorited"], false);
        assert_eq!(json["results"][1]["is_favorited"], true);
    }

    #[test]
    fn a_full_page_signals_there_may_be_more() {
        let out = search_output(vec![preview("a"), preview("b")], 2, 2, |_, _| false);

        assert!(out.has_more);
    }

    #[test]
    fn browsing_a_feed_takes_no_category() {
        let registry = registry();
        let feed = browse_target(&registry, "daily feed").unwrap();

        assert_eq!(select_category(feed, None).unwrap(), None);
    }

    #[test]
    fn a_result_as_search_and_browse_emit_it_is_keepable_as_a_preview() {
        let emitted =
            serde_json::to_value(search_output(vec![preview("a")], 1, 24, |_, _| false)).unwrap();
        let one = serde_json::to_string(&emitted["results"][0]).unwrap();

        let parsed = preview_from_json(&one).expect("a result round-trips into the Preview it was");

        assert_eq!(parsed.source_type.as_str(), "feed");
        assert_eq!(parsed.source_id, "a");
        assert_eq!(parsed.full_url, "https://example.com/a.jpg");
        assert_eq!(parsed.thumbnail_url, "https://example.com/a-thumb.jpg");
        assert_eq!((parsed.width, parsed.height), (3840, 1080));
        assert_eq!(parsed.tags, vec!["wide".to_string()]);
    }

    #[test]
    fn an_incomplete_preview_is_refused_with_the_field_it_is_missing() {
        for missing in ["source_type", "source_id", "full_url"] {
            let mut obj = serde_json::json!({
                "source_type": "feed",
                "source_id": "a",
                "full_url": "https://example.com/a.jpg",
            });
            obj.as_object_mut().unwrap().remove(missing);
            let err = preview_from_json(&obj.to_string())
                .map(|_| ())
                .expect_err("{missing} is not optional");

            assert!(err.to_string().contains(missing), "{err}");

            // Present but empty is just as unkeepable, and says the same.
            obj[missing] = serde_json::json!("");
            let err = preview_from_json(&obj.to_string())
                .map(|_| ())
                .expect_err("an empty {missing} identifies nothing");
            assert!(err.to_string().contains(missing), "{err}");
        }
    }

    #[test]
    fn malformed_input_is_refused_rather_than_panicking() {
        assert!(preview_from_json("").map(|_| ()).is_err());
        assert!(preview_from_json("{not json").map(|_| ()).is_err());
        assert!(preview_from_json("[]").map(|_| ()).is_err());
    }

    #[test]
    fn handing_the_whole_page_over_instead_of_one_result_says_so() {
        let page =
            serde_json::to_string(&search_output(vec![preview("a")], 1, 24, |_, _| false)).unwrap();

        let err = preview_from_json(&page)
            .map(|_| ())
            .expect_err("a page is not a result");

        assert!(err.to_string().contains("results"), "{err}");
    }

    #[test]
    fn a_wallpaper_kept_by_preview_answers_exactly_as_one_kept_by_url() {
        let by_url = kept("abc", &preview("a"));
        let by_preview = kept("abc", &preview("a"));

        assert_eq!(by_url, by_preview);
        assert_eq!(by_url["id"], "abc");
        assert_eq!(by_url["source_type"], "feed");
        assert_eq!(by_url["source_id"], "a");
        assert_eq!(by_url["source_url"], "https://example.com/a");
    }

    struct Explaining;

    #[async_trait]
    impl WallpaperSource for Explaining {
        fn name(&self) -> &str {
            "explaining"
        }
        fn source_type(&self) -> &str {
            "explaining"
        }
        fn explain_unresolvable(&self, url: &str) -> Option<String> {
            url.contains("explaining.test")
                .then(|| format!("{url} names a category page, not an image"))
        }
        async fn search(
            &self,
            _q: &str,
            _p: u32,
            _pp: u32,
            _a: AspectRatioFilter,
        ) -> CoreResult<Vec<WallpaperPreview>> {
            Ok(Vec::new())
        }
        async fn download(&self, _p: &WallpaperPreview) -> CoreResult<bytes::Bytes> {
            Ok(bytes::Bytes::new())
        }
    }

    #[test]
    fn a_url_naming_a_page_is_a_different_error_from_one_nobody_recognises() {
        let mut registry = registry();
        registry.register(Box::new(Explaining));

        let page = unresolvable_error(&registry, "https://explaining.test/space-wallpapers");
        let unknown = unresolvable_error(&registry, "https://nobody.test/whatever");

        assert!(page.contains("not an image"), "{page}");
        assert!(
            unknown.contains("no source could resolve URL"),
            "an unrecognised URL keeps the message it always had: {unknown}"
        );
        assert!(!unknown.contains("not an image"), "{unknown}");
    }

    fn wallpaper(id: &str) -> Wallpaper {
        Wallpaper {
            id: id.into(),
            source_type: SourceType::new("test"),
            source_id: id.into(),
            source_url: None,
            width: 5120,
            height: 1440,
            tags: Vec::new(),
            file_path: format!("/tmp/{id}.jpg"),
            added_at: "2026-09-10T00:00:00Z".into(),
            last_used: None,
            use_count: 0,
        }
    }

    #[test]
    fn favorites_list_takes_the_library_from_the_daemon_when_it_answers() {
        let page = FavoritesPage {
            wallpapers: vec![wallpaper("wp1")],
            total: 1,
            offset: 0,
        };
        let resp = IpcResponse::ok_with_data(serde_json::to_value(&page).unwrap());

        let library = library_from_daemon(Ok(resp)).expect("a served page is the answer");

        assert_eq!(library.len(), 1);
        assert_eq!(library[0].id, "wp1");
    }

    #[test]
    fn favorites_list_falls_back_when_the_daemon_is_down() {
        // The CLI contract promises an answer with no daemon; the IPC contract
        // does not. `None` is the signal to read the database directly.
        let down = Err(muralis_core::error::MuralisError::Ipc(
            "failed to connect to daemon".into(),
        ));

        assert!(library_from_daemon(down).is_none());
    }

    #[test]
    fn favorites_list_falls_back_when_the_daemon_answers_badly() {
        assert!(library_from_daemon(Ok(IpcResponse::error("engine unavailable"))).is_none());
        assert!(library_from_daemon(Ok(IpcResponse::ok())).is_none());
        assert!(
            library_from_daemon(Ok(IpcResponse::ok_with_data(serde_json::json!("nonsense"))))
                .is_none()
        );
    }
}
