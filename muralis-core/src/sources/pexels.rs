use async_trait::async_trait;
use serde::Deserialize;

use crate::error::Result;
use crate::models::{SourceType, WallpaperPreview};
use crate::sources::{AspectRatioFilter, WallpaperSource};

const API_BASE: &str = "https://api.pexels.com/v1";

pub struct PexelsClient {
    api_key: String,
    client: reqwest::Client,
}

impl PexelsClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl WallpaperSource for PexelsClient {
    async fn search(
        &self,
        query: &str,
        page: u32,
        _aspect: AspectRatioFilter,
    ) -> Result<Vec<WallpaperPreview>> {
        let resp: PexelsSearchResponse = self
            .client
            .get(format!("{API_BASE}/search"))
            .header("Authorization", &self.api_key)
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
            .photos
            .into_iter()
            .map(|p| {
                let full_url = p.src.original.clone();
                WallpaperPreview {
                    source_type: SourceType::Pexels,
                    source_id: p.id.to_string(),
                    source_url: p.url,
                    thumbnail_url: p.src.small,
                    full_url,
                    width: p.width,
                    height: p.height,
                    tags: Vec::new(), // Pexels doesn't return tags in search
                }
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
        "pexels"
    }
}

// -- API response types --

#[derive(Debug, Deserialize)]
struct PexelsSearchResponse {
    photos: Vec<PexelsPhoto>,
}

#[derive(Debug, Deserialize)]
struct PexelsPhoto {
    id: u64,
    width: u32,
    height: u32,
    url: String,
    src: PexelsSrc,
}

#[derive(Debug, Deserialize)]
struct PexelsSrc {
    original: String,
    small: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOCK_RESPONSE: &str = r##"{
        "total_results": 1,
        "page": 1,
        "per_page": 24,
        "photos": [
            {
                "id": 12345,
                "width": 4000,
                "height": 2500,
                "url": "https://www.pexels.com/photo/12345/",
                "photographer": "Test",
                "photographer_url": "",
                "photographer_id": 1,
                "avg_color": "#000000",
                "src": {
                    "original": "https://images.pexels.com/photos/12345/pexels-photo-12345.jpeg",
                    "large2x": "https://images.pexels.com/photos/12345/pexels-photo-12345.jpeg?w=1880",
                    "large": "https://images.pexels.com/photos/12345/pexels-photo-12345.jpeg?w=940",
                    "medium": "https://images.pexels.com/photos/12345/pexels-photo-12345.jpeg?w=350",
                    "small": "https://images.pexels.com/photos/12345/pexels-photo-12345.jpeg?w=130",
                    "portrait": "",
                    "landscape": "",
                    "tiny": ""
                },
                "liked": false,
                "alt": "Test photo"
            }
        ]
    }"##;

    #[test]
    fn test_parse_pexels_response() {
        let resp: PexelsSearchResponse = serde_json::from_str(MOCK_RESPONSE).unwrap();
        assert_eq!(resp.photos.len(), 1);
        let p = &resp.photos[0];
        assert_eq!(p.id, 12345);
        assert_eq!(p.width, 4000);
    }

    #[test]
    fn test_pexels_to_preview() {
        let resp: PexelsSearchResponse = serde_json::from_str(MOCK_RESPONSE).unwrap();
        let previews: Vec<WallpaperPreview> = resp
            .photos
            .into_iter()
            .map(|p| WallpaperPreview {
                source_type: SourceType::Pexels,
                source_id: p.id.to_string(),
                source_url: p.url,
                thumbnail_url: p.src.small,
                full_url: p.src.original,
                width: p.width,
                height: p.height,
                tags: Vec::new(),
            })
            .collect();

        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].source_id, "12345");
        assert_eq!(previews[0].width, 4000);
    }
}
