use async_trait::async_trait;
use serde::Deserialize;

use crate::error::Result;
use crate::models::{SourceType, WallpaperPreview};
use crate::sources::{AspectRatioFilter, WallpaperSource};

const API_BASE: &str = "https://api.unsplash.com";

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
    async fn search(
        &self,
        query: &str,
        page: u32,
        _aspect: AspectRatioFilter,
    ) -> Result<Vec<WallpaperPreview>> {
        let resp: UnsplashSearchResponse = self
            .client
            .get(format!("{API_BASE}/search/photos"))
            .header("Authorization", format!("Client-ID {}", self.access_key))
            .query(&[
                ("query", query),
                ("page", &page.to_string()),
                ("per_page", "24"),
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
                source_type: SourceType::Unsplash,
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

    fn name(&self) -> &str {
        "unsplash"
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
                source_type: SourceType::Unsplash,
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
