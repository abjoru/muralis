use std::sync::Arc;

use serde::Deserialize;

use muralis_core::models::{SourceType, WallpaperPreview};
use muralis_core::sources::{ContentSafety, SourceContext, WallpaperSource};
use muralis_source_common::{
    Auth, Descriptor, DetailResponse, ReqwestFetch, RestSource, SearchResponse,
};

/// Pixabay has a single endpoint; `base` + `search_path` reconstruct it.
const API_BASE: &str = "https://pixabay.com";
const API_PATH: &str = "/api/";
/// Upstream pages consumed per logical page — Pixabay aspect filtering is
/// client-side (Block B > 1), so over-fetch a few pages to fill a logical page.
const BLOCK: u32 = 3;
/// Pixabay's max `per_page` is 200; we request a healthy page to fill blocks.
const PER_PAGE_CAP: u32 = 100;

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct PixabayConfig {
    pub enabled: bool,
    pub api_key: Option<String>,
}

pub fn create_sources(
    table: &toml::Table,
    client: reqwest::Client,
    ctx: &SourceContext,
) -> Vec<Box<dyn WallpaperSource>> {
    let Some(val) = table.get("pixabay") else {
        return Vec::new();
    };
    let config: PixabayConfig = val.clone().try_into().unwrap_or_default();
    if !config.enabled {
        return Vec::new();
    }
    let Some(key) = config.api_key else {
        return Vec::new();
    };

    let desc = Descriptor {
        source_type: "pixabay",
        display_name: "Pixabay",
        base: API_BASE,
        auth: Auth::QueryParam {
            key: "key",
            value: Some(key),
        },
        search_path: API_PATH,
        detail_path: API_PATH,
        query_key: "q",
        per_page_param: Some("per_page"),
        per_page_cap: PER_PAGE_CAP,
        block: BLOCK,
        extra_query: extra_query(ctx),
        server_aspect_param: None, // Pixabay has no aspect param; filter client-side
    };

    let http = Arc::new(ReqwestFetch(client));
    vec![Box::new(
        RestSource::<PixabaySearchResponse, PixabayHit>::new(desc, http),
    )]
}

/// Constant + context-driven query params:
/// - `orientation=horizontal` — landscape server-side (like pexels/unsplash).
/// - `safesearch` — driven by the global ceiling. Pixabay hosts no explicit
///   tier (it self-caps at "moderate"), so the only meaningful distinction is
///   safe vs. not. We enable safesearch only at the strictest ceiling
///   (`Safe`); at `Moderate`/`Nsfw` we leave it off, since Pixabay cannot
///   surface anything past its own moderate cap anyway.
/// - `min_width`/`min_height` — pushed from the global `[filter]` (only Pixabay
///   can do this server-side).
fn extra_query(ctx: &SourceContext) -> Vec<(&'static str, String)> {
    let safesearch = ctx.content_safety == ContentSafety::Safe;
    vec![
        ("orientation", "horizontal".into()),
        ("safesearch", safesearch.to_string()),
        ("min_width", ctx.min_width.to_string()),
        ("min_height", ctx.min_height.to_string()),
    ]
}

// -- API response types --

#[derive(Debug, Deserialize)]
pub struct PixabaySearchResponse {
    hits: Vec<PixabayHit>,
}

#[derive(Debug, Deserialize)]
pub struct PixabayHit {
    id: u64,
    #[serde(rename = "pageURL")]
    page_url: String,
    #[serde(rename = "largeImageURL", default)]
    large_image_url: String,
    #[serde(rename = "fullHDURL", default)]
    full_hd_url: String,
    #[serde(rename = "imageURL", default)]
    image_url: String,
    #[serde(rename = "previewURL", default)]
    preview_url: String,
    #[serde(rename = "imageWidth")]
    image_width: u32,
    #[serde(rename = "imageHeight")]
    image_height: u32,
    #[serde(default)]
    tags: String,
}

impl PixabayHit {
    /// Pick the best available full-resolution URL. `imageURL` (full original)
    /// is only present on the paid/full API tier; prefer it, then `fullHDURL`,
    /// then `largeImageURL`.
    fn full_url(&self) -> String {
        for candidate in [&self.image_url, &self.full_hd_url, &self.large_image_url] {
            if !candidate.is_empty() {
                return candidate.clone();
            }
        }
        self.preview_url.clone()
    }
}

impl SearchResponse for PixabaySearchResponse {
    fn into_previews(self, source_type: &str) -> Vec<WallpaperPreview> {
        self.hits
            .into_iter()
            .map(|h| h.into_preview(source_type))
            .collect()
    }
}

impl DetailResponse for PixabayHit {
    fn into_preview(self, source_type: &str) -> WallpaperPreview {
        let full = self.full_url();
        let thumb = if self.preview_url.is_empty() {
            full.clone()
        } else {
            self.preview_url.clone()
        };
        let tags = self
            .tags
            .split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        WallpaperPreview {
            source_type: SourceType::new(source_type),
            source_id: self.id.to_string(),
            source_url: self.page_url,
            thumbnail_url: thumb,
            full_url: full,
            width: self.image_width,
            height: self.image_height,
            tags,
        }
    }

