use std::fmt;

use async_trait::async_trait;

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

    pub fn ratio_pair(self) -> Option<(u32, u32)> {
        match self {
            Self::All => None,
            Self::Ratio16x9 => Some((16, 9)),
            Self::Ratio21x9 => Some((21, 9)),
            Self::Ratio32x9 => Some((32, 9)),
            Self::Ratio16x10 => Some((16, 10)),
            Self::Ratio4x3 => Some((4, 3)),
            Self::Ratio3x2 => Some((3, 2)),
        }
    }

    pub fn ratio_value(&self) -> Option<f64> {
        self.ratio_pair().map(|(w, h)| w as f64 / h as f64)
    }

    pub fn matches(self, width: u32, height: u32) -> bool {
        let Some(target) = self.ratio_value() else {
            return true;
        };
        if width == 0 || height == 0 {
            return true;
        }
        let ratio = width as f64 / height as f64;
        (ratio - target).abs() < 0.1
    }

    pub fn from_dimensions(w: u32, h: u32) -> Self {
        if w == 0 || h == 0 {
            return Self::All;
        }
        let ratio = w as f64 / h as f64;
        Self::ALL
            .iter()
            .copied()
            .filter_map(|f| f.ratio_pair().map(|(rw, rh)| (f, rw as f64 / rh as f64)))
            .min_by(|a, b| {
                (ratio - a.1)
                    .abs()
                    .partial_cmp(&(ratio - b.1).abs())
                    .unwrap()
            })
            .map(|(v, _)| v)
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
    /// Display name / tab label (e.g. "Wallhaven", "Bing Daily")
    fn name(&self) -> &str;
    /// DB type string (e.g. "wallhaven", "feed")
    fn source_type(&self) -> &str;
    async fn search(
        &self,
        query: &str,
        page: u32,
        per_page: u32,
        aspect: AspectRatioFilter,
    ) -> Result<Vec<WallpaperPreview>>;
    async fn download(&self, preview: &WallpaperPreview) -> Result<bytes::Bytes>;
}

pub struct SourceRegistry {
    sources: Vec<Box<dyn WallpaperSource>>,
}

impl SourceRegistry {
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
        }
    }

    pub fn register(&mut self, source: Box<dyn WallpaperSource>) {
        self.sources.push(source);
    }

    pub fn names(&self) -> Vec<&str> {
        self.sources.iter().map(|s| s.name()).collect()
    }

    pub fn get(&self, name: &str) -> Option<&dyn WallpaperSource> {
        self.sources
            .iter()
            .find(|s| s.name() == name)
            .map(|s| s.as_ref())
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn WallpaperSource> {
        self.sources.iter().map(|s| s.as_ref())
    }
}

impl Default for SourceRegistry {
    fn default() -> Self {
        Self::new()
    }
}
