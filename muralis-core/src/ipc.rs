use serde::{Deserialize, Serialize};

use crate::error::{MuralisError, Result};
use crate::models::DisplayMode;
use crate::paths::MuralisPaths;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum IpcRequest {
    Status,
    Next,
    Prev,
    SetWallpaper { id: String },
    SetMode { mode: DisplayMode },
    Pause,
    Resume,
    Reload,
    Quit,
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
        };
        let data = serde_json::to_value(&status).unwrap();
        let resp = IpcResponse::ok_with_data(data);
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("abc123"));
        assert!(json.contains("42"));
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
            IpcRequest::Quit,
        ];

        for req in requests {
            let json = serde_json::to_string(&req).unwrap();
            let _parsed: IpcRequest = serde_json::from_str(&json).unwrap();
        }
    }
}