    /// Pixabay's detail lookup is a query-param id (`?id=N`), which the shared
    /// engine's path-based `/{detail_path}/{id}` shape cannot express. Resolving
    /// a Pixabay page URL into a preview is therefore unsupported in this slice;
    /// return `None` so `favorites add` simply skips Pixabay rather than hitting
    /// a malformed endpoint.
    fn parse_id(_url: &str) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use muralis_core::sources::AspectRatioFilter;
    use muralis_source_common::testing::StubFetch;

    const MOCK_RESPONSE: &str = r##"{
        "total": 1,
        "totalHits": 1,
        "hits": [
            {
                "id": 195893,
                "pageURL": "https://pixabay.com/photos/blossom-bloom-flower-195893/",
                "previewURL": "https://cdn.pixabay.com/photo/2013/10/15/195893_150.jpg",
                "largeImageURL": "https://pixabay.com/get/195893_1280.jpg",
                "fullHDURL": "https://pixabay.com/get/195893_1920.jpg",
                "imageURL": "https://pixabay.com/get/195893_orig.jpg",
                "imageWidth": 4000,
                "imageHeight": 2250,
                "tags": "blossom, bloom, flower"
            }
        ]
    }"##;

    fn ctx() -> SourceContext {
        SourceContext {
            content_safety: ContentSafety::Safe,
            min_width: 1920,
            min_height: 1080,
        }
    }

    fn source(
        http: Arc<StubFetch>,
        ctx: &SourceContext,
    ) -> RestSource<PixabaySearchResponse, PixabayHit> {
        let desc = Descriptor {
            source_type: "pixabay",
            display_name: "Pixabay",
            base: API_BASE,
            auth: Auth::QueryParam {
                key: "key",
                value: Some("RAWKEY".into()),
            },
            search_path: API_PATH,
            detail_path: API_PATH,
            query_key: "q",
            per_page_param: Some("per_page"),
            per_page_cap: PER_PAGE_CAP,
            block: BLOCK,
            extra_query: extra_query(ctx),
            server_aspect_param: None,
        };
        RestSource::new(desc, http)
    }

    #[tokio::test]
    async fn maps_envelope_to_preview() {
        let http = Arc::new(StubFetch::ok_pages(&[
            MOCK_RESPONSE,
            r#"{"hits":[]}"#,
            r#"{"hits":[]}"#,
        ]));
        let previews = source(http, &ctx())
            .search("flowers", 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();

        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].source_id, "195893");
        assert_eq!(previews[0].width, 4000);
        assert_eq!(previews[0].height, 2250);
        // prefers imageURL (full original) for the wallpaper
        assert_eq!(
            previews[0].full_url,
            "https://pixabay.com/get/195893_orig.jpg"
        );
        assert_eq!(
            previews[0].source_url,
            "https://pixabay.com/photos/blossom-bloom-flower-195893/"
        );
        assert_eq!(previews[0].tags, vec!["blossom", "bloom", "flower"]);
    }

    #[tokio::test]
    async fn falls_back_to_full_hd_then_large_when_original_absent() {
        let body = r#"{"hits":[{
            "id": 1,
            "pageURL": "https://pixabay.com/photos/x-1/",
            "previewURL": "https://cdn.pixabay.com/photo/1_150.jpg",
            "largeImageURL": "https://pixabay.com/get/1_1280.jpg",
            "fullHDURL": "https://pixabay.com/get/1_1920.jpg",
            "imageURL": "",
            "imageWidth": 1920,
            "imageHeight": 1080,
            "tags": ""
        }]}"#;
        let http = Arc::new(StubFetch::ok(body));
        let previews = source(http, &ctx())
            .search("x", 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();
        assert_eq!(previews[0].full_url, "https://pixabay.com/get/1_1920.jpg");
        assert!(previews[0].tags.is_empty());
    }

    #[tokio::test]
    async fn search_sends_key_safesearch_orientation_and_min_dims() {
        let http = Arc::new(StubFetch::ok(MOCK_RESPONSE));
        source(http.clone(), &ctx())
            .search("waves", 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();

        let call = http.last_call();
        assert_eq!(call.url, "https://pixabay.com/api/");
        assert_eq!(call.query_value("key"), Some("RAWKEY"));
        assert_eq!(call.query_value("q"), Some("waves"));
        assert_eq!(call.query_value("orientation"), Some("horizontal"));
        assert_eq!(call.query_value("safesearch"), Some("true")); // Safe ceiling
        assert_eq!(call.query_value("min_width"), Some("1920"));
        assert_eq!(call.query_value("min_height"), Some("1080"));
        assert_eq!(call.query_value("per_page"), Some("100"));
    }

    #[tokio::test]
    async fn safesearch_off_when_ceiling_not_safe() {
        let ctx = SourceContext {
            content_safety: ContentSafety::Moderate,
            min_width: 800,
            min_height: 600,
        };
        let http = Arc::new(StubFetch::ok(MOCK_RESPONSE));
        source(http.clone(), &ctx)
            .search("x", 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();

        let call = http.last_call();
        assert_eq!(call.query_value("safesearch"), Some("false"));
        assert_eq!(call.query_value("min_width"), Some("800"));
    }
}
