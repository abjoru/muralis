use iced::widget::{button, column, container, pick_list, row, scrollable, text, text_input};
use iced::{Element, Length};

use muralis_core::config::Config;
use muralis_core::ipc::DaemonStatus;
use muralis_core::models::{BackendType, DisplayMode};

use crate::message::Message;

fn section_header(label: &str) -> text::Text<'_> {
    text(label)
        .size(14)
        .color(iced::Color::from_rgb8(0xa8, 0x99, 0x84))
}

fn label(s: &str) -> text::Text<'_> {
    text(s).size(13)
}

pub fn view<'a>(config: &Config, daemon_status: &Option<DaemonStatus>) -> Element<'a, Message> {
    let connected = daemon_status.is_some();

    // --- Daemon section ---
    let daemon_info: Element<'a, Message> = if let Some(status) = daemon_status {
        column![
            row![
                label("Status:"),
                text(if status.paused { "paused" } else { "running" }).size(13),
            ]
            .spacing(8),
            row![label("Mode:"), text(status.mode.to_string()).size(13),].spacing(8),
            row![
                label("Wallpapers:"),
                text(status.wallpaper_count.to_string()).size(13),
            ]
            .spacing(8),
        ]
        .spacing(4)
        .into()
    } else {
        text("Not connected — changes take effect on daemon start")
            .size(12)
            .color(iced::Color::from_rgb8(0x83, 0xa5, 0x98))
            .into()
    };

    let daemon_controls = row![
        button("Prev")
            .on_press_maybe(connected.then_some(Message::DaemonPrev))
            .padding([6, 16]),
        button(if daemon_status.as_ref().is_some_and(|s| s.paused) {
            "Resume"
        } else {
            "Pause"
        })
        .on_press_maybe(connected.then_some(Message::DaemonTogglePause))
        .padding([6, 16]),
        button("Next")
            .on_press_maybe(connected.then_some(Message::DaemonNext))
            .padding([6, 16]),
    ]
    .spacing(8);

    let daemon_section =
        column![section_header("Daemon"), daemon_info, daemon_controls,].spacing(8);

    // --- Display section ---
    let mode_row = row![
        label("Mode:"),
        pick_list(
            DisplayMode::ALL,
            Some(config.display.mode),
            Message::SettingsModeChanged
        )
        .width(Length::Fixed(180.0)),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    let shows_interval = matches!(
        config.display.mode,
        DisplayMode::Random | DisplayMode::Sequential
    );

    let mut display_col = column![section_header("Display"), mode_row].spacing(8);

    if shows_interval {
        let interval_row = row![
            label("Interval:"),
            text_input("e.g. 30m, 1h", &config.display.interval)
                .on_input(Message::SettingsIntervalChanged)
                .width(Length::Fixed(180.0)),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        display_col = display_col.push(interval_row);
    }

    let backend_row = row![
        label("Backend:"),
        pick_list(
            BackendType::ALL,
            Some(config.general.backend),
            Message::SettingsBackendChanged
        )
        .width(Length::Fixed(180.0)),
        text("(requires restart)")
            .size(11)
            .color(iced::Color::from_rgb8(0x83, 0xa5, 0x98)),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);
    display_col = display_col.push(backend_row);

    let content = column![daemon_section, display_col]
        .spacing(24)
        .padding(20)
        .max_width(600);

    container(scrollable(content))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
