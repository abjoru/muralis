use std::io::{BufReader, Cursor};
use std::sync::{Arc, LazyLock};

use async_trait::async_trait;
use image::ImageReader;
use scraper::{Html, Selector};
use serde::Deserialize;
use tokio::sync::Semaphore;

use muralis_core::error::Result;
use muralis_core::models::{SourceType, WallpaperPreview};
use muralis_core::sources::{AspectRatioFilter, WallpaperSource};

static IMG_SEL: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("img[src]").expect("valid selector"));

/// Max concurrent dimension fetch tasks.
const MAX_DIM_CONCURRENCY: usize = 8;

/// Skip dimension fetch if content-length exceeds this (server ignored Range).
const MAX_DIM_BODY_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, Deserialize)]
pub struct FeedConfig {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub enabled: bool,
}

pub fn create_sources(
    table: &toml::Table,
    client: reqwest::Client,
) -> Vec<Box<dyn WallpaperSource>> {
    let Some(val) = table.get("feeds") else {
        return Vec::new();
    };
    let Some(arr) = val.as_array() else {
        return Vec::new();
    };
    let mut sources: Vec<Box<dyn WallpaperSource>> = Vec::new();
    for item in arr {
        let config: FeedConfig = match item.clone().try_into() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("failed to parse feed config: {e}");
                continue;
            }
        };
        if config.enabled {
            sources.push(Box::new(FeedSource {
                config,
                client: client.clone(),
            }));
        }
    }
    sources
}

pub struct FeedSource {
    config: FeedConfig,
    client: reqwest::Client,
}

#[async_trait]
impl WallpaperSource for FeedSource {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn source_type(&self) -> &str {
        "feed"
    }

    async fn search(
        &self,
        _query: &str,
        _page: u32,
        _per_page: u32,
        _aspect: AspectRatioFilter,
    ) -> Result<Vec<WallpaperPreview>> {
        let body = self
            .client
            .get(&self.config.url)
            .send()
            .await?
            .bytes()
            .await?;
        let feed = feed_rs::parser::parse(&body[..]).map_err(|e| {
            muralis_core::error::MuralisError::Source(format!("feed parse error: {e}"))
        })?;

        let mut previews = Vec::new();

        for entry in &feed.entries {
            if let Some((image_url, width, height)) = extract_image(entry) {
                let id = entry.id.replace(['/', ':', '.'], "_");
                let title = entry
                    .title
                    .as_ref()
                    .map(|t| t.content.clone())
                    .unwrap_or_default();

                previews.push(WallpaperPreview {
                    source_type: SourceType::new("feed"),
                    source_id: id,
                    source_url: entry
                        .links
                        .first()
                        .map(|l| l.href.clone())
                        .unwrap_or_default(),
                    thumbnail_url: image_url.clone(),
                    full_url: image_url,
                    width,
                    height,
                    tags: vec![title, self.config.name.clone()],
                });
            }
        }

        // Fetch dimensions for entries with unknown sizes
        let semaphore = Arc::new(Semaphore::new(MAX_DIM_CONCURRENCY));
        let mut handles = Vec::new();
        for (i, p) in previews.iter().enumerate() {
            if p.width == 0 && p.height == 0 {
                let client = self.client.clone();
                let url = p.thumbnail_url.clone();
                let permit = semaphore.clone();
                handles.push((
                    i,
                    tokio::spawn(async move {
                        let _permit = permit.acquire().await;
                        fetch_dimensions(&client, &url).await
                    }),
                ));
            }
        }
        for (i, handle) in handles {
            match handle.await {
                Ok((w, h)) => {
                    previews[i].width = w;
                    previews[i].height = h;
                }
                Err(e) => {
                    tracing::warn!("dimension fetch task failed: {e}");
                }
            }
        }

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

/// Fetch image dimensions via partial HTTP download (first 32KB).
async fn fetch_dimensions(client: &reqwest::Client, url: &str) -> (u32, u32) {
    let resp = match client
        .get(url)
        .header("Range", "bytes=0-32767")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!("dimension fetch request failed for {url}: {e}");
            return (0, 0);
        }
    };

