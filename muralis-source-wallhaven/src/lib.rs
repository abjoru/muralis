use std::sync::Arc;

use serde::Deserialize;

use muralis_core::models::{SourceType, WallpaperPreview};
use muralis_core::sources::WallpaperSource;
use muralis_source_common::{
    Auth, Descriptor, DetailResponse, ReqwestFetch, RestSource, SearchResponse,
};

const API_BASE: &str = "https://wallhaven.cc/api/v1";

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
) -> Vec<Box<dyn WallpaperSource>> {
    let Some(val) = table.get("wallhaven") else {
        return Vec::new();
    };
    let config: WallhavenConfig = val.clone().try_into().unwrap_or_default();
    if !config.enabled {
        return Vec::new();
    }

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
        extra_query: vec![("categories", config.categories), ("purity", config.purity)],
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
