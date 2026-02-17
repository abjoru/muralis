use async_trait::async_trait;
use serde::Deserialize;

use crate::config::WallhavenConfig;
use crate::error::Result;
use crate::models::{SourceType, WallpaperPreview};
use crate::sources::{AspectRatioFilter, WallpaperSource};

const API_BASE: &str = "https://wallhaven.cc/api/v1";

pub struct WallhavenClient {
    config: WallhavenConfig,
    client: reqwest::Client,
}

impl WallhavenClient {
    pub fn new(config: WallhavenConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl WallpaperSource for WallhavenClient {
    async fn search(&self, query: &str, page: u32, aspect: AspectRatioFilter) -> Result<Vec<WallpaperPreview>> {
        let mut req = self
            .client
            .get(format!("{API_BASE}/search"))
            .query(&[
                ("q", query),
                ("page", &page.to_string()),
                ("categories", &self.config.categories),
                ("purity", &self.config.purity),
            ]);

        if let Some(ref key) = self.config.api_key {
            req = req.query(&[("apikey", key)]);
        }

        if let Some(ratio) = aspect.to_wallhaven_ratio() {
            req = req.query(&[("ratios", ratio)]);
        }

        let resp: WallhavenResponse = req.send().await?.json().await?;
        let previews = resp
            .data
            .into_iter()
            .map(|w| WallpaperPreview {
                source_type: SourceType::Wallhaven,
                source_id: w.id.clone(),
                source_url: w.url,
                thumbnail_url: w.thumbs.small,
                full_url: w.path,
                width: w.dimension_x,
                height: w.dimension_y,
                tags: w
                    .tags
                    .into_iter()
                    .map(|t| t.name)
                    .collect(),
            })
            .collect();
        Ok(previews)
    }

    async fn download(&self, preview: &WallpaperPreview) -> Result<bytes::Bytes> {
        let bytes = self.client.get(&preview.full_url).send().await?.bytes().await?;
        Ok(bytes)
    }

    fn name(&self) -> &str {
        "wallhaven"
    }
}

// -- API response types --

#[derive(Debug, Deserialize)]
struct WallhavenResponse {
    data: Vec<WallhavenWallpaper>,
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
    small: String,
}

#[derive(Debug, Deserialize)]
struct WallhavenTag {
    name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOCK_RESPONSE: &str = r##"{
        "data": [
            {
                "id": "abc123",
                "url": "https://wallhaven.cc/w/abc123",
                "short_url": "https://whvn.cc/abc123",
                "views": 1000,
                "favorites": 50,
                "source": "",
                "purity": "sfw",
                "category": "general",
                "dimension_x": 3840,
                "dimension_y": 2160,
                "resolution": "3840x2160",
                "ratio": "1.78",
                "file_size": 5000000,
                "file_type": "image/jpeg",
                "created_at": "2024-01-01 00:00:00",
                "colors": ["#000000"],
                "path": "https://w.wallhaven.cc/full/ab/wallhaven-abc123.jpg",
                "thumbs": {
                    "large": "https://th.wallhaven.cc/lg/ab/abc123.jpg",
                    "original": "https://th.wallhaven.cc/orig/ab/abc123.jpg",
                    "small": "https://th.wallhaven.cc/small/ab/abc123.jpg"
                },
                "tags": [
                    {"id": 1, "name": "landscape", "alias": "", "category_id": 1, "category": "General", "purity": "sfw", "created_at": "2024-01-01"},
                    {"id": 2, "name": "nature", "alias": "", "category_id": 1, "category": "General", "purity": "sfw", "created_at": "2024-01-01"}
                ]
            }
        ],
        "meta": {
            "current_page": 1,
            "last_page": 1,
            "per_page": 24,
            "total": 1
        }
    }"##;

    #[test]
    fn test_parse_wallhaven_response() {
        let resp: WallhavenResponse = serde_json::from_str(MOCK_RESPONSE).unwrap();
        assert_eq!(resp.data.len(), 1);
        let w = &resp.data[0];
        assert_eq!(w.id, "abc123");
        assert_eq!(w.dimension_x, 3840);
        assert_eq!(w.dimension_y, 2160);
        assert_eq!(w.tags.len(), 2);
        assert_eq!(w.tags[0].name, "landscape");
    }

    #[test]
    fn test_wallhaven_to_preview() {
        let resp: WallhavenResponse = serde_json::from_str(MOCK_RESPONSE).unwrap();
        let previews: Vec<WallpaperPreview> = resp
            .data
            .into_iter()
            .map(|w| WallpaperPreview {
                source_type: SourceType::Wallhaven,
                source_id: w.id.clone(),
                source_url: w.url,
                thumbnail_url: w.thumbs.small,
                full_url: w.path,
                width: w.dimension_x,
                height: w.dimension_y,
                tags: w.tags.into_iter().map(|t| t.name).collect(),
            })
            .collect();

        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].source_id, "abc123");
        assert_eq!(previews[0].width, 3840);
        assert_eq!(previews[0].tags, vec!["landscape", "nature"]);
    }
}
