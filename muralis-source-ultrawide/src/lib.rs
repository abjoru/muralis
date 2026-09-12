//! ultrawidewallpapers.net as a **Browsed Source**.
//!
//! The site publishes no API, no feed and no text search. What it does publish
//! is a **Gallery endpoint** — the one its own page calls as you scroll —
//! taking a zero-based offset, a limit and a comma-separated tag list, and
//! answering with an HTML fragment of bare cards. So this is not a
//! **RestSource**: there is no paged JSON to descriptor-drive. Its nearest
//! sibling is the **Feed Source** — non-REST, parsing markup, browsed rather
//! than searched.
//!
//! Its **Categories** are the site's tags. The curated **Category pages** it
//! used to browse are retired: they exposed 8 hand-picked slugs out of 78,
//! most of them resolution-flavoured and filtering nothing; they had no
//! pagination, so a logical page was a slice of one refetched 192-card page;
//! and they lazy-loaded three cards in four. The endpoint answers all three.
//!
//! Access posture is part of the contract, not an optimisation (see the epic):
//! a request is made only in response to a user action, never speculatively
//! and never on a timer, and it asks for exactly the window being displayed —
//! `limit` is uncapped upstream and that stays out of bounds. Outbound
//! requests identify muralis; every **Preview** links back to the site; and
//! nothing is enumerated beyond what one window returns.

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

pub mod drift;

pub use drift::{
    check_live_gallery, check_live_gallery_with, DriftCheck, DriftFailure, DriftLimits,
};

/// Stable identity for this host, so its filename-shaped `source_id`s cannot
/// collide with another Source's ids.
const SOURCE_TYPE: &str = "ultrawide";
/// What a user types to select this Source — `muralis browse Ultrawide` — and
/// what the GUI renders as its chip. One word, like every other built-in, so it
/// needs no shell quoting; the messages below hand it over unquoted for that
/// reason. Cosmetic, and deliberately separate from `SOURCE_TYPE` above.
const DISPLAY_NAME: &str = "Ultrawide";

/// The site, canonical host included — every master hangs off it, and
/// `resolve_url` matches against it before parsing an id. The apex, not `www.`:
/// the site redirects `www.` here, and a request that has to be redirected is
/// two requests.
const BASE_URL: &str = "https://ultrawidewallpapers.net/";

/// The **Gallery endpoint**: the site's own infinite scroll calls it, and it is
/// the whole of this Source's retrieval. Takes `offset`, `limit` and an
/// optional comma-separated `tag`, and answers with a fragment of bare cards.
const GALLERY_ENDPOINT: &str = "https://ultrawidewallpapers.net/gallery_load.php";

/// The human-facing page behind the endpoint. Two things read it: a
/// **Preview**'s `source_url` link-back, and the **Drift check**'s question
/// about whether the shipped tag vocabulary still matches what it publishes.
pub(crate) const GALLERY_PAGE: &str = "https://ultrawidewallpapers.net/gallery";

/// The query parameter the gallery page reads its tag selection from, so a
/// link-back opens on the tag the **Preview** was browsed under.
const GALLERY_TAGS_PARAM: &str = "tags";

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

/// The thumbnail width the gallery asks its own resize endpoint for.
const THUMB_WIDTH: u32 = 400;

/// One card in the endpoint's fragment: an anchor to the full-resolution
/// master carrying the filename, wrapping the thumbnail `<img>`.
static CARD_SEL: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("a[data-filename][href]").expect("valid selector"));
static THUMB_SEL: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("img").expect("valid selector"));

/// Where a card's thumbnail URL lives. One attribute, not two: the endpoint
/// writes the real URL into `src` and leaves the deferral to the browser's own
/// `loading="lazy"`, so the 1x1 placeholder the **Category pages** needed
/// `data-src` for is not part of this contract. The value is still checked
/// rather than trusted — a `data:` URI is a value, not a location, and a
/// placeholder loads successfully and paints nothing, so a **Preview**
/// carrying one fails *silently*.
const THUMB_ATTR: &str = "src";

