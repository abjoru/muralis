use muralis_core::ipc::{DaemonStatus, IpcResponse};
use muralis_core::models::{BackendType, DisplayMode, Wallpaper, WallpaperPreview};
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    ToggleSelect(usize),
    #[allow(dead_code)]
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

    // settings
    ToggleSettings,
    SettingsModeChanged(DisplayMode),
    SettingsBackendChanged(BackendType),
    SettingsIntervalChanged(String),
    DaemonNext,
    DaemonPrev,
    DaemonTogglePause,
    DaemonIpcResult(Result<IpcResponse, String>),
    ConfigSaved,
    ConfigSaveError(String),

    // errors
    Error(String),
    Noop,
}
