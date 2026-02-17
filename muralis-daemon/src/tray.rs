use ksni::blocking::TrayMethods;
use tokio::sync::mpsc;
use tracing::{info, warn};

use muralis_core::models::DisplayMode;

use crate::display::DaemonCommand;

struct MuralisTray {
    cmd_tx: mpsc::Sender<DaemonCommand>,
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
        let _ = Command::new("muralis-gui").spawn();
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
    std::thread::spawn(move || {
        let tray = MuralisTray { cmd_tx };
        match tray.spawn() {
            Ok(_handle) => {
                info!("tray spawned");
                // keep thread alive to maintain tray
                loop {
                    std::thread::park();
                }
            }
            Err(e) => warn!("tray error: {e}"),
        }
    });
}
