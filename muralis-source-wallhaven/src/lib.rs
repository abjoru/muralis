use std::sync::Arc;

use serde::Deserialize;

use muralis_core::models::{SourceType, WallpaperPreview};
use muralis_core::sources::{ContentSafety, SourceContext, WallpaperSource};
use muralis_source_common::{
    Auth, Descriptor, DetailResponse, ReqwestFetch, RestSource, SearchResponse,
};

const API_BASE: &str = "https://wallhaven.cc/api/v1";

/// Map the global content-safety ceiling onto wallhaven's 3-bit `purity`
/// string (SFW / Sketchy / NSFW). The ceiling is a *cap*: we bitwise-AND the
/// configured purity with the ceiling's allowed bits, so per-source config can
/// only tighten below the ceiling, never loosen past it (ADR 0002). A stale
/// `purity="111"` under global `safe` clamps to `100` (SFW only) — no NSFW leak.
fn effective_purity(configured: &str, ceiling: ContentSafety) -> String {
    let cap = match ceiling {
        ContentSafety::Safe => 0b100,
        ContentSafety::Moderate => 0b110,
        ContentSafety::Nsfw => 0b111,
    };
    let cfg = parse_purity_bits(configured);
    let bits = cfg & cap;
    // Never emit an all-zero purity (wallhaven would reject it); fall back to
    // SFW-only, which is always within any ceiling.
    let bits = if bits == 0 { 0b100 } else { bits };
    format!("{}{}{}", (bits >> 2) & 1, (bits >> 1) & 1, bits & 1)
}

/// Parse a wallhaven purity string ("100", "110", …) into a 3-bit mask. Any
/// non-`"1"`/`"0"` char is treated as `0`; short/long strings are tolerated by
/// reading at most the first three chars. Defaults to SFW-only on garbage.
fn parse_purity_bits(s: &str) -> u8 {
    let mut bits = 0u8;
    for (i, c) in s.chars().take(3).enumerate() {
        if c == '1' {
            bits |= 1 << (2 - i);
        }
    }
    if bits == 0 {
        0b100
    } else {
        bits
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WallhavenConfig {
    pub enabled: bool,
    pub api_key: Option<String>,
    pub categories: String,
    pub purity: String,
}

impl Default for WallhavenConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            api_key: None,
            categories: "100".into(),
            purity: "100".into(),
        }
    }
}

pub fn create_sources(
    table: &toml::Table,
    client: reqwest::Client,
    ctx: &SourceContext,
) -> Vec<Box<dyn WallpaperSource>> {
    let Some(val) = table.get("wallhaven") else {
        return Vec::new();
    };
    let config: WallhavenConfig = val.clone().try_into().unwrap_or_default();
    if !config.enabled {
        return Vec::new();
    }

    let purity = effective_purity(&config.purity, ctx.content_safety);

    let desc = Descriptor {
        source_type: "wallhaven",
        display_name: "Wallhaven",
        base: API_BASE,
        auth: Auth::QueryParam {
            key: "apikey",
            value: config.api_key,
        },
        search_path: "/search",
        detail_path: "/w",
        query_key: "q",
        per_page_param: None, // wallhaven uses a fixed server page size
        per_page_cap: 24,
        block: 1, // server filters by aspect, so ~every result matches
        extra_query: vec![("categories", config.categories), ("purity", purity)],
        server_aspect_param: Some("ratios"),
    };

    let http = Arc::new(ReqwestFetch(client));
    vec![Box::new(RestSource::<
        WallhavenResponse,
        WallhavenDetailResponse,
    >::new(desc, http))]
}

// -- API response types --

#[derive(Debug, Deserialize)]
pub struct WallhavenResponse {
    data: Vec<WallhavenWallpaper>,
}

#[derive(Debug, Deserialize)]
pub struct WallhavenDetailResponse {
    data: WallhavenWallpaper,
}

#[derive(Debug, Deserialize)]
struct WallhavenWallpaper {
    id: String,
    url: String,
    path: String,
    dimension_x: u32,
    dimension_y: u32,
    thumbs: WallhavenThumbs,
    #[serde(default)]
    tags: Vec<WallhavenTag>,
}

#[derive(Debug, Deserialize)]
struct WallhavenThumbs {
    original: String,
}

#[derive(Debug, Deserialize)]
struct WallhavenTag {
    name: String,
}

impl WallhavenWallpaper {
    fn into_preview(self, source_type: &str) -> WallpaperPreview {
        WallpaperPreview {
            source_type: SourceType::new(source_type),
            source_id: self.id,
            source_url: self.url,
            thumbnail_url: self.thumbs.original,
            full_url: self.path,
            width: self.dimension_x,
            height: self.dimension_y,
            tags: self.tags.into_iter().map(|t| t.name).collect(),
        }
    }
}

impl SearchResponse for WallhavenResponse {
    fn into_previews(self, source_type: &str) -> Vec<WallpaperPreview> {
        self.data
            .into_iter()
            .map(|w| w.into_preview(source_type))
            .collect()
    }
}

impl DetailResponse for WallhavenDetailResponse {
    fn into_preview(self, source_type: &str) -> WallpaperPreview {
        self.data.into_preview(source_type)
    }

