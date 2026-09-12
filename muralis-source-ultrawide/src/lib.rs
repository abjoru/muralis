//! ultrawidewallpapers.net as a **Browsed Source**.
//!
//! The site publishes no API, no feed and no text search: its only
//! navigational dimension is a category slug, and there are no per-wallpaper
//! detail pages. So this is not a **RestSource** — there is no paged JSON to
//! descriptor-drive. Its nearest sibling is the **Feed Source**: non-REST,
//! parsing markup, browsed rather than searched.
//!
//! Access posture is part of the contract, not an optimisation (see the epic):
//! a category page is fetched only in response to a user action, never
//! speculatively and never on a timer; outbound requests identify muralis;
//! every **Preview** links back to the site; and nothing is enumerated beyond
//! what a category page publicly lists.

use std::sync::{Arc, LazyLock};

use async_trait::async_trait;
use scraper::{Html, Selector};
use url::Url;

use muralis_core::error::{MuralisError, Result};
use muralis_core::models::{SourceType, WallpaperPreview};
use muralis_core::sources::{
    AspectRatioFilter, RetrievalMode, SourceCategory, SourceContext, WallpaperSource,
};
use muralis_source_common::{HttpFetch, ReqwestFetch};

/// Stable identity for this host, so its filename-shaped `source_id`s cannot
/// collide with another Source's ids.
const SOURCE_TYPE: &str = "ultrawide";
const DISPLAY_NAME: &str = "Ultrawide Wallpapers";

/// The site, canonical host included — every category page and every master
/// hangs off it, and `resolve_url` matches against it before parsing an id.
const BASE_URL: &str = "https://www.ultrawidewallpapers.net/";

/// Outbound requests say who is asking. Part of the access posture: this
/// Source is a user-initiated renderer, and an operator reading their logs
/// should be able to tell that from the request itself.
fn user_agent() -> &'static str {
    muralis_core::http::user_agent()
}

/// Every master the site publishes is a single 32:9 image of exactly this
/// size; other ratios exist only inside its own server-side crop tool.
/// Previews therefore report the master's true dimensions — never the
/// thumbnail tag's `width`/`height`, which describe the thumbnail.
const MASTER_WIDTH: u32 = 7680;
const MASTER_HEIGHT: u32 = 2160;

/// The thumbnail width the site's own category pages request.
const THUMB_WIDTH: u32 = 386;

/// One card on a category page: an anchor to the full-resolution master
/// carrying the filename, wrapping the thumbnail `<img>`.
static CARD_SEL: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("a[data-filename][href]").expect("valid selector"));
static THUMB_SEL: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("img[src]").expect("valid selector"));

/// The shipped category set, used when config names none: a short, useful
/// selection rather than all ~88 slugs the site's sitemap lists, which would
/// make an unusable menu and go stale silently. Slugs come from config; the
/// site's navigation is never scraped to discover them.
const DEFAULT_CATEGORIES: &[&str] = &[
    "32-9-wallpapers",
    "super-ultrawide-wallpapers",
    "dark-ultrawide-wallpapers",
    "gaming-ultrawide-wallpapers",
    "space-wallpapers",
    "nature-wallpapers",
    "abstract-wallpapers",
    "minimalist-wallpapers",
];

/// The `[sources.ultrawide]` subsection: off unless switched on, and an
/// optional slug list that replaces [`DEFAULT_CATEGORIES`] when present.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct UltrawideConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub categories: Option<Vec<String>>,
}

pub fn create_sources(
    table: &toml::Table,
    client: reqwest::Client,
    ctx: &SourceContext,
) -> Vec<Box<dyn WallpaperSource>> {
    let Some(value) = table.get("ultrawide") else {
        return Vec::new();
    };
    let config: UltrawideConfig = match value.clone().try_into() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("ultrawide: skipping malformed config: {e}");
            return Vec::new();
        }
    };
    if !config.enabled {
        return Vec::new();
    }
    let slugs: Vec<String> = config.categories.unwrap_or_else(|| {
        DEFAULT_CATEGORIES
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    });

    vec![Box::new(UltrawideSource::new(
        Arc::new(ReqwestFetch(client)),
        &slugs,
        ctx,
    ))]
}

