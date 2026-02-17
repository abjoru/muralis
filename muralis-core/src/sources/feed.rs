use scraper::{Html, Selector};

use crate::config::FeedConfig;
use crate::error::Result;
use crate::models::{SourceType, WallpaperPreview};

pub struct FeedClient {
    client: reqwest::Client,
}

impl Default for FeedClient {
    fn default() -> Self {
        Self::new()
    }
}

impl FeedClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("muralis/0.1")
                .build()
                .unwrap_or_default(),
        }
    }

    pub async fn fetch_feed(&self, config: &FeedConfig) -> Result<Vec<WallpaperPreview>> {
        let body = self.client.get(&config.url).send().await?.bytes().await?;
        let feed = feed_rs::parser::parse(&body[..])
            .map_err(|e| crate::error::MuralisError::Config(format!("feed parse error: {e}")))?;

        let mut previews = Vec::new();

        for entry in &feed.entries {
            if let Some(image_url) = extract_image(entry) {
                let id = entry.id.replace(['/', ':', '.'], "_");
                let title = entry
                    .title
                    .as_ref()
                    .map(|t| t.content.clone())
                    .unwrap_or_default();

                previews.push(WallpaperPreview {
                    source_type: SourceType::Feed,
                    source_id: id,
                    source_url: entry
                        .links
                        .first()
                        .map(|l| l.href.clone())
                        .unwrap_or_default(),
                    thumbnail_url: image_url.clone(),
                    full_url: image_url,
                    width: 0,
                    height: 0,
                    tags: vec![title, config.name.clone()],
                });
            }
        }

        Ok(previews)
    }

    pub async fn download_image(&self, url: &str) -> Result<bytes::Bytes> {
        let bytes = self.client.get(url).send().await?.bytes().await?;
        Ok(bytes)
    }
}

/// Extract image URL from feed entry using multiple strategies:
/// 1. Media content objects
/// 2. Enclosure/link with image type
/// 3. Inline HTML <img> parsing from content/summary
fn extract_image(entry: &feed_rs::model::Entry) -> Option<String> {
    // 1. media content
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
                    return Some(url_str.to_string());
                }
            }
        }
        if let Some(thumb) = media.thumbnails.first() {
            return Some(thumb.image.uri.clone());
        }
    }

    // 2. enclosures / links with image type
    for link in &entry.links {
        if link
            .media_type
            .as_deref()
            .is_some_and(|t| t.starts_with("image/"))
        {
            return Some(link.href.clone());
        }
    }

    // 3. inline HTML img extraction
    if let Some(ref content) = entry.content {
        if let Some(ref body) = content.body {
            if let Some(url) = extract_img_from_html(body) {
                return Some(url);
            }
        }
    }
    if let Some(ref summary) = entry.summary {
        if let Some(url) = extract_img_from_html(&summary.content) {
            return Some(url);
        }
    }

    None
}

fn extract_img_from_html(html: &str) -> Option<String> {
    let doc = Html::parse_fragment(html);
    let sel = Selector::parse("img[src]").ok()?;
    doc.select(&sel)
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
        assert_eq!(img.as_deref(), Some("https://example.com/sunset.jpg"));

        // entry with inline img
        let img = extract_image(&feed.entries[1]);
        assert_eq!(img.as_deref(), Some("https://example.com/mountain.jpg"));

        // entry with no image
        assert!(extract_image(&feed.entries[2]).is_none());
    }
}
