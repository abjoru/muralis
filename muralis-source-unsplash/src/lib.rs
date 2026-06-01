use std::sync::Arc;

use serde::Deserialize;

use muralis_core::models::{SourceType, WallpaperPreview};
use muralis_core::sources::{SourceContext, WallpaperSource};
use muralis_source_common::{
    Auth, Descriptor, DetailResponse, ReqwestFetch, RestSource, SearchResponse,
};

const API_BASE: &str = "https://api.unsplash.com";
/// Upstream pages consumed per logical page — Unsplash is client-filtered by
/// aspect, so over-fetch a few pages to fill a logical page.
const BLOCK: u32 = 3;

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct UnsplashConfig {
    pub enabled: bool,
    pub access_key: Option<String>,
}

pub fn create_sources(
    table: &toml::Table,
    client: reqwest::Client,
    _ctx: &SourceContext,
) -> Vec<Box<dyn WallpaperSource>> {
    let Some(val) = table.get("unsplash") else {
        return Vec::new();
    };
    let config: UnsplashConfig = val.clone().try_into().unwrap_or_default();
    if !config.enabled {
        return Vec::new();
    }
    let Some(key) = config.access_key else {
        return Vec::new();
    };

    let desc = Descriptor {
        source_type: "unsplash",
        display_name: "Unsplash",
        base: API_BASE,
        auth: Auth::Header {
            name: "Authorization",
            value: format!("Client-ID {key}"),
        },
        search_path: "/search/photos",
        detail_path: "/photos",
        query_key: "query",
        per_page_param: Some("per_page"),
        per_page_cap: 30,
        block: BLOCK,
        extra_query: vec![("orientation", "landscape".into())],
        server_aspect_param: None, // filtered client-side
    };

    let http = Arc::new(ReqwestFetch(client));
    vec![Box::new(
        RestSource::<UnsplashSearchResponse, UnsplashPhoto>::new(desc, http),
    )]
}

// -- API response types --

#[derive(Debug, Deserialize)]
pub struct UnsplashSearchResponse {
    results: Vec<UnsplashPhoto>,
}

#[derive(Debug, Deserialize)]
pub struct UnsplashPhoto {
    id: String,
    width: u32,
    height: u32,
    urls: UnsplashUrls,
    links: UnsplashLinks,
    #[serde(default)]
    tags: Vec<UnsplashTag>,
}

#[derive(Debug, Deserialize)]
struct UnsplashUrls {
    raw: String,
    regular: String,
}

#[derive(Debug, Deserialize)]
struct UnsplashLinks {
    html: String,
}

#[derive(Debug, Deserialize)]
struct UnsplashTag {
    title: String,
}

impl SearchResponse for UnsplashSearchResponse {
    fn into_previews(self, source_type: &str) -> Vec<WallpaperPreview> {
        self.results
            .into_iter()
            .map(|p| p.into_preview(source_type))
            .collect()
    }
}

impl DetailResponse for UnsplashPhoto {
    fn into_preview(self, source_type: &str) -> WallpaperPreview {
        WallpaperPreview {
            source_type: SourceType::new(source_type),
            source_id: self.id,
            source_url: self.links.html,
            thumbnail_url: self.urls.regular,
            full_url: self.urls.raw,
            width: self.width,
            height: self.height,
            tags: self.tags.into_iter().map(|t| t.title).collect(),
        }
    }

    fn parse_id(url: &str) -> Option<String> {
        let rest = url.strip_prefix("https://unsplash.com/photos/")?;
        Some(
            rest.split('/')
                .next()
                .unwrap_or(rest)
                .trim_end_matches('/')
                .to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use muralis_core::sources::AspectRatioFilter;
    use muralis_source_common::testing::StubFetch;

    const MOCK_RESPONSE: &str = r##"{
        "total": 1,
        "total_pages": 1,
        "results": [
            {
                "id": "uns_001",
                "width": 5000,
                "height": 3000,
                "urls": {
                    "raw": "https://images.unsplash.com/photo-001",
                    "regular": "https://images.unsplash.com/photo-001?w=1080"
                },
                "links": { "html": "https://unsplash.com/photos/uns_001" },
                "tags": [ {"title": "mountain"}, {"title": "sky"} ]
            }
        ]
    }"##;

    fn source(http: Arc<StubFetch>) -> RestSource<UnsplashSearchResponse, UnsplashPhoto> {
        let desc = Descriptor {
            source_type: "unsplash",
            display_name: "Unsplash",
            base: API_BASE,
            auth: Auth::Header {
                name: "Authorization",
                value: "Client-ID KEY".into(),
            },
            search_path: "/search/photos",
            detail_path: "/photos",
            query_key: "query",
            per_page_param: Some("per_page"),
            per_page_cap: 30,
            block: BLOCK,
            extra_query: vec![("orientation", "landscape".into())],
            server_aspect_param: None,
        };
        RestSource::new(desc, http)
    }

    #[tokio::test]
    async fn maps_response_to_preview_with_tags() {
        // block=3: only the first upstream page has a result.
        let http = Arc::new(StubFetch::ok_pages(&[
            MOCK_RESPONSE,
            r#"{"results":[]}"#,
            r#"{"results":[]}"#,
        ]));
        let previews = source(http)
            .search("x", 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();

        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].source_id, "uns_001");
        assert_eq!(
            previews[0].full_url,
            "https://images.unsplash.com/photo-001"
        );
        assert_eq!(previews[0].tags, vec!["mountain", "sky"]);
    }

    #[tokio::test]
    async fn search_uses_header_auth_query_key_and_per_page() {
        let http = Arc::new(StubFetch::ok(MOCK_RESPONSE));
        source(http.clone())
            .search("birds", 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();

        let call = http.last_call();
        assert_eq!(call.url, "https://api.unsplash.com/search/photos");
        assert_eq!(call.header_value("Authorization"), Some("Client-ID KEY"));
        assert_eq!(call.query_value("query"), Some("birds"));
        assert_eq!(call.query_value("per_page"), Some("30"));
        assert_eq!(call.query_value("orientation"), Some("landscape"));
    }

    #[test]
    fn parses_id_from_url() {
        assert_eq!(
            UnsplashPhoto::parse_id("https://unsplash.com/photos/uns_001"),
            Some("uns_001".into())
        );
        assert_eq!(
            UnsplashPhoto::parse_id("https://unsplash.com/photos/abc/extra"),
            Some("abc".into())
        );
        assert_eq!(UnsplashPhoto::parse_id("https://pexels.com/photo/1"), None);
    }
}