pub struct UltrawideSource {
    http: Arc<dyn HttpFetch>,
    categories: Vec<SourceCategory>,
    min_width: u32,
    min_height: u32,
}

impl UltrawideSource {
    pub fn new(http: Arc<dyn HttpFetch>, slugs: &[String], ctx: &SourceContext) -> Self {
        Self {
            http,
            categories: slugs
                .iter()
                .map(|s| SourceCategory::new(s, label_for(s)))
                .collect(),
            min_width: ctx.min_width,
            min_height: ctx.min_height,
        }
    }
}

#[async_trait]
impl WallpaperSource for UltrawideSource {
    fn name(&self) -> &str {
        DISPLAY_NAME
    }

    fn source_type(&self) -> &str {
        SOURCE_TYPE
    }

    fn retrieval_mode(&self) -> RetrievalMode {
        RetrievalMode::Browsed
    }

    fn categories(&self) -> Vec<SourceCategory> {
        self.categories.clone()
    }

    async fn search(
        &self,
        _query: &str,
        _page: u32,
        _per_page: u32,
        _aspect: AspectRatioFilter,
    ) -> Result<Vec<WallpaperPreview>> {
        Err(source_error(
            "search",
            format!(
                "'{DISPLAY_NAME}' is a browsed source with no query dimension; \
                 use: muralis browse '{DISPLAY_NAME}' --category <slug>"
            ),
        ))
    }

    async fn browse(
        &self,
        category: Option<&str>,
        page: u32,
        per_page: u32,
        aspect: AspectRatioFilter,
    ) -> Result<Vec<WallpaperPreview>> {
        let slug = category.ok_or_else(|| {
            source_error(
                "browse",
                format!("'{DISPLAY_NAME}' publishes categories; name one with --category"),
            )
        })?;
        let url = category_url(slug);
        let (status, body) = self
            .http
            .get(&url, &[("User-Agent", user_agent())], &[])
            .await?;
        if !status.is_success() {
            return Err(source_error(
                "browse",
                format!("{DISPLAY_NAME}: category '{slug}' returned HTTP {status} for {url}"),
            ));
        }
        let html = String::from_utf8_lossy(&body);
        let mut previews = parse_category(&html, slug, &url)?;

        // The masters are uniform, so this keeps everything or nothing — which
        // is the contract: a filter excluding 32:9 correctly gets no answer
        // from this Source rather than a cropped one. Parsing having succeeded
        // is established above, so an empty answer here means "no match", not
        // "markup changed".
        previews.retain(|p| {
            aspect.matches(p.width, p.height)
                && p.width >= self.min_width
                && p.height >= self.min_height
        });

        // A category page has no pagination and no load-more upstream: it is a
        // fixed, curated slice. Logical pages therefore walk *that* slice and
        // stop, rather than reaching for a page-2 the site does not publish.
        let skip = page.saturating_sub(1).saturating_mul(per_page) as usize;
        Ok(previews
            .into_iter()
            .skip(skip)
            .take(per_page as usize)
            .collect())
    }

    /// Host-matched **before** an id is parsed out of the URL, so this Source
    /// cannot claim a URL belonging to another one — the filename tail of a
    /// master path is far too generic to identify on its own.
    async fn resolve_url(&self, url: &str) -> Result<Option<WallpaperPreview>> {
        Ok(preview_from_master_url(url))
    }

    /// A URL on this host that is not a master names a **page** — a category
    /// page is one URL shared by every wallpaper listed on it, and the site
    /// publishes no per-wallpaper detail page at all. Saying so is the whole
    /// point: pasting a category page is a different mistake from pasting a
    /// URL nothing here has ever heard of.
    fn explain_unresolvable(&self, url: &str) -> Option<String> {
        if preview_from_master_url(url).is_some() {
            return None;
        }
        let parsed = Url::parse(url).ok()?;
        if !is_site_host(parsed.host_str()?) {
            return None;
        }
        let slug = parsed
            .path_segments()
            .and_then(|mut s| s.next())
            .filter(|s| !s.is_empty())
            .unwrap_or("<slug>")
            .to_string();
        Some(format!(
            "{DISPLAY_NAME}: {url} names a page on the site, not an image — a category page \
             is one URL shared by every wallpaper listed on it. Keep a browsed result with: \
             muralis browse '{DISPLAY_NAME}' --category {slug} | jq -c '.results[0]' | \
             muralis favorites keep"
        ))
    }

