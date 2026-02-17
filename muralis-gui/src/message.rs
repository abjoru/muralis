use muralis_core::ipc::DaemonStatus;
use muralis_core::models::{Wallpaper, WallpaperPreview};
pub use muralis_core::sources::AspectRatioFilter;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Tab {
    Favorites,
    Wallhaven,
    Unsplash,
    Pexels,
    Feeds,
}

impl Tab {
    pub fn label(&self) -> &str {
        match self {
            Self::Favorites => "Favorites",
            Self::Wallhaven => "Wallhaven",
            Self::Unsplash => "Unsplash",
            Self::Pexels => "Pexels",
            Self::Feeds => "Feeds",
        }
    }

    pub const ALL: &[Tab] = &[
        Tab::Favorites,
        Tab::Wallhaven,
        Tab::Unsplash,
        Tab::Pexels,
        Tab::Feeds,
    ];
}

#[derive(Debug, Clone)]
pub enum Message {
    // navigation
    TabSelected(Tab),

    // search
    SearchQueryChanged(String),
    SearchSubmit,
    SearchResults(Tab, Vec<WallpaperPreview>),
    SearchError(String),
    SearchLoading(bool),

    // thumbnails
    ThumbnailClicked(usize),
    ThumbnailLoaded(String, Vec<u8>),

    // preview
    PreviewLoaded(Vec<u8>),
    ClosePreview,

    // actions
    Favorite(WallpaperPreview),
    Favorited(String),
    Unfavorite(String),
    Unfavorited(String),
    Apply(String),
    Applied,
    Blacklist(WallpaperPreview),
    Blacklisted,

    // favorites
    FavoritesLoaded(Vec<Wallpaper>),

    // daemon
    #[allow(dead_code)]
    DaemonStatusUpdate(Option<DaemonStatus>),

    // multi-select
    ToggleSelect(usize),
    RangeSelect(usize),
    ClearSelection,
    BatchFavorite,
    BatchBlacklist,
    BatchUnfavorite,

    // pagination
    NextPage,
    PrevPage,

    // aspect ratio filter
    AspectFilterChanged(AspectRatioFilter),

    // crop overlay
    MonitorsDetected(u32, u32),
    ToggleCropOverlay,
    CropOverlayReady(Vec<u8>),

    // errors
    Error(String),
    Noop,
}
