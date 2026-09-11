use serde::{Deserialize, Serialize};

use crate::error::{MuralisError, Result};
use crate::models::{DisplayMode, Wallpaper};
use crate::paths::MuralisPaths;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum IpcRequest {
    Status,
    Next,
    Prev,
    SetWallpaper {
        id: String,
    },
    SetMode {
        mode: DisplayMode,
    },
    /// The **Library** — every kept wallpaper. A bare `favorites` asks for all
    /// of it; `offset`/`limit` window it for a Consumer that draws a page at a
    /// time. A row serializes to ~445 B, so a 1000-wallpaper Library is a
    /// ~435 KiB single socket line, against ~7 KiB for a 16-item grid page.
    Favorites {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        offset: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<u32>,
    },
    Pause,
    Resume,
    Reload,
    /// Hold the connection open and stream `DaemonEvent` lines until the
    /// Consumer hangs up. Unlike every other variant this gets no
    /// `IpcResponse`: the first lines back are a **snapshot** of daemon state
    /// as events, so a Consumer that just reconnected is current without a
    /// follow-up round-trip.
    Subscribe,
    Quit,
}

/// One line of the **subscription**: something the daemon did, pushed to every
/// **Consumer** holding a `Subscribe` connection open.
///
/// Tagged like `IpcRequest` so a Consumer switches on one field, and carries
/// everything needed to act — `WallpaperChanged` holds the whole `Wallpaper`
/// row because `SessionData.setWallpaper()` wants a path, and asking `Status`
/// for it would defeat the point of pushing. Adding a variant is
/// backwards-compatible; changing a shipped one is not.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum DaemonEvent {
    /// A wallpaper reached the screen. Emitted only on a successful apply —
    /// a failed one leaves the previous wallpaper up, and saying otherwise
    /// would have a Consumer regenerate its palette from an image nobody sees.
    /// Boxed only to keep the variants a similar size — `Box<T>` serializes
    /// as the row itself, so the wire shape is unaffected.
    WallpaperChanged {
        wallpaper: Box<Wallpaper>,
    },
    ModeChanged {
        mode: DisplayMode,
    },
    PauseChanged {
        paused: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum IpcResponse {
    Ok {
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<serde_json::Value>,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub running: bool,
    pub mode: DisplayMode,
    pub paused: bool,
    pub current_wallpaper: Option<String>,
    pub wallpaper_count: u32,
    pub next_change: Option<String>,
    /// The modes this daemon's config can actually run
    /// (`Config::available_modes`). A Consumer offers these and greys out the
    /// rest rather than presenting six equal choices of which two would
    /// silently stop the wallpaper changing; one that ignores the field
    /// behaves as before, and the daemon refuses an unusable mode regardless.
    pub available_modes: Vec<DisplayMode>,
    /// Why the last wallpaper apply failed, if it did. `None` once one
    /// succeeds. A Consumer reads this to tell "nothing applied yet" apart from
    /// "the backend refused" — the failure used to be a `warn!` nobody saw.
    pub last_error: Option<String>,
}

/// The `Favorites` response: one window onto the **Library**, plus the total so
/// a Consumer can size its scrollbar without asking for everything.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FavoritesPage {
    pub wallpapers: Vec<crate::models::Wallpaper>,
    pub total: u32,
    pub offset: u32,
}

impl IpcResponse {
    pub fn ok() -> Self {
        Self::Ok { data: None }
    }

    pub fn ok_with_data(data: serde_json::Value) -> Self {
        Self::Ok { data: Some(data) }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self::Error {
            message: msg.into(),
        }
    }
}

/// Send a request to the daemon and receive a response.
pub async fn send_request(request: &IpcRequest) -> Result<IpcResponse> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    let socket_path = MuralisPaths::socket_path();
    let stream = UnixStream::connect(&socket_path)
        .await
        .map_err(|e| MuralisError::Ipc(format!("failed to connect to daemon: {e}")))?;

    let (reader, mut writer) = stream.into_split();

    let mut line = serde_json::to_string(request)?;
    line.push('\n');
    writer.write_all(line.as_bytes()).await?;
    writer.shutdown().await?;

    let mut buf_reader = BufReader::new(reader);
    let mut response_line = String::new();
    buf_reader.read_line(&mut response_line).await?;

    let response: IpcResponse = serde_json::from_str(response_line.trim())?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialize() {
        let req = IpcRequest::Status;
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"command":"status"}"#);

        let req = IpcRequest::SetWallpaper { id: "abc".into() };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""command":"set_wallpaper""#));
        assert!(json.contains(r#""id":"abc""#));
    }

    #[test]
    fn test_request_deserialize() {
        let json = r#"{"command":"next"}"#;
        let req: IpcRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(req, IpcRequest::Next));

        let json = r#"{"command":"set_mode","mode":"random"}"#;
        let req: IpcRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(
            req,
            IpcRequest::SetMode {
                mode: DisplayMode::Random
            }
        ));
    }

    #[test]
    fn test_response_serialize() {
        let resp = IpcResponse::ok();
        let json = serde_json::to_string(&resp).unwrap();
        assert_eq!(json, r#"{"status":"ok"}"#);

        let resp = IpcResponse::error("not found");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""status":"error""#));
        assert!(json.contains("not found"));
    }

    #[test]
    fn test_response_with_data() {
        let status = DaemonStatus {
            running: true,
            mode: DisplayMode::Random,
            paused: false,
            current_wallpaper: Some("abc123".into()),
            wallpaper_count: 42,
            next_change: Some("2025-01-01T01:00:00Z".into()),
            last_error: None,
            available_modes: vec![DisplayMode::Static, DisplayMode::Random],
        };
        let data = serde_json::to_value(&status).unwrap();
        let resp = IpcResponse::ok_with_data(data);
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("abc123"));
        assert!(json.contains("42"));
    }

    #[test]
    fn status_names_the_usable_modes_on_the_wire() {
        // A Consumer greys out its switcher off this array, so the names have
        // to be the ones `SetMode` accepts back.
        let status = DaemonStatus {
            running: true,
            mode: DisplayMode::Random,
            paused: false,
            current_wallpaper: None,
            wallpaper_count: 0,
            next_change: None,
            available_modes: vec![DisplayMode::RandomStartup, DisplayMode::Schedule],
            last_error: None,
        };

        let json = serde_json::to_string(&status).unwrap();

        assert!(
            json.contains(r#""available_modes":["random_startup","schedule"]"#),
            "unexpected wire shape: {json}"
        );
    }

    #[test]
    fn favorites_asks_for_the_whole_library_by_default() {
        // The CLI contract is "the whole Library"; paging is the Consumer's
        // opt-in, so the bare command must carry no window at all.
        let json = r#"{"command":"favorites"}"#;
        let req: IpcRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(
            req,
            IpcRequest::Favorites {
                offset: None,
                limit: None
            }
        ));

        let line = serde_json::to_string(&IpcRequest::Favorites {
            offset: None,
            limit: None,
        })
        .unwrap();
        assert_eq!(line, r#"{"command":"favorites"}"#);
    }

    #[test]
    fn favorites_carries_a_window_when_a_consumer_pages() {
        let json = r#"{"command":"favorites","offset":32,"limit":16}"#;
        let req: IpcRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(
            req,
            IpcRequest::Favorites {
                offset: Some(32),
                limit: Some(16)
            }
        ));
    }

    #[test]
    fn subscribe_is_a_bare_command_like_the_one_shots() {
        let req: IpcRequest = serde_json::from_str(r#"{"command":"subscribe"}"#).unwrap();
        assert!(matches!(req, IpcRequest::Subscribe));
        assert_eq!(
            serde_json::to_string(&IpcRequest::Subscribe).unwrap(),
            r#"{"command":"subscribe"}"#
        );
    }

    #[test]
    fn a_wallpaper_event_carries_the_path_without_a_follow_up_request() {
        // The DMS Widget hands `file_path` straight to `setWallpaper()`; an
        // event it has to chase with a `Status` is not a push.
        let event = DaemonEvent::WallpaperChanged {
            wallpaper: Box::new(Wallpaper {
                id: "abc123".into(),
                source_type: crate::models::SourceType::new("wallhaven"),
                source_id: "x7k2m".into(),
                source_url: None,
                width: 3840,
                height: 2160,
                tags: vec!["night".into()],
                file_path: "/home/u/.local/share/muralis/wallpapers/abc123.jpg".into(),
                added_at: "2026-09-10T00:00:00Z".into(),
                last_used: None,
                use_count: 0,
            }),
        };

        let json = serde_json::to_string(&event).unwrap();

        assert!(
            json.starts_with(r#"{"event":"wallpaper_changed","wallpaper":{"#),
            "unexpected wire shape: {json}"
        );
        assert!(
            json.contains(r#""file_path":"/home/u/.local/share/muralis/wallpapers/abc123.jpg""#)
        );
    }

    #[test]
    fn the_other_event_kinds_name_themselves_the_same_way() {
        assert_eq!(
            serde_json::to_string(&DaemonEvent::ModeChanged {
                mode: DisplayMode::Sequential
            })
            .unwrap(),
            r#"{"event":"mode_changed","mode":"sequential"}"#
        );
        assert_eq!(
            serde_json::to_string(&DaemonEvent::PauseChanged { paused: true }).unwrap(),
            r#"{"event":"pause_changed","paused":true}"#
        );
    }

    #[test]
    fn test_roundtrip_all_requests() {
        let requests = vec![
            IpcRequest::Status,
            IpcRequest::Next,
            IpcRequest::Prev,
            IpcRequest::SetWallpaper { id: "test".into() },
            IpcRequest::SetMode {
                mode: DisplayMode::Workspace,
            },
            IpcRequest::Pause,
            IpcRequest::Resume,
            IpcRequest::Reload,
            IpcRequest::Subscribe,
            IpcRequest::Quit,
        ];

        for req in requests {
            let json = serde_json::to_string(&req).unwrap();
            let _parsed: IpcRequest = serde_json::from_str(&json).unwrap();
        }
    }
}