    /// The full-resolution master, byte-for-byte as the site serves it — this
    /// Source never crops, resizes or otherwise alters an image.
    async fn download(&self, preview: &WallpaperPreview) -> Result<bytes::Bytes> {
        let (status, body) = self
            .http
            .get(&preview.full_url, &[("User-Agent", user_agent())], &[])
            .await?;
        if !status.is_success() {
            return Err(source_error(
                "download",
                format!(
                    "{DISPLAY_NAME}: HTTP {status} for {url}",
                    url = preview.full_url
                ),
            ));
        }
        Ok(body)
    }
}

/// The two hosts the site answers on. Exact matches: a suffix test would hand
/// `ultrawidewallpapers.net.example.com` our credentials-free trust.
fn is_site_host(host: &str) -> bool {
    host == "ultrawidewallpapers.net" || host == "www.ultrawidewallpapers.net"
}

/// A **Preview** for a full-resolution master URL on this site, or `None` for
/// anything else. There are no per-wallpaper detail pages, so the filename is
/// the identity and the site root is the link back.
fn preview_from_master_url(url: &str) -> Option<WallpaperPreview> {
    let parsed = Url::parse(url).ok()?;
    if !is_site_host(parsed.host_str()?) {
        return None;
    }
    let segments: Vec<&str> = parsed.path_segments()?.collect();
    let [first, .., "highres", filename] = segments.as_slice() else {
        return None;
    };
    if *first != "wallpapers" || !is_image_filename(filename) {
        return None;
    }

    Some(WallpaperPreview {
        source_type: SourceType::new(SOURCE_TYPE),
        source_id: (*filename).to_string(),
        source_url: BASE_URL.to_string(),
        thumbnail_url: format!(
            "{BASE_URL}resizecachethumbs.php?image={filename}&width={THUMB_WIDTH}"
        ),
        full_url: url.to_string(),
        width: MASTER_WIDTH,
        height: MASTER_HEIGHT,
        tags: vec!["ultrawidewallpapers.net".to_string()],
    })
}