    // If server ignored Range and sends full body, check content-length
    if let Some(len) = resp.content_length() {
        if resp.status() == reqwest::StatusCode::OK && len > MAX_DIM_BODY_BYTES {
            tracing::debug!("skipping dimension fetch for {url}: body too large ({len} bytes)");
            return (0, 0);
        }
    }

    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => {
            tracing::debug!("dimension fetch body read failed for {url}: {e}");
            return (0, 0);
        }
    };
    let cursor = Cursor::new(bytes.as_ref());
    let reader = match ImageReader::new(BufReader::new(cursor)).with_guessed_format() {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!("dimension fetch format guess failed for {url}: {e}");
            return (0, 0);
        }
    };
    match reader.into_dimensions() {
        Ok(dims) => dims,
        Err(e) => {
            tracing::debug!("dimension fetch decode failed for {url}: {e}");
            (0, 0)
        }
    }
}

/// Extract image URL and dimensions from feed entry.
/// Returns (url, width, height). Dimensions are 0 when unknown from metadata.
fn extract_image(entry: &feed_rs::model::Entry) -> Option<(String, u32, u32)> {
    // 1. media content (may include dimensions)
    for media in &entry.media {
        for content in &media.content {
            if let Some(ref url) = content.url {
                let url_str = url.as_str();
                if is_image_url(url_str)
                    || content
                        .content_type
                        .as_ref()
                        .is_some_and(|t| t.ty() == "image")
                {
                    let w = content.width.unwrap_or(0);
                    let h = content.height.unwrap_or(0);
                    return Some((url_str.to_string(), w, h));
                }
            }
        }
        if let Some(thumb) = media.thumbnails.first() {
            let w = thumb.image.width.unwrap_or(0);
            let h = thumb.image.height.unwrap_or(0);
            return Some((thumb.image.uri.clone(), w, h));
        }
    }

    // 2. enclosures / links with image type
    for link in &entry.links {
        if link
            .media_type
            .as_deref()
            .is_some_and(|t| t.starts_with("image/"))
        {
            return Some((link.href.clone(), 0, 0));
        }
    }

    // 3. inline HTML img extraction
    if let Some(ref content) = entry.content {
        if let Some(ref body) = content.body {
            if let Some(url) = extract_img_from_html(body) {
                return Some((url, 0, 0));
            }
        }
    }
    if let Some(ref summary) = entry.summary {
        if let Some(url) = extract_img_from_html(&summary.content) {
            return Some((url, 0, 0));
        }
    }

    None
}

fn extract_img_from_html(html: &str) -> Option<String> {
    let doc = Html::parse_fragment(html);
    doc.select(&IMG_SEL)
        .next()
        .and_then(|el| el.value().attr("src"))
        .map(|s| s.to_string())
}