    fn parse_id(url: &str) -> Option<String> {
        let rest = url
            .strip_prefix("https://wallhaven.cc/w/")
            .or_else(|| url.strip_prefix("https://whvn.cc/"))?;
        Some(rest.trim_end_matches('/').to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use muralis_core::sources::AspectRatioFilter;
    use muralis_source_common::testing::StubFetch;

    const MOCK_RESPONSE: &str = r##"{
        "data": [
            {
                "id": "abc123",
                "url": "https://wallhaven.cc/w/abc123",
                "dimension_x": 3840,
                "dimension_y": 2160,
                "path": "https://w.wallhaven.cc/full/ab/wallhaven-abc123.jpg",
                "thumbs": { "original": "https://th.wallhaven.cc/orig/ab/abc123.jpg" },
                "tags": [
                    {"id": 1, "name": "landscape"},
                    {"id": 2, "name": "nature"}
                ]
            }
        ],
        "meta": { "current_page": 1, "last_page": 1, "per_page": 24, "total": 1 }
    }"##;

    fn source(http: Arc<StubFetch>) -> RestSource<WallhavenResponse, WallhavenDetailResponse> {
        let desc = Descriptor {
            source_type: "wallhaven",
            display_name: "Wallhaven",
            base: API_BASE,
            auth: Auth::QueryParam {
                key: "apikey",
                value: Some("KEY".into()),
            },
            search_path: "/search",
            detail_path: "/w",
            query_key: "q",
            per_page_param: None,
            per_page_cap: 24,
            block: 1,
            extra_query: vec![("categories", "100".into()), ("purity", "100".into())],
            server_aspect_param: Some("ratios"),
        };
        RestSource::new(desc, http)
    }

    #[tokio::test]
    async fn maps_response_to_preview_with_tags() {
        let http = Arc::new(StubFetch::ok(MOCK_RESPONSE));
        let previews = source(http)
            .search("x", 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();

        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].source_id, "abc123");
        assert_eq!(previews[0].width, 3840);
        assert_eq!(previews[0].tags, vec!["landscape", "nature"]);
    }

    #[tokio::test]
    async fn search_sends_q_apikey_categories_and_ratio() {
        let http = Arc::new(StubFetch::ok(MOCK_RESPONSE));
        source(http.clone())
            .search("trees", 1, 24, AspectRatioFilter::Ratio16x9)
            .await
            .unwrap();

        let call = http.last_call();
        assert_eq!(call.query_value("q"), Some("trees"));
        assert_eq!(call.query_value("apikey"), Some("KEY"));
        assert_eq!(call.query_value("categories"), Some("100"));
        assert_eq!(call.query_value("ratios"), Some("16x9"));
        assert_eq!(call.query_value("per_page"), None); // not sent for wallhaven
    }

    #[test]
    fn effective_purity_clamps_to_ceiling() {
        // global safe forces SFW-only regardless of a stale config purity
        assert_eq!(effective_purity("111", ContentSafety::Safe), "100");
        assert_eq!(effective_purity("110", ContentSafety::Safe), "100");
        // moderate allows up to sketchy, strips NSFW bit
        assert_eq!(effective_purity("111", ContentSafety::Moderate), "110");
        // nsfw lets the configured purity through unchanged
        assert_eq!(effective_purity("111", ContentSafety::Nsfw), "111");
        // config can still tighten below the ceiling
        assert_eq!(effective_purity("100", ContentSafety::Nsfw), "100");
        // garbage / empty config falls back to SFW-only
        assert_eq!(effective_purity("", ContentSafety::Nsfw), "100");
        assert_eq!(effective_purity("000", ContentSafety::Nsfw), "100");
    }

    #[tokio::test]
    async fn search_request_uses_clamped_purity_under_safe_ceiling() {
        // a source whose purity was clamped by the global `safe` ceiling must
        // send purity=100 to wallhaven — no NSFW leak.
        let clamped = effective_purity("111", ContentSafety::Safe);
        let desc = Descriptor {
            source_type: "wallhaven",
            display_name: "Wallhaven",
            base: API_BASE,
            auth: Auth::QueryParam {
                key: "apikey",
                value: Some("KEY".into()),
            },
            search_path: "/search",
            detail_path: "/w",
            query_key: "q",
            per_page_param: None,
            per_page_cap: 24,
            block: 1,
            extra_query: vec![("categories", "100".into()), ("purity", clamped)],
            server_aspect_param: Some("ratios"),
        };
        let http = Arc::new(StubFetch::ok(MOCK_RESPONSE));
        let src: RestSource<WallhavenResponse, WallhavenDetailResponse> =
            RestSource::new(desc, http.clone());

        src.search("x", 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();

        assert_eq!(http.last_call().query_value("purity"), Some("100"));
    }

    #[test]
    fn parses_id_from_both_url_forms() {
        assert_eq!(
            WallhavenDetailResponse::parse_id("https://wallhaven.cc/w/abc123"),
            Some("abc123".into())
        );
        assert_eq!(
            WallhavenDetailResponse::parse_id("https://whvn.cc/xyz789/"),
            Some("xyz789".into())
        );
        assert_eq!(
            WallhavenDetailResponse::parse_id("https://example.com/p/1"),
            None
        );
    }
}
