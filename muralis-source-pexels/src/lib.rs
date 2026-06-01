use std::sync::Arc;

use serde::Deserialize;

use muralis_core::models::{SourceType, WallpaperPreview};
use muralis_core::sources::{SourceContext, WallpaperSource};
use muralis_source_common::{
    Auth, Descriptor, DetailResponse, ReqwestFetch, RestSource, SearchResponse,
};

const API_BASE: &str = "https://api.pexels.com/v1";
/// Upstream pages consumed per logical page — Pexels is client-filtered by
/// aspect, so over-fetch a few pages to fill a logical page.
const BLOCK: u32 = 3;

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct PexelsConfig {
    pub enabled: bool,
    pub api_key: Option<String>,
}

pub fn create_sources(
    table: &toml::Table,
    client: reqwest::Client,
    _ctx: &SourceContext,
) -> Vec<Box<dyn WallpaperSource>> {
    let Some(val) = table.get("pexels") else {
        return Vec::new();
    };
    let config: PexelsConfig = val.clone().try_into().unwrap_or_default();
    if !config.enabled {
        return Vec::new();
    }
    let Some(key) = config.api_key else {
        return Vec::new();
    };

    let desc = Descriptor {
        source_type: "pexels".into(),
        display_name: "Pexels".into(),
        base: API_BASE,
        auth: Auth::Header {
            name: "Authorization",
            value: key,
        },
        search_path: "/search",
        detail_path: "/photos",
        query_key: "query",
        tag_prefix: None,
        per_page_param: Some("per_page"),
        per_page_cap: 80,
        block: BLOCK,
        extra_query: vec![("orientation", "landscape".into())],
        server_aspect_param: None, // filtered client-side
    };

    let http = Arc::new(ReqwestFetch(client));
    vec![Box::new(
        RestSource::<PexelsSearchResponse, PexelsPhoto>::new(desc, http),
    )]
}

// -- API response types --

#[derive(Debug, Deserialize)]
pub struct PexelsSearchResponse {
    photos: Vec<PexelsPhoto>,
}

#[derive(Debug, Deserialize)]
pub struct PexelsPhoto {
    id: u64,
    width: u32,
    height: u32,
    url: String,
    src: PexelsSrc,
}

#[derive(Debug, Deserialize)]
struct PexelsSrc {
    original: String,
    medium: String,
}

impl SearchResponse for PexelsSearchResponse {
    fn into_previews(self, source_type: &str) -> Vec<WallpaperPreview> {
        self.photos
            .into_iter()
            .map(|p| p.into_preview(source_type))
            .collect()
    }
}

impl DetailResponse for PexelsPhoto {
    fn into_preview(self, source_type: &str) -> WallpaperPreview {
        WallpaperPreview {
            source_type: SourceType::new(source_type),
            source_id: self.id.to_string(),
            source_url: self.url,
            thumbnail_url: self.src.medium,
            full_url: self.src.original,
            width: self.width,
            height: self.height,
            tags: Vec::new(), // Pexels has no tags
        }
    }

    fn parse_id(url: &str) -> Option<String> {
        if !url.contains("pexels.com/photo/") {
            return None;
        }
        // pexels.com/photo/some-slug-12345/  ->  12345
        url.trim_end_matches('/')
            .rsplit('-')
            .next()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|id| id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use muralis_core::sources::AspectRatioFilter;
    use muralis_source_common::testing::StubFetch;

    const MOCK_RESPONSE: &str = r##"{
        "page": 1,
        "per_page": 80,
        "photos": [
            {
                "id": 12345,
                "width": 4000,
                "height": 2500,
                "url": "https://www.pexels.com/photo/12345/",
                "src": {
                    "original": "https://images.pexels.com/photos/12345/pexels-photo-12345.jpeg",
                    "medium": "https://images.pexels.com/photos/12345/pexels-photo-12345.jpeg?w=350"
                }
            }
        ]
    }"##;

    fn source(http: Arc<StubFetch>) -> RestSource<PexelsSearchResponse, PexelsPhoto> {
        let desc = Descriptor {
            source_type: "pexels".into(),
            display_name: "Pexels".into(),
            base: API_BASE,
            auth: Auth::Header {
                name: "Authorization",
                value: "RAWKEY".into(),
            },
            search_path: "/search",
            detail_path: "/photos",
            query_key: "query",
            tag_prefix: None,
            per_page_param: Some("per_page"),
            per_page_cap: 80,
            block: BLOCK,
            extra_query: vec![("orientation", "landscape".into())],
            server_aspect_param: None,
        };
        RestSource::new(desc, http)
    }

    #[tokio::test]
    async fn maps_response_to_preview_with_empty_tags() {
        let http = Arc::new(StubFetch::ok_pages(&[
            MOCK_RESPONSE,
            r#"{"photos":[]}"#,
            r#"{"photos":[]}"#,
        ]));
        let previews = source(http)
            .search("x", 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();

        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].source_id, "12345");
        assert_eq!(previews[0].width, 4000);
        assert!(previews[0].tags.is_empty());
    }

    #[tokio::test]
    async fn search_uses_raw_header_auth_and_per_page_cap() {
        let http = Arc::new(StubFetch::ok(MOCK_RESPONSE));
        source(http.clone())
            .search("waves", 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();

        let call = http.last_call();
        assert_eq!(call.url, "https://api.pexels.com/v1/search");
        assert_eq!(call.header_value("Authorization"), Some("RAWKEY")); // no prefix
        assert_eq!(call.query_value("query"), Some("waves"));
        assert_eq!(call.query_value("per_page"), Some("80"));
    }

    #[test]
    fn parses_numeric_id_from_url() {
        assert_eq!(
            PexelsPhoto::parse_id("https://www.pexels.com/photo/some-slug-12345/"),
            Some("12345".into())
        );
        assert_eq!(
            PexelsPhoto::parse_id("https://unsplash.com/photos/abc"),
            None
        );
    }
}