fn is_image_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".png")
        || lower.ends_with(".webp")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_img_from_html() {
        let html =
            r#"<p>Hello</p><img src="https://example.com/image.jpg" alt="test"><p>World</p>"#;
        let url = extract_img_from_html(html).unwrap();
        assert_eq!(url, "https://example.com/image.jpg");
    }

    #[test]
    fn test_extract_img_from_html_no_img() {
        let html = "<p>No images here</p>";
        assert!(extract_img_from_html(html).is_none());
    }

    #[test]
    fn test_is_image_url() {
        assert!(is_image_url("https://example.com/photo.jpg"));
        assert!(is_image_url("https://example.com/photo.PNG"));
        assert!(!is_image_url("https://example.com/video.mp4"));
    }

    #[test]
    fn test_parse_rss_feed() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
        <rss version="2.0">
            <channel>
                <title>Test Feed</title>
                <item>
                    <title>Beautiful Sunset</title>
                    <link>https://example.com/sunset</link>
                    <guid>sunset-001</guid>
                    <enclosure url="https://example.com/sunset.jpg" type="image/jpeg" length="500000"/>
                </item>
                <item>
                    <title>Mountain View</title>
                    <link>https://example.com/mountain</link>
                    <guid>mountain-001</guid>
                    <description><![CDATA[<p>A great view</p><img src="https://example.com/mountain.jpg">]]></description>
                </item>
                <item>
                    <title>No Image</title>
                    <link>https://example.com/text</link>
                    <guid>text-001</guid>
                    <description>Just text content</description>
                </item>
            </channel>
        </rss>"#;

        let feed = feed_rs::parser::parse(&xml[..]).unwrap();
        assert_eq!(feed.entries.len(), 3);

        // entry with enclosure
        let img = extract_image(&feed.entries[0]);
        assert_eq!(
            img.as_ref().map(|(u, _, _)| u.as_str()),
            Some("https://example.com/sunset.jpg")
        );

        // entry with inline img
        let img = extract_image(&feed.entries[1]);
        assert_eq!(
            img.as_ref().map(|(u, _, _)| u.as_str()),
            Some("https://example.com/mountain.jpg")
        );

        // entry with no image
        assert!(extract_image(&feed.entries[2]).is_none());
    }

    #[test]
    fn test_media_content_with_dimensions() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
        <rss version="2.0" xmlns:media="http://search.yahoo.com/mrss/">
            <channel>
                <title>Test</title>
                <item>
                    <title>Wide Image</title>
                    <guid>wide-001</guid>
                    <media:content url="https://example.com/wide.jpg" type="image/jpeg" width="1920" height="1080"/>
                </item>
            </channel>
        </rss>"#;

        let feed = feed_rs::parser::parse(&xml[..]).unwrap();
        let (url, w, h) = extract_image(&feed.entries[0]).unwrap();
        assert_eq!(url, "https://example.com/wide.jpg");
        assert_eq!(w, 1920);
        assert_eq!(h, 1080);
    }

    #[test]
    fn test_media_thumbnail_fallback_with_dimensions() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
        <rss version="2.0" xmlns:media="http://search.yahoo.com/mrss/">
            <channel>
                <title>Test</title>
                <item>
                    <title>Thumb Only</title>
                    <guid>thumb-001</guid>
                    <media:thumbnail url="https://example.com/thumb.jpg" width="640" height="480"/>
                </item>
            </channel>
        </rss>"#;

        let feed = feed_rs::parser::parse(&xml[..]).unwrap();
        let (url, w, h) = extract_image(&feed.entries[0]).unwrap();
        assert_eq!(url, "https://example.com/thumb.jpg");
        assert_eq!(w, 640);
        assert_eq!(h, 480);
    }

    #[test]
    fn test_parse_atom_feed() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
        <feed xmlns="http://www.w3.org/2005/Atom">
            <title>Atom Test</title>
            <entry>
                <title>Atom Image</title>
                <id>atom-001</id>
                <link href="https://example.com/atom-entry"/>
                <content type="html"><![CDATA[<img src="https://example.com/atom.jpg">]]></content>
            </entry>
        </feed>"#;

        let feed = feed_rs::parser::parse(&xml[..]).unwrap();
        assert_eq!(feed.entries.len(), 1);

        let (url, w, h) = extract_image(&feed.entries[0]).unwrap();
        assert_eq!(url, "https://example.com/atom.jpg");
        assert_eq!(w, 0);
        assert_eq!(h, 0);
    }

    #[test]
    fn test_create_sources_filters_disabled() {
        let toml_str = r#"
            [[feeds]]
            name = "active"
            url = "https://example.com/feed1.xml"
            enabled = true

            [[feeds]]
            name = "inactive"
            url = "https://example.com/feed2.xml"
            enabled = false

            [[feeds]]
            name = "default"
            url = "https://example.com/feed3.xml"
        "#;
        let table: toml::Table = toml_str.parse().unwrap();
        let client = reqwest::Client::new();
        let sources = create_sources(&table, client);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name(), "active");
    }

    #[test]
    fn test_feed_config_defaults() {
        let toml_str = r#"
            name = "test"
            url = "https://example.com/feed.xml"
        "#;
        let config: FeedConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.name, "test");
        assert_eq!(config.url, "https://example.com/feed.xml");
        assert!(!config.enabled);
    }
}
