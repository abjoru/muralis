use std::time::Duration;

use ksni::blocking::TrayMethods;
use tokio::sync::mpsc;
use tracing::{info, warn};

use muralis_core::models::DisplayMode;

use crate::display::DaemonCommand;

const TRAY_STARTUP_DELAY: Duration = Duration::from_secs(3);
const TRAY_RETRIES: u32 = 3;
const TRAY_RETRY_DELAY: Duration = Duration::from_secs(2);

struct MuralisTray {
    cmd_tx: mpsc::Sender<DaemonCommand>,
    gui: Option<std::process::Child>,
    icon_theme_path: String,
}

impl MuralisTray {
    fn send(&self, cmd: DaemonCommand) {
        let _ = self.cmd_tx.try_send(cmd);
    }
}

impl ksni::Tray for MuralisTray {
    fn id(&self) -> String {
        "muralis".into()
    }

    fn icon_name(&self) -> String {
        "muralis".into()
    }

    fn icon_theme_path(&self) -> String {
        self.icon_theme_path.clone()
    }

    fn title(&self) -> String {
        "Muralis".into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "Muralis - Wallpaper Manager".into(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        use std::process::Command;
        if let Some(child) = self.gui.as_mut() {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
                let _ = child.wait();
                self.gui = None;
                return;
            }
        }
        self.gui = Command::new("muralis-gui").spawn().ok();
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;

        vec![
            StandardItem {
                label: "Next".into(),
                activate: Box::new(|tray: &mut Self| tray.send(DaemonCommand::Next)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Previous".into(),
                activate: Box::new(|tray: &mut Self| tray.send(DaemonCommand::Prev)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            SubMenu {
                label: "Mode".into(),
                submenu: vec![
                    StandardItem {
                        label: "Static".into(),
                        activate: Box::new(|tray: &mut Self| {
                            tray.send(DaemonCommand::SetMode {
                                mode: DisplayMode::Static,
                            });
                        }),
                        ..Default::default()
                    }
                    .into(),
                    StandardItem {
                        label: "Random".into(),
                        activate: Box::new(|tray: &mut Self| {
                            tray.send(DaemonCommand::SetMode {
                                mode: DisplayMode::Random,
                            });
                        }),
                        ..Default::default()
                    }
                    .into(),
                    StandardItem {
                        label: "Random (Startup)".into(),
                        activate: Box::new(|tray: &mut Self| {
                            tray.send(DaemonCommand::SetMode {
                                mode: DisplayMode::RandomStartup,
                            });
                        }),
                        ..Default::default()
                    }
                    .into(),
                    StandardItem {
                        label: "Sequential".into(),
                        activate: Box::new(|tray: &mut Self| {
                            tray.send(DaemonCommand::SetMode {
                                mode: DisplayMode::Sequential,
                            });
                        }),
                        ..Default::default()
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Pause".into(),
                activate: Box::new(|tray: &mut Self| tray.send(DaemonCommand::Pause)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|tray: &mut Self| tray.send(DaemonCommand::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub fn spawn_tray(cmd_tx: mpsc::Sender<DaemonCommand>) {
    let icon_theme_path = std::env::var("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_default();
            std::path::PathBuf::from(home).join(".local/share")
        })
        .join("icons")
        .to_string_lossy()
        .into_owned();

    std::thread::spawn(move || {
        // delay to let SNI host (waybar/ags) initialize first
        std::thread::sleep(TRAY_STARTUP_DELAY);

        for attempt in 1..=TRAY_RETRIES {
            let tray = MuralisTray {
                cmd_tx: cmd_tx.clone(),
                gui: None,
                icon_theme_path: icon_theme_path.clone(),
            };
            match tray.spawn() {
                Ok(_handle) => {
                    info!("tray spawned");
                    loop {
                        std::thread::park();
                    }
                }
                Err(e) if attempt < TRAY_RETRIES => {
                    warn!("tray attempt {attempt}/{TRAY_RETRIES} failed: {e}");
                    std::thread::sleep(TRAY_RETRY_DELAY);
                }
                Err(e) => warn!("tray failed after {TRAY_RETRIES} attempts: {e}"),
            }
        }
    });
}