fn is_image_filename(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".jpg", ".jpeg", ".png", ".webp"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

fn source_error(op: &str, kind: impl Into<String>) -> MuralisError {
    MuralisError::Source {
        source_type: SOURCE_TYPE.into(),
        op: op.into(),
        kind: kind.into(),
    }
}

fn category_url(slug: &str) -> String {
    format!("{BASE_URL}{slug}")
}

/// Turn a category page's cards into **Previews**, in page order, one per
/// distinct master — the site repeats a card across its carousel pages.
///
/// A page that yields no card is an error naming the source and the category
/// rather than an empty result: an upstream markup change must be diagnosable
/// instead of looking like an empty category.
fn parse_category(html: &str, slug: &str, page_url: &str) -> Result<Vec<WallpaperPreview>> {
    let doc = Html::parse_document(html);
    let base = Url::parse(page_url).map_err(|e| source_error("browse", e.to_string()))?;
    let mut seen: Vec<String> = Vec::new();
    let mut previews = Vec::new();

    for card in doc.select(&CARD_SEL) {
        let (Some(filename), Some(href)) = (
            card.value().attr("data-filename"),
            card.value().attr("href"),
        ) else {
            continue;
        };
        let Some(thumb) = card
            .select(&THUMB_SEL)
            .next()
            .and_then(|img| img.value().attr("src"))
        else {
            continue;
        };
        if seen.iter().any(|s| s == filename) {
            continue;
        }
        seen.push(filename.to_string());

        previews.push(WallpaperPreview {
            source_type: SourceType::new(SOURCE_TYPE),
            source_id: filename.to_string(),
            source_url: page_url.to_string(),
            thumbnail_url: absolute(&base, thumb),
            full_url: absolute(&base, href),
            width: MASTER_WIDTH,
            height: MASTER_HEIGHT,
            tags: vec![label_for(slug), "ultrawidewallpapers.net".to_string()],
        });
    }

    if previews.is_empty() {
        return Err(source_error(
            "browse",
            format!(
                "{DISPLAY_NAME}: category '{slug}' listed no parseable cards at {page_url} \
                 — the site's card markup has most likely changed"
            ),
        ));
    }
    Ok(previews)
}

fn absolute(base: &Url, href: &str) -> String {
    base.join(href)
        .map(String::from)
        .unwrap_or_else(|_| href.to_string())
}

/// A category's display label, derived from its slug: hyphens become spaces,
/// words are capitalised, and a leading numeric pair reads as an aspect ratio
/// (`32-9-wallpapers` → `32:9 Wallpapers`).
fn label_for(slug: &str) -> String {
    let words: Vec<&str> = slug.split('-').filter(|w| !w.is_empty()).collect();
    let numeric = |w: &str| !w.is_empty() && w.chars().all(|c| c.is_ascii_digit());
    let mut out = String::new();
    let mut i = 0;
    while i < words.len() {
        if !out.is_empty() {
            out.push(' ');
        }
        if numeric(words[i]) && i + 1 < words.len() && numeric(words[i + 1]) {
            out.push_str(words[i]);
            out.push(':');
            out.push_str(words[i + 1]);
            i += 2;
            continue;
        }
        let mut chars = words[i].chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use muralis_source_common::testing::StubFetch;

    const CATEGORY_PAGE: &str = include_str!("../tests/fixtures/category-32-9.html");
    const DRIFTED_PAGE: &str = include_str!("../tests/fixtures/category-drifted.html");

    fn source_from(stub: Arc<StubFetch>) -> UltrawideSource {
        UltrawideSource::new(
            stub,
            &["32-9-wallpapers".to_string()],
            &SourceContext::default(),
        )
    }

    fn source(page: &str) -> UltrawideSource {
        source_from(Arc::new(StubFetch::ok(page)))
    }

    #[tokio::test]
    async fn browsing_a_category_yields_its_cards_at_the_masters_true_dimensions() {
        let previews = source(CATEGORY_PAGE)
            .browse(Some("32-9-wallpapers"), 1, 24, AspectRatioFilter::All)
            .await
            .expect("the captured category page parses");

        assert_eq!(previews.len(), 23, "24 cards, one of them a repeat");
        let first = &previews[0];
        assert_eq!(first.source_type.as_str(), "ultrawide");
        assert_eq!(first.source_id, "aishot-5774.jpg");
        assert_eq!(
            first.full_url,
            "https://www.ultrawidewallpapers.net/wallpapers/329/highres/aishot-5774.jpg"
        );
        assert_eq!(
            first.thumbnail_url,
            "https://www.ultrawidewallpapers.net/resizecachethumbs.php?image=aishot-5774.jpg&width=386"
        );
        assert_eq!((first.width, first.height), (MASTER_WIDTH, MASTER_HEIGHT));
    }

    #[tokio::test]
    async fn the_aspect_filter_is_honoured_against_the_masters_true_dimensions() {
        let at = |aspect| async move {
            source(CATEGORY_PAGE)
                .browse(Some("32-9-wallpapers"), 1, 24, aspect)
                .await
                .expect("a filter that matches nothing is still a parsed page")
                .len()
        };

        assert_eq!(at(AspectRatioFilter::Ratio32x9).await, 23);
        assert_eq!(
            at(AspectRatioFilter::Ratio21x9).await,
            0,
            "every master is 32:9 and is never cropped, so 21:9 has nothing here"
        );
    }

    #[tokio::test]
    async fn the_global_minimum_dimensions_are_applied_rather_than_assumed_satisfied() {
        let demanding = UltrawideSource::new(
            Arc::new(StubFetch::ok(CATEGORY_PAGE)),
            &["32-9-wallpapers".to_string()],
            &SourceContext {
                min_width: MASTER_WIDTH + 1,
                ..SourceContext::default()
            },
        );

        let previews = demanding
            .browse(Some("32-9-wallpapers"), 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();

        assert!(previews.is_empty(), "7680 wide is below a 7681 floor");
    }

    #[tokio::test]
    async fn paging_walks_the_one_page_the_category_lists_and_then_ends() {
        let src = source(CATEGORY_PAGE);
        let page = |n| async move {
            source(CATEGORY_PAGE)
                .browse(Some("32-9-wallpapers"), n, 10, AspectRatioFilter::All)
                .await
                .unwrap()
        };

        let first = page(1).await;
        let second = page(2).await;
        let third = page(3).await;
        let fourth = page(4).await;

        assert_eq!((first.len(), second.len(), third.len()), (10, 10, 3));
        assert!(
            fourth.is_empty(),
            "past the end of a fixed slice is the established end-of-results signal"
        );
        let all = src
            .browse(Some("32-9-wallpapers"), 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();
        assert_eq!(first[0].source_id, all[0].source_id);
        assert_eq!(second[0].source_id, all[10].source_id);
        assert_eq!(third[0].source_id, all[20].source_id);
    }

    #[tokio::test]
    async fn every_outbound_request_identifies_muralis_and_asks_only_for_the_category_page() {
        let stub = Arc::new(StubFetch::ok(CATEGORY_PAGE));
        let src = source_from(stub.clone());

        for _ in 0..2 {
            src.browse(Some("32-9-wallpapers"), 1, 24, AspectRatioFilter::All)
                .await
                .unwrap();
        }
        let calls = stub.calls();

        assert_eq!(calls.len(), 2, "one page fetch per user-initiated browse");
        for call in &calls {
            assert_eq!(
                call.url,
                "https://www.ultrawidewallpapers.net/32-9-wallpapers"
            );
            let ua = call
                .header_value("User-Agent")
                .expect("outbound requests identify muralis");
            assert!(ua.contains("muralis"), "{ua}");
        }
        // The Source never fetches a thumbnail: it reads the card's thumbnail
        // URL off the page and hands it on, so browsing a category twice costs
        // zero thumbnail fetches and a rendered thumbnail is cached locally.
        assert!(
            calls.iter().all(|c| !c.url.contains("resizecachethumbs")),
            "thumbnails are not fetched by the Source"
        );
    }

    #[tokio::test]
    async fn a_master_url_resolves_to_a_preview_keyed_on_its_filename() {
        let preview = source(CATEGORY_PAGE)
            .resolve_url(
                "https://www.ultrawidewallpapers.net/wallpapers/329/highres/aishot-5774.jpg",
            )
            .await
            .unwrap()
            .expect("a master on this host is ours to resolve");

        assert_eq!(preview.source_type.as_str(), "ultrawide");
        assert_eq!(preview.source_id, "aishot-5774.jpg");
        assert_eq!(
            preview.full_url,
            "https://www.ultrawidewallpapers.net/wallpapers/329/highres/aishot-5774.jpg"
        );
        assert_eq!(
            (preview.width, preview.height),
            (MASTER_WIDTH, MASTER_HEIGHT)
        );
        assert!(
            preview.source_url.contains("ultrawidewallpapers.net"),
            "a kept image links back to the site"
        );
    }

    #[tokio::test]
    async fn a_url_belonging_to_another_source_is_not_claimed() {
        let src = source(CATEGORY_PAGE);
        for foreign in [
            "https://wallhaven.cc/w/abc123",
            "https://www.pexels.com/photo/whatever-12345/",
            "https://danbooru.donmai.us/posts/7654321",
            // Host-matching comes first: an id-shaped tail is not enough.
            "https://example.com/wallpapers/329/highres/aishot-5774.jpg",
            "https://ultrawidewallpapers.net.evil.example/wallpapers/329/highres/x.jpg",
        ] {
            assert!(
                src.resolve_url(foreign).await.unwrap().is_none(),
                "claimed {foreign}"
            );
        }
        // Our own host, but not a master image.
        assert!(src
            .resolve_url("https://www.ultrawidewallpapers.net/about")
            .await
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_category_page_url_is_explained_as_a_page_rather_than_left_unrecognised() {
        let src = source(CATEGORY_PAGE);

        let why = src
            .explain_unresolvable("https://www.ultrawidewallpapers.net/space-wallpapers")
            .expect("this Source recognises its own category page");

        assert!(why.contains("space-wallpapers"), "{why}");
        assert!(
            why.contains("page") && why.contains("not an image"),
            "the user must learn what kind of URL was expected: {why}"
        );
        assert!(
            why.contains("favorites keep"),
            "and how to keep a browsed result instead: {why}"
        );
    }

    #[test]
    fn a_url_this_source_can_resolve_or_does_not_own_gets_no_explanation() {
        let src = source(CATEGORY_PAGE);

        assert_eq!(
            src.explain_unresolvable(
                "https://www.ultrawidewallpapers.net/wallpapers/329/highres/aishot-5774.jpg"
            ),
            None,
            "a master resolves; there is nothing to explain"
        );
        assert_eq!(
            src.explain_unresolvable("https://wallhaven.cc/w/abc123"),
            None
        );
        assert_eq!(src.explain_unresolvable("not a url at all"), None);
    }

    #[tokio::test]
    async fn keeping_an_image_downloads_the_master_unchanged() {
        let master = bytes::Bytes::from_static(b"\xff\xd8\xff\xe0 not really a jpeg, but verbatim");
        let stub = Arc::new(StubFetch::new(vec![(
            reqwest::StatusCode::OK,
            master.clone(),
        )]));
        let src = source_from(stub.clone());
        let preview = src
            .resolve_url(
                "https://www.ultrawidewallpapers.net/wallpapers/329/highres/aishot-5774.jpg",
            )
            .await
            .unwrap()
            .unwrap();

        let kept = src.download(&preview).await.unwrap();

        assert_eq!(kept, master, "what is kept is what the site serves");
        let call = stub.last_call();
        assert_eq!(call.url, preview.full_url);
        assert!(call
            .header_value("User-Agent")
            .is_some_and(|ua| ua.contains("muralis")));
    }

    #[tokio::test]
    async fn a_master_the_site_refuses_is_an_error_not_an_empty_file() {
        let stub = Arc::new(StubFetch::status(403));
        let src = source_from(stub);
        let preview = src
            .resolve_url(
                "https://www.ultrawidewallpapers.net/wallpapers/329/highres/aishot-5774.jpg",
            )
            .await
            .unwrap()
            .unwrap();

        let err = src
            .download(&preview)
            .await
            .expect_err("403 is not an image");

        assert!(err.to_string().contains("403"), "{err}");
    }

    fn configured(toml_str: &str) -> Vec<Box<dyn WallpaperSource>> {
        let table: toml::Table = toml_str.parse().unwrap();
        create_sources(&table, reqwest::Client::new(), &SourceContext::default())
    }

    #[test]
    fn the_source_is_absent_until_it_is_enabled() {
        assert!(configured("[sources.other]\nenabled = true").is_empty());
        assert!(configured("[ultrawide]\nenabled = false").is_empty());
        assert!(
            configured("[ultrawide]").is_empty(),
            "disabled by default: no `enabled` key is not an opt-in"
        );
    }

    #[test]
    fn omitting_categories_yields_the_shipped_defaults_and_naming_them_yields_exactly_those() {
        let shipped = configured("[ultrawide]\nenabled = true");
        assert_eq!(shipped.len(), 1);
        assert_eq!(
            shipped[0]
                .categories()
                .iter()
                .map(|c| c.slug.clone())
                .collect::<Vec<_>>(),
            DEFAULT_CATEGORIES
        );

        let chosen = configured(
            r#"
            [ultrawide]
            enabled = true
            categories = ["space-wallpapers", "32-9-wallpapers"]
        "#,
        );
        assert_eq!(
            chosen[0].categories(),
            vec![
                SourceCategory::new("space-wallpapers", "Space Wallpapers"),
                SourceCategory::new("32-9-wallpapers", "32:9 Wallpapers"),
            ]
        );
        assert_eq!(chosen[0].retrieval_mode(), RetrievalMode::Browsed);
        assert_eq!(chosen[0].source_type(), "ultrawide");
    }

    #[tokio::test]
    async fn searching_this_source_is_refused_and_points_at_browse() {
        let err = source(CATEGORY_PAGE)
            .search("sunset", 1, 24, AspectRatioFilter::All)
            .await
            .expect_err("there is no query dimension to search on");
        let msg = err.to_string();

        assert!(msg.contains("browse"), "{msg}");
        assert!(msg.contains(DISPLAY_NAME), "{msg}");
    }

    #[tokio::test]
    async fn card_markup_that_has_changed_shape_fails_loudly_naming_source_and_category() {
        let err = source(DRIFTED_PAGE)
            .browse(Some("32-9-wallpapers"), 1, 24, AspectRatioFilter::All)
            .await
            .expect_err("an unparseable page is not an empty category");
        let msg = err.to_string();

        assert!(msg.contains(DISPLAY_NAME), "{msg}");
        assert!(msg.contains("32-9-wallpapers"), "{msg}");
    }
}