/// The tag vocabulary the site publishes on its gallery page, shipped whole:
/// 28 names, in the site's own casing and order. Unlike the ~88 category slugs
/// this replaced, every one of these narrows something real, so there is no
/// taste call left to make — config names a subset when a shorter menu is
/// wanted. The site's navigation is never scraped at runtime to discover them;
/// the **Drift check** asks weekly whether this list still matches.
pub const DEFAULT_TAGS: &[&str] = &[
    "Abandoned",
    "Abstract",
    "Animals",
    "Architecture",
    "Colorful",
    "Crops great @ 16:9",
    "Cute",
    "Cyberpunk",
    "Dark",
    "Fantasy",
    "Illustration",
    "Landscape",
    "Monsters",
    "OLED",
    "Painted style",
    "Pattern",
    "People",
    "Photo",
    "Pixel Art",
    "Plants",
    "Realistic",
    "Retro",
    "Sci-fi",
    "Space",
    "Spaceships",
    "Steampunk",
    "Surreal",
    "Vehicles",
];

/// The `[sources.ultrawide]` subsection: off unless switched on, and an
/// optional tag list that replaces [`DEFAULT_TAGS`] when present.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct UltrawideConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// The retired **Category page** slug list. Kept only to be recognised and
    /// refused loudly: a config carrying it would otherwise be read as naming
    /// no tags at all, and silently yield the whole vocabulary.
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
    if config.categories.is_some() {
        tracing::warn!(
            "ultrawide: `categories` is retired — the Source now browses the site's tags. \
             Rename the key to `tags` and name tags from: {}",
            DEFAULT_TAGS.join(", ")
        );
    }
    let tags: Vec<String> = config
        .tags
        .unwrap_or_else(|| DEFAULT_TAGS.iter().map(|s| (*s).to_string()).collect());
    for tag in &tags {
        if !DEFAULT_TAGS.contains(&tag.as_str()) {
            tracing::warn!(
                "ultrawide: '{tag}' is not a tag the site publishes; browsing it will \
                 return nothing. Published tags: {}",
                DEFAULT_TAGS.join(", ")
            );
        }
    }

    vec![Box::new(UltrawideSource::new(
        Arc::new(ReqwestFetch(client)),
        &tags,
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
    /// A tag is its own slug *and* its own label: the site publishes names a
    /// person reads, so there is nothing to derive and nothing to prettify.
    pub fn new(http: Arc<dyn HttpFetch>, tags: &[String], ctx: &SourceContext) -> Self {
        Self {
            http,
            categories: tags
                .iter()
                .map(|t| SourceCategory::new(t.clone(), t.clone()))
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
                "{DISPLAY_NAME} is a browsed source with no query dimension; \
                 use: muralis browse {DISPLAY_NAME} --category <tag>"
            ),
        ))
    }

    /// One logical page, one request: the window is asked for by `offset` and
    /// `limit`, so nothing larger is fetched and nothing is sliced afterwards.
    async fn browse(
        &self,
        category: Option<&str>,
        page: u32,
        per_page: u32,
        aspect: AspectRatioFilter,
    ) -> Result<Vec<WallpaperPreview>> {
        let tag = category.ok_or_else(|| {
            source_error(
                "browse",
                format!("{DISPLAY_NAME} publishes categories; name one with --category"),
            )
        })?;
        let offset = page.saturating_sub(1).saturating_mul(per_page);
        let url = gallery_request(tag, offset, per_page);
        let (status, body) = self
            .http
            .get(&url, &[("User-Agent", user_agent())], &[])
            .await?;
        if !status.is_success() {
            return Err(source_error(
                "browse",
                format!("{DISPLAY_NAME}: tag '{tag}' returned HTTP {status} for {url}"),
            ));
        }
        let html = String::from_utf8_lossy(&body);
        let mut previews = parse_gallery(&html, tag, &url)?;

        // The masters are uniform, so this keeps everything or nothing — which
        // is the contract: a filter excluding 32:9 correctly gets no answer
        // from this Source rather than a cropped one.
        previews.retain(|p| {
            aspect.matches(p.width, p.height)
                && p.width >= self.min_width
                && p.height >= self.min_height
        });
        Ok(previews)
    }

    /// Host-matched **before** an id is parsed out of the URL, so this Source
    /// cannot claim a URL belonging to another one — the filename tail of a
    /// master path is far too generic to identify on its own.
    async fn resolve_url(&self, url: &str) -> Result<Option<WallpaperPreview>> {
        Ok(preview_from_master_url(url))
    }

    /// A URL on this host that is not a master names a **page** — the gallery
    /// is one URL shared by everything it lists, and the site publishes no
    /// per-wallpaper detail page at all. Saying so is the whole point: pasting
    /// a listing is a different mistake from pasting a URL nothing here has
    /// ever heard of.
    fn explain_unresolvable(&self, url: &str) -> Option<String> {
        if preview_from_master_url(url).is_some() {
            return None;
        }
        let parsed = Url::parse(url).ok()?;
        if !is_site_host(parsed.host_str()?) {
            return None;
        }
        // The tag the pasted URL was listing, when it says: the way out is
        // then the same selection, browsed.
        let tag = parsed
            .query_pairs()
            .find(|(k, _)| k == GALLERY_TAGS_PARAM)
            .map(|(_, v)| v.into_owned())
            .or_else(|| self.categories.first().map(|c| c.slug.clone()))
            .unwrap_or_else(|| "Dark".to_string());
        Some(format!(
            "{DISPLAY_NAME}: {url} names a page on the site, not an image — the gallery \
             is one URL shared by everything listed on it. Keep a browsed result with: \
             muralis browse {DISPLAY_NAME} --category {tag} | jq -c '.results[0]' | \
             muralis favorites keep",
            tag = shell_arg(&tag)
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

/// The **Gallery endpoint** request for one window of one tag, built whole so
/// the URL that is fetched and the URL a failure names are the same string.
/// Tags carry spaces and punctuation (`Pixel Art`, `Crops great @ 16:9`), so
/// the encoding happens here rather than in a format string.
pub(crate) fn gallery_request(tag: &str, offset: u32, limit: u32) -> String {
    let mut url = Url::parse(GALLERY_ENDPOINT).expect("a valid endpoint URL");
    url.query_pairs_mut()
        .append_pair("offset", &offset.to_string())
        .append_pair("limit", &limit.to_string())
        .append_pair("tag", tag);
    url.into()
}

/// The gallery page as a human would open it, filtered to `tag` — what a
/// **Preview** links back to.
fn gallery_page_for(tag: &str) -> String {
    let mut url = Url::parse(GALLERY_PAGE).expect("a valid gallery URL");
    url.query_pairs_mut().append_pair(GALLERY_TAGS_PARAM, tag);
    url.into()
}

/// A value as it must be typed into a shell. One-word tags (and the display
/// name) come back verbatim; `Pixel Art` and `Crops great @ 16:9` come back
/// quoted, because a suggestion that does not run as written is not a
/// suggestion.
fn shell_arg(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// Turn one window of the **Gallery endpoint** into **Previews**, in the order
/// it listed them, one per distinct master.
///
/// An *empty* fragment is the end of the results — the endpoint's own signal
/// for an offset past the end, and for a tag it does not know — and becomes
/// the empty result muralis already treats as the end. A fragment that
/// arrived with content in it and yielded no card is the opposite: an upstream
/// markup change, reported loudly so it stays diagnosable.
fn parse_gallery(fragment: &str, tag: &str, request: &str) -> Result<Vec<WallpaperPreview>> {
    if fragment.trim().is_empty() {
        return Ok(Vec::new());
    }
    let doc = Html::parse_fragment(fragment);
    let base = Url::parse(BASE_URL).map_err(|e| source_error("browse", e.to_string()))?;
    let link_back = gallery_page_for(tag);
    let mut seen: Vec<String> = Vec::new();
    let mut previews = Vec::new();

    for card in doc.select(&CARD_SEL) {
        let (Some(filename), Some(href)) = (
            card.value().attr("data-filename"),
            card.value().attr("href"),
        ) else {
            continue;
        };
        // Checked for every card the selector matches, repeat or not: an
        // unusable thumbnail is a markup change, and the window it appears in
        // is reported rather than quietly shedding the cards it broke.
        let Some(thumb) = thumbnail_of(card) else {
            return Err(source_error(
                "browse",
                format!(
                    "{DISPLAY_NAME}: tag '{tag}' has a card for '{filename}' with no \
                     fetchable thumbnail URL in `{THUMB_ATTR}` at {request} \
                     — the site's card markup has most likely changed"
                ),
            ));
        };
        if seen.iter().any(|s| s == filename) {
            continue;
        }
        seen.push(filename.to_string());

        previews.push(WallpaperPreview {
            source_type: SourceType::new(SOURCE_TYPE),
            source_id: filename.to_string(),
            source_url: link_back.clone(),
            thumbnail_url: absolute(&base, thumb),
            full_url: absolute(&base, href),
            width: MASTER_WIDTH,
            height: MASTER_HEIGHT,
            tags: vec![tag.to_string(), "ultrawidewallpapers.net".to_string()],
        });
    }

    if previews.is_empty() {
        return Err(source_error(
            "browse",
            format!(
                "{DISPLAY_NAME}: tag '{tag}' returned a fragment with no parseable cards \
                 for {request} — the site's card markup has most likely changed"
            ),
        ));
    }
    Ok(previews)
}

/// A card's thumbnail URL, when the attribute holds a fetchable one — `None`
/// when it does not, which is drift, not a Preview to emit.
fn thumbnail_of(card: scraper::ElementRef<'_>) -> Option<&str> {
    let img = card.select(&THUMB_SEL).next()?;
    img.value()
        .attr(THUMB_ATTR)
        .map(str::trim)
        .filter(|v| is_fetchable(v))
}

/// A `data:` URI is a value, not a location: a placeholder is a valid image
/// that loads successfully and paints nothing, so a Preview carrying one fails
/// silently — the grid renders blank and nothing errors.
fn is_fetchable(value: &str) -> bool {
    !value.is_empty() && !value.to_ascii_lowercase().starts_with("data:")
}

fn absolute(base: &Url, href: &str) -> String {
    base.join(href)
        .map(String::from)
        .unwrap_or_else(|_| href.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    use muralis_source_common::testing::StubFetch;

    /// Two windows of the gallery endpoint: offsets 0 and 24, limit 24, tag
    /// `Dark`. Captured from the endpoint, cards verbatim.
    const PAGE_1: &str = include_str!("../tests/fixtures/gallery-dark-page-1.html");
    const PAGE_2: &str = include_str!("../tests/fixtures/gallery-dark-page-2.html");
    /// The same window with the filename attribute renamed: markup drift.
    const DRIFTED: &str = include_str!("../tests/fixtures/gallery-drifted.html");

    fn source_from(stub: Arc<StubFetch>) -> UltrawideSource {
        UltrawideSource::new(stub, &tags(&["Dark"]), &SourceContext::default())
    }

    fn tags(names: &[&str]) -> Vec<String> {
        names.iter().map(|t| (*t).to_string()).collect()
    }

    fn source(fragment: &str) -> UltrawideSource {
        source_from(Arc::new(StubFetch::ok(fragment)))
    }

    /// The endpoint takes a zero-based offset and a limit, so a logical page
    /// asks for exactly the window it will return — one request, no slice of
    /// a larger set.
    #[tokio::test]
    async fn a_logical_page_asks_the_endpoint_for_exactly_the_window_it_returns() {
        let stub = Arc::new(StubFetch::ok(PAGE_1));
        let src = source_from(stub.clone());

        let previews = src
            .browse(Some("Dark"), 2, 24, AspectRatioFilter::All)
            .await
            .expect("the captured fragment parses");

        let calls = stub.calls();
        assert_eq!(calls.len(), 1, "one request per logical page");
        assert_eq!(
            calls[0].url,
            "https://ultrawidewallpapers.net/gallery_load.php?offset=24&limit=24&tag=Dark",
            "page 2 of 24 is offset 24, limit 24 — the window, and nothing larger"
        );
        assert_eq!(previews.len(), 24);
        assert_eq!((previews[0].width, previews[0].height), (7680, 2160));
    }

    /// Server-side paging: two logical pages are two windows, each fetched
    /// once, and what they return does not overlap. Under the retired
    /// **Category page** model both pages were slices of one refetched page.
    #[tokio::test]
    async fn two_logical_pages_are_two_windows_fetched_once_each_and_do_not_overlap() {
        let stub = Arc::new(StubFetch::ok_pages(&[PAGE_1, PAGE_2]));
        let src = source_from(stub.clone());

        let first = src
            .browse(Some("Dark"), 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();
        let second = src
            .browse(Some("Dark"), 2, 24, AspectRatioFilter::All)
            .await
            .unwrap();

        let windows: Vec<String> = stub.calls().iter().map(|c| c.url.clone()).collect();
        assert_eq!(windows.len(), 2, "one request per logical page, no refetch");
        assert_ne!(windows[0], windows[1], "and never the same window twice");
        assert!(windows[0].contains("offset=0"), "{windows:?}");
        assert!(windows[1].contains("offset=24"), "{windows:?}");

        let ids = |ps: &[WallpaperPreview]| -> Vec<String> {
            ps.iter().map(|p| p.source_id.clone()).collect()
        };
        let (a, b) = (ids(&first), ids(&second));
        assert!(!a.is_empty() && !b.is_empty());
        assert!(
            a.iter().all(|id| !b.contains(id)),
            "page 1 and page 2 of a tag are disjoint"
        );
    }

    /// Within one window every card is a distinct master, so a filename never
    /// appears twice in a page of results.
    #[tokio::test]
    async fn a_page_of_results_names_each_master_once() {
        let previews = source(PAGE_1)
            .browse(Some("Dark"), 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();

        let mut ids: Vec<&str> = previews.iter().map(|p| p.source_id.as_str()).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "a filename appeared twice in one page");
    }

    /// The endpoint answers an offset past the end — and a tag it does not
    /// know — with an empty fragment. That is the end of the results, which
    /// muralis already spells as an empty page, not an error to invent.
    #[tokio::test]
    async fn an_empty_fragment_is_the_end_of_the_results_rather_than_an_error() {
        let past_the_end = source("")
            .browse(Some("Dark"), 500, 24, AspectRatioFilter::All)
            .await
            .expect("an empty window is an answer");

        assert!(past_the_end.is_empty());
    }

    /// Tags are the site's own names, and some carry spaces and punctuation.
    /// Encoding is the request builder's job, not the caller's.
    #[tokio::test]
    async fn a_tag_carrying_spaces_and_punctuation_is_encoded_into_the_request() {
        let stub = Arc::new(StubFetch::ok(PAGE_1));
        let src = UltrawideSource::new(
            stub.clone(),
            &tags(&["Crops great @ 16:9"]),
            &SourceContext::default(),
        );

        let previews = src
            .browse(Some("Crops great @ 16:9"), 1, 24, AspectRatioFilter::All)
            .await
            .expect("a punctuated tag is browsable");

        assert!(!previews.is_empty());
        assert_eq!(
            stub.last_call().url,
            "https://ultrawidewallpapers.net/gallery_load.php\
             ?offset=0&limit=24&tag=Crops+great+%40+16%3A9"
        );
        assert_eq!(
            previews[0].tags[0], "Crops great @ 16:9",
            "the Preview carries the tag in its published casing"
        );
    }

    /// Every master is one 7680x2160 image, never cropped, so a filter that
    /// excludes 32:9 correctly gets nothing from this Source.
    #[tokio::test]
    async fn the_aspect_filter_is_honoured_against_the_masters_true_dimensions() {
        let at = |aspect| async move {
            source(PAGE_1)
                .browse(Some("Dark"), 1, 24, aspect)
                .await
                .expect("a filter that matches nothing is still a parsed window")
                .len()
        };

        assert_eq!(at(AspectRatioFilter::Ratio32x9).await, 24);
        assert_eq!(at(AspectRatioFilter::Ratio21x9).await, 0);
    }

    #[tokio::test]
    async fn the_global_minimum_dimensions_are_applied_rather_than_assumed_satisfied() {
        let demanding = UltrawideSource::new(
            Arc::new(StubFetch::ok(PAGE_1)),
            &tags(&["Dark"]),
            &SourceContext {
                min_width: MASTER_WIDTH + 1,
                ..SourceContext::default()
            },
        );

        let previews = demanding
            .browse(Some("Dark"), 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();

        assert!(previews.is_empty(), "7680 wide is below a 7681 floor");
    }

    #[tokio::test]
    async fn every_outbound_request_identifies_muralis_and_asks_for_nothing_else() {
        let stub = Arc::new(StubFetch::ok(PAGE_1));
        let src = source_from(stub.clone());

        src.browse(Some("Dark"), 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();

        let call = stub.last_call();
        let ua = call
            .header_value("User-Agent")
            .expect("outbound requests identify muralis");
        assert!(ua.starts_with("muralis/"), "{ua}");
        // The Source never fetches a thumbnail: it reads the card's thumbnail
        // URL out of the fragment and hands it on, so a rendered thumbnail is
        // cached locally rather than re-fetched per render.
        assert!(
            stub.calls()
                .iter()
                .all(|c| !c.url.contains("resizecachethumbs")),
            "thumbnails are not fetched by the Source"
        );
    }

    /// A fragment that arrived with content in it and yielded no card is the
    /// opposite of an empty one: an upstream markup change, which must be
    /// diagnosable rather than looking like the end of the results.
    #[tokio::test]
    async fn a_fragment_whose_card_markup_changed_fails_loudly_naming_source_tag_and_request() {
        let err = source(DRIFTED)
            .browse(Some("Dark"), 1, 24, AspectRatioFilter::All)
            .await
            .expect_err("an unparseable fragment is not an empty window");
        let msg = err.to_string();

        assert!(msg.contains(DISPLAY_NAME), "{msg}");
        assert!(msg.contains("Dark"), "{msg}");
        assert!(
            msg.contains("gallery_load.php?offset=0&limit=24&tag=Dark"),
            "the failure names the request it made: {msg}"
        );
    }

    /// A card whose thumbnail attribute holds a placeholder rather than a URL
    /// has nothing usable left: a `data:` URI loads successfully and paints
    /// nothing, so emitting it would fail silently in the grid.
    #[tokio::test]
    async fn a_card_with_no_fetchable_thumbnail_fails_loudly_and_names_the_card() {
        let placeholder = PAGE_1.replacen(
            r#"src="resizecachethumbs.php?image=aishot-5787.jpg&width=400""#,
            r#"src="data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==""#,
            1,
        );
        assert_ne!(placeholder, PAGE_1, "a thumbnail was replaced");

        let err = source(&placeholder)
            .browse(Some("Dark"), 1, 24, AspectRatioFilter::All)
            .await
            .expect_err("a placeholder is not a thumbnail URL");
        let msg = err.to_string();

        assert!(msg.contains(DISPLAY_NAME), "{msg}");
        assert!(msg.contains("aishot-5787.jpg"), "the card is named: {msg}");
    }

    /// A browsed result carries the master's true dimensions, the site's own
    /// thumbnail URL, and a link back to the gallery filtered to its tag.
    #[tokio::test]
    async fn a_browsed_card_becomes_a_preview_keyed_on_its_filename_and_linked_back() {
        let previews = source(PAGE_1)
            .browse(Some("Dark"), 1, 24, AspectRatioFilter::All)
            .await
            .expect("the captured fragment parses");

        let first = &previews[0];
        assert_eq!(first.source_type.as_str(), "ultrawide");
        assert_eq!(first.source_id, "aishot-5792.jpg");
        assert_eq!(
            first.full_url,
            "https://ultrawidewallpapers.net/wallpapers/329/highres/aishot-5792.jpg"
        );
        assert_eq!(
            first.thumbnail_url,
            "https://ultrawidewallpapers.net/resizecachethumbs.php?image=aishot-5792.jpg&width=400"
        );
        assert_eq!(
            first.source_url, "https://ultrawidewallpapers.net/gallery?tags=Dark",
            "a kept image links back to the gallery on the tag it came from"
        );
        assert_eq!((first.width, first.height), (MASTER_WIDTH, MASTER_HEIGHT));
        for preview in &previews {
            assert!(
                !preview.thumbnail_url.starts_with("data:") && !preview.thumbnail_url.is_empty(),
                "{id}: {url}",
                id = preview.source_id,
                url = preview.thumbnail_url
            );
        }
    }

    #[tokio::test]
    async fn a_master_url_resolves_to_a_preview_keyed_on_its_filename() {
        let preview = source(PAGE_1)
            .resolve_url(
                "https://www.ultrawidewallpapers.net/wallpapers/329/highres/aishot-5774.jpg",
            )
            .await
            .unwrap()
            .expect("a master on this host is ours to resolve");

        assert_eq!(preview.source_type.as_str(), "ultrawide");
        assert_eq!(preview.source_id, "aishot-5774.jpg");
        assert_eq!(
            (preview.width, preview.height),
            (MASTER_WIDTH, MASTER_HEIGHT)
        );
        assert!(preview.source_url.contains("ultrawidewallpapers.net"));
    }

    #[tokio::test]
    async fn a_url_belonging_to_another_source_is_not_claimed() {
        let src = source(PAGE_1);
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
        assert!(src
            .resolve_url("https://ultrawidewallpapers.net/gallery")
            .await
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_gallery_url_is_explained_as_a_page_rather_than_left_unrecognised() {
        let src = source(PAGE_1);

        let why = src
            .explain_unresolvable("https://ultrawidewallpapers.net/gallery?tags=Space")
            .expect("this Source recognises its own gallery");

        assert!(
            why.contains("page") && why.contains("not an image"),
            "the user must learn what kind of URL was expected: {why}"
        );
        assert!(
            why.contains("favorites keep"),
            "and how to keep instead: {why}"
        );
        assert!(
            why.contains(&format!("muralis browse {DISPLAY_NAME} --category Space")),
            "the way out names the tag the pasted URL was listing: {why}"
        );
    }

    /// A suggestion that does not run as written is not a suggestion: a tag
    /// carrying spaces comes back quoted, a one-word tag bare.
    #[test]
    fn a_suggested_command_is_typed_into_a_shell_verbatim() {
        let src = source(PAGE_1);

        let punctuated = src
            .explain_unresolvable("https://ultrawidewallpapers.net/gallery?tags=Pixel+Art")
            .expect("still our gallery");

        assert!(
            punctuated.contains("--category 'Pixel Art'"),
            "a tag with a space is quoted: {punctuated}"
        );
    }

    #[test]
    fn a_url_this_source_can_resolve_or_does_not_own_gets_no_explanation() {
        let src = source(PAGE_1);

        assert_eq!(
            src.explain_unresolvable(
                "https://ultrawidewallpapers.net/wallpapers/329/highres/aishot-5774.jpg"
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
            .resolve_url("https://ultrawidewallpapers.net/wallpapers/329/highres/aishot-5774.jpg")
            .await
            .unwrap()
            .unwrap();

        let kept = src.download(&preview).await.unwrap();

        assert_eq!(kept, master, "what is kept is what the site serves");
        let call = stub.last_call();
        assert_eq!(call.url, preview.full_url);
        assert!(call
            .header_value("User-Agent")
            .is_some_and(|ua| ua.starts_with("muralis/")));
    }

    #[tokio::test]
    async fn a_master_the_site_refuses_is_an_error_not_an_empty_file() {
        let src = source_from(Arc::new(StubFetch::status(403)));
        let preview = src
            .resolve_url("https://ultrawidewallpapers.net/wallpapers/329/highres/aishot-5774.jpg")
            .await
            .unwrap()
            .unwrap();

        let err = src
            .download(&preview)
            .await
            .expect_err("403 is not an image");

        assert!(err.to_string().contains("403"), "{err}");
    }

    #[tokio::test]
    async fn a_window_the_site_refuses_is_an_error_rather_than_an_empty_page() {
        let err = source_from(Arc::new(StubFetch::status(503)))
            .browse(Some("Dark"), 1, 24, AspectRatioFilter::All)
            .await
            .expect_err("a 503 is not the end of the results");

        assert!(err.to_string().contains("503"), "{err}");
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

    /// The whole published vocabulary ships, because every tag narrows
    /// something real — there is no taste call left to make. Config names a
    /// subset when a shorter menu is wanted.
    #[test]
    fn omitting_tags_publishes_the_whole_vocabulary_and_naming_some_publishes_exactly_those() {
        let shipped = configured("[ultrawide]\nenabled = true");
        assert_eq!(shipped.len(), 1);
        let published: Vec<String> = shipped[0]
            .categories()
            .iter()
            .map(|c| c.slug.clone())
            .collect();
        assert_eq!(published, DEFAULT_TAGS);
        assert_eq!(published.len(), 28);
        assert!(published.contains(&"Crops great @ 16:9".to_string()));

        let chosen = configured(
            r#"
            [ultrawide]
            enabled = true
            tags = ["Space", "Pixel Art"]
        "#,
        );
        assert_eq!(
            chosen[0].categories(),
            vec![
                SourceCategory::new("Space", "Space"),
                SourceCategory::new("Pixel Art", "Pixel Art"),
            ],
            "a tag is its own slug and its own label"
        );
        assert_eq!(chosen[0].retrieval_mode(), RetrievalMode::Browsed);
        assert_eq!(chosen[0].source_type(), "ultrawide");
    }

    /// The retired key must not read as "named no tags", which would silently
    /// publish the whole vocabulary and look like it had been honoured.
    #[test]
    fn the_retired_category_slug_list_is_recognised_rather_than_silently_ignored() {
        let stale = configured(
            r#"
            [ultrawide]
            enabled = true
            categories = ["32-9-wallpapers", "space-wallpapers"]
        "#,
        );

        assert_eq!(stale.len(), 1, "the Source still loads");
        assert_eq!(
            stale[0]
                .categories()
                .iter()
                .map(|c| c.slug.clone())
                .collect::<Vec<_>>(),
            DEFAULT_TAGS,
            "and falls back to the vocabulary rather than to dead slugs"
        );
    }

    /// The display name is cosmetic; `source_type` is identity. A Library row
    /// written before the retarget keys on the latter, so it must survive it.
    #[tokio::test]
    async fn a_wallpaper_kept_before_the_retarget_is_still_reported_as_favorited() {
        let kept = source(PAGE_1)
            .browse(Some("Dark"), 1, 24, AspectRatioFilter::All)
            .await
            .expect("the captured fragment parses")
            .remove(0);

        let db = muralis_core::db::Database::open_in_memory().expect("in-memory Library");
        db.insert_wallpaper(&muralis_core::models::Wallpaper {
            id: "sha256-of-the-master".into(),
            source_type: muralis_core::models::SourceType::new("ultrawide"),
            source_id: kept.source_id.clone(),
            source_url: Some(kept.source_url.clone()),
            width: kept.width,
            height: kept.height,
            tags: vec![],
            file_path: "/data/wallpapers/sha256-of-the-master.jpg".into(),
            added_at: "2026-09-01T00:00:00Z".into(),
            last_used: None,
            use_count: 0,
        })
        .expect("a row kept before the retarget");

        assert!(
            db.is_favorited_by_source(kept.source_type.as_str(), &kept.source_id)
                .unwrap(),
            "retargeting the Source must not orphan what the Library already holds"
        );
    }

    #[test]
    fn the_display_name_can_be_typed_on_a_shell_without_quoting() {
        let name = source(PAGE_1).name().to_string();

        assert_eq!(shell_arg(&name), name, "'{name}' would need quoting");
        assert!(name.to_lowercase().contains("ultrawide"), "{name}");
    }

    #[tokio::test]
    async fn searching_this_source_is_refused_and_points_at_browse() {
        let err = source(PAGE_1)
            .search("sunset", 1, 24, AspectRatioFilter::All)
            .await
            .expect_err("there is no query dimension to search on");
        let msg = err.to_string();

        assert!(
            msg.contains(&format!("muralis browse {DISPLAY_NAME} --category")),
            "the way out must be copy-pasteable as written, unquoted: {msg}"
        );
    }
}
