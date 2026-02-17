pub mod feed;
pub mod pexels;
pub mod unsplash;
pub mod wallhaven;

use std::fmt;

use async_trait::async_trait;

use crate::config::SourcesConfig;
use crate::error::Result;
use crate::models::WallpaperPreview;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AspectRatioFilter {
    All,
    Ratio16x9,
    Ratio21x9,
    Ratio32x9,
    Ratio16x10,
    Ratio4x3,
    Ratio3x2,
}

impl AspectRatioFilter {
    pub const ALL: &[AspectRatioFilter] = &[
        Self::All,
        Self::Ratio16x9,
        Self::Ratio21x9,
        Self::Ratio32x9,
        Self::Ratio16x10,
        Self::Ratio4x3,
        Self::Ratio3x2,
    ];

    pub fn matches(self, width: u32, height: u32) -> bool {
        if self == Self::All || width == 0 || height == 0 {
            return true;
        }
        let ratio = width as f64 / height as f64;
        let target = match self {
            Self::Ratio16x9 => 16.0 / 9.0,
            Self::Ratio21x9 => 21.0 / 9.0,
            Self::Ratio32x9 => 32.0 / 9.0,
            Self::Ratio16x10 => 16.0 / 10.0,
            Self::Ratio4x3 => 4.0 / 3.0,
            Self::Ratio3x2 => 3.0 / 2.0,
            Self::All => unreachable!(),
        };
        (ratio - target).abs() < 0.1
    }

    pub fn from_dimensions(w: u32, h: u32) -> Self {
        if w == 0 || h == 0 {
            return Self::All;
        }
        let ratio = w as f64 / h as f64;
        let candidates = [
            (Self::Ratio16x9, 16.0 / 9.0),
            (Self::Ratio21x9, 21.0 / 9.0),
            (Self::Ratio32x9, 32.0 / 9.0),
            (Self::Ratio16x10, 16.0 / 10.0),
            (Self::Ratio4x3, 4.0 / 3.0),
            (Self::Ratio3x2, 3.0 / 2.0),
        ];
        candidates
            .iter()
            .min_by(|a, b| {
                (ratio - a.1)
                    .abs()
                    .partial_cmp(&(ratio - b.1).abs())
                    .unwrap()
            })
            .map(|(v, _)| *v)
            .unwrap_or(Self::All)
    }

    pub fn to_wallhaven_ratio(&self) -> Option<&'static str> {
        match self {
            Self::All => None,
            Self::Ratio16x9 => Some("16x9"),
            Self::Ratio21x9 => Some("21x9"),
            Self::Ratio32x9 => Some("32x9"),
            Self::Ratio16x10 => Some("16x10"),
            Self::Ratio4x3 => Some("4x3"),
            Self::Ratio3x2 => Some("3x2"),
        }
    }
}

impl fmt::Display for AspectRatioFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => write!(f, "All"),
            Self::Ratio16x9 => write!(f, "16:9"),
            Self::Ratio21x9 => write!(f, "21:9"),
            Self::Ratio32x9 => write!(f, "32:9"),
            Self::Ratio16x10 => write!(f, "16:10"),
            Self::Ratio4x3 => write!(f, "4:3"),
            Self::Ratio3x2 => write!(f, "3:2"),
        }
    }
}

#[async_trait]
pub trait WallpaperSource: Send + Sync {
    async fn search(&self, query: &str, page: u32, aspect: AspectRatioFilter) -> Result<Vec<WallpaperPreview>>;
    async fn download(&self, preview: &WallpaperPreview) -> Result<bytes::Bytes>;
    fn name(&self) -> &str;
}

/// Create all enabled source clients from config.
pub fn create_sources(config: &SourcesConfig) -> Vec<Box<dyn WallpaperSource>> {
    let mut sources: Vec<Box<dyn WallpaperSource>> = Vec::new();

    if config.wallhaven.enabled {
        sources.push(Box::new(wallhaven::WallhavenClient::new(
            config.wallhaven.clone(),
        )));
    }

    if config.unsplash.enabled {
        if let Some(ref key) = config.unsplash.access_key {
            sources.push(Box::new(unsplash::UnsplashClient::new(key.clone())));
        }
    }

    if config.pexels.enabled {
        if let Some(ref key) = config.pexels.api_key {
            sources.push(Box::new(pexels::PexelsClient::new(key.clone())));
        }
    }

    sources
}
