use async_trait::async_trait;
use serde::Deserialize;

use muralis_core::error::Result;
use muralis_core::models::{SourceType, WallpaperPreview};
use muralis_core::sources::{AspectRatioFilter, WallpaperSource};

const API_BASE: &str = "https://api.unsplash.com";

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct UnsplashConfig {
    pub enabled: bool,
    pub access_key: Option<String>,
}

pub fn create_sources(table: &toml::Table) -> Vec<Box<dyn WallpaperSource>> {
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
    vec![Box::new(UnsplashClient::new(key))]
}

pub struct UnsplashClient {
    access_key: String,
    client: reqwest::Client,
}

impl UnsplashClient {
    pub fn new(access_key: String) -> Self {
        Self {
            access_key,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl WallpaperSource for UnsplashClient {
    fn name(&self) -> &str {
        "Unsplash"
    }

    fn source_type(&self) -> &str {
        "unsplash"
    }

    async fn search(
        &self,
        query: &str,
        page: u32,
        per_page: u32,
        _aspect: AspectRatioFilter,
    ) -> Result<Vec<WallpaperPreview>> {
        let clamped = per_page.min(30);
        let resp: UnsplashSearchResponse = self
            .client
            .get(format!("{API_BASE}/search/photos"))
            .header("Authorization", format!("Client-ID {}", self.access_key))
            .query(&[
                ("query", query),
                ("page", &page.to_string()),
                ("per_page", &clamped.to_string()),
                ("orientation", "landscape"),
            ])
            .send()
            .await?
            .json()
            .await?;

        let previews = resp
            .results
            .into_iter()
            .map(|p| WallpaperPreview {
                source_type: SourceType::new("unsplash"),
                source_id: p.id.clone(),
                source_url: p.links.html,
                thumbnail_url: p.urls.small,
                full_url: p.urls.raw,
                width: p.width,
                height: p.height,
                tags: p.tags.into_iter().map(|t| t.title).collect(),
            })
            .collect();
        Ok(previews)
    }

    async fn download(&self, preview: &WallpaperPreview) -> Result<bytes::Bytes> {
        let bytes = self
            .client
            .get(&preview.full_url)
            .send()
            .await?
            .bytes()
            .await?;
        Ok(bytes)
    }
}

// -- API response types --

#[derive(Debug, Deserialize)]
struct UnsplashSearchResponse {
    results: Vec<UnsplashPhoto>,
}

#[derive(Debug, Deserialize)]
struct UnsplashPhoto {
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
    small: String,
}

#[derive(Debug, Deserialize)]
struct UnsplashLinks {
    html: String,
}

#[derive(Debug, Deserialize)]
struct UnsplashTag {
    title: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOCK_RESPONSE: &str = r##"{
        "total": 1,
        "total_pages": 1,
        "results": [
            {
                "id": "uns_001",
                "width": 5000,
                "height": 3000,
                "color": "#000000",
                "urls": {
                    "raw": "https://images.unsplash.com/photo-001",
                    "full": "https://images.unsplash.com/photo-001?q=100",
                    "regular": "https://images.unsplash.com/photo-001?w=1080",
                    "small": "https://images.unsplash.com/photo-001?w=400",
                    "thumb": "https://images.unsplash.com/photo-001?w=200"
                },
                "links": {
                    "self": "https://api.unsplash.com/photos/uns_001",
                    "html": "https://unsplash.com/photos/uns_001",
                    "download": "https://unsplash.com/photos/uns_001/download"
                },
                "tags": [
                    {"title": "mountain"},
                    {"title": "sky"}
                ]
            }
        ]
    }"##;

    #[test]
    fn test_parse_unsplash_response() {
        let resp: UnsplashSearchResponse = serde_json::from_str(MOCK_RESPONSE).unwrap();
        assert_eq!(resp.results.len(), 1);
        let p = &resp.results[0];
        assert_eq!(p.id, "uns_001");
        assert_eq!(p.width, 5000);
        assert_eq!(p.tags.len(), 2);
    }

    #[test]
    fn test_unsplash_to_preview() {
        let resp: UnsplashSearchResponse = serde_json::from_str(MOCK_RESPONSE).unwrap();
        let previews: Vec<WallpaperPreview> = resp
            .results
            .into_iter()
            .map(|p| WallpaperPreview {
                source_type: SourceType::new("unsplash"),
                source_id: p.id.clone(),
                source_url: p.links.html,
                thumbnail_url: p.urls.small,
                full_url: p.urls.raw,
                width: p.width,
                height: p.height,
                tags: p.tags.into_iter().map(|t| t.title).collect(),
            })
            .collect();

        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].source_id, "uns_001");
        assert_eq!(previews[0].tags, vec!["mountain", "sky"]);
    }
}
