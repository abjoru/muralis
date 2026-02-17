use serde::Deserialize;
use tokio::process::Command;

use crate::error::{MuralisError, Result};
use crate::models::MonitorInfo;

/// Detect connected monitors via `hyprctl monitors -j`.
pub async fn detect_monitors() -> Result<Vec<MonitorInfo>> {
    let output = Command::new("hyprctl")
        .args(["monitors", "-j"])
        .output()
        .await
        .map_err(|e| MuralisError::Backend(format!("failed to run hyprctl: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(MuralisError::Backend(format!(
            "hyprctl monitors failed: {stderr}"
        )));
    }

    let json = String::from_utf8_lossy(&output.stdout);
    parse_monitors(&json)
}

fn parse_monitors(json: &str) -> Result<Vec<MonitorInfo>> {
    let raw: Vec<HyprMonitor> = serde_json::from_str(json)?;
    Ok(raw
        .into_iter()
        .map(|m| MonitorInfo {
            name: m.name,
            width: m.width,
            height: m.height,
            scale: m.scale,
        })
        .collect())
}

/// Get the minimum resolution across all monitors (for search filtering).
pub fn min_resolution(monitors: &[MonitorInfo]) -> Option<(u32, u32)> {
    if monitors.is_empty() {
        return None;
    }
    let min_w = monitors.iter().map(|m| m.width).min().unwrap();
    let min_h = monitors.iter().map(|m| m.height).min().unwrap();
    Some((min_w, min_h))
}

/// Get the primary (first) monitor's aspect ratio as a string like "16:9".
pub fn primary_aspect_ratio(monitors: &[MonitorInfo]) -> Option<String> {
    monitors.first().map(|m| {
        let g = gcd(m.width, m.height);
        format!("{}:{}", m.width / g, m.height / g)
    })
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

#[derive(Debug, Deserialize)]
struct HyprMonitor {
    name: String,
    width: u32,
    height: u32,
    scale: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOCK_MONITORS: &str = r#"[
        {
            "id": 0,
            "name": "DP-1",
            "description": "Dell U2720Q",
            "make": "Dell",
            "model": "U2720Q",
            "serial": "",
            "width": 3840,
            "height": 2160,
            "refreshRate": 60.0,
            "x": 0,
            "y": 0,
            "activeWorkspace": {"id": 1, "name": "1"},
            "specialWorkspace": {"id": 0, "name": ""},
            "reserved": [0, 0, 0, 0],
            "scale": 1.5,
            "transform": 0,
            "focused": true,
            "dpmsStatus": true,
            "vrr": false,
            "activelyTearing": false
        },
        {
            "id": 1,
            "name": "HDMI-A-1",
            "description": "LG 27GL850",
            "make": "LG",
            "model": "27GL850",
            "serial": "",
            "width": 2560,
            "height": 1440,
            "refreshRate": 144.0,
            "x": 3840,
            "y": 0,
            "activeWorkspace": {"id": 2, "name": "2"},
            "specialWorkspace": {"id": 0, "name": ""},
            "reserved": [0, 0, 0, 0],
            "scale": 1.0,
            "transform": 0,
            "focused": false,
            "dpmsStatus": true,
            "vrr": false,
            "activelyTearing": false
        }
    ]"#;

    #[test]
    fn test_parse_monitors() {
        let monitors = parse_monitors(MOCK_MONITORS).unwrap();
        assert_eq!(monitors.len(), 2);

        assert_eq!(monitors[0].name, "DP-1");
        assert_eq!(monitors[0].width, 3840);
        assert_eq!(monitors[0].height, 2160);
        assert_eq!(monitors[0].scale, 1.5);

        assert_eq!(monitors[1].name, "HDMI-A-1");
        assert_eq!(monitors[1].width, 2560);
        assert_eq!(monitors[1].height, 1440);
    }

    #[test]
    fn test_min_resolution() {
        let monitors = parse_monitors(MOCK_MONITORS).unwrap();
        let (w, h) = min_resolution(&monitors).unwrap();
        assert_eq!(w, 2560);
        assert_eq!(h, 1440);
    }

    #[test]
    fn test_min_resolution_empty() {
        assert!(min_resolution(&[]).is_none());
    }

    #[test]
    fn test_primary_aspect_ratio() {
        let monitors = parse_monitors(MOCK_MONITORS).unwrap();
        let ratio = primary_aspect_ratio(&monitors).unwrap();
        assert_eq!(ratio, "16:9");
    }

    #[test]
    fn test_gcd() {
        assert_eq!(gcd(3840, 2160), 240);
        assert_eq!(gcd(2560, 1440), 160);
        assert_eq!(gcd(1920, 1080), 120);
    }
}
