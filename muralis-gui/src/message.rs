use muralis_core::ipc::{DaemonStatus, IpcResponse};
use muralis_core::models::{BackendType, DisplayMode, Wallpaper, WallpaperPreview};
pub use muralis_core::sources::AspectRatioFilter;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Tab {
    Favorites,
    Source(String),
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
