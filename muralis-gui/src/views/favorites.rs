use std::collections::{HashMap, HashSet};

use iced::widget::image::Handle as ImageHandle;
use iced::widget::{button, column, container, row, scrollable, text, Image, Space};
use iced::{Element, Length};

use muralis_core::models::Wallpaper;

use crate::message::Message;

#[allow(clippy::too_many_arguments)]
pub fn view<'a>(
    wallpapers: &'a [Wallpaper],
    thumbnail_cache: &'a HashMap<String, ImageHandle>,
    selected_index: Option<usize>,
    _preview_handle: &'a Option<ImageHandle>,
    _preview_loading: bool,
    multi_selected: &'a HashSet<usize>,
    _crop_overlay_active: bool,
    _crop_overlay_handle: &'a Option<ImageHandle>,
    _crop_needed: bool,
) -> Element<'a, Message> {
    let filtered: Vec<(usize, &Wallpaper)> = wallpapers.iter().enumerate().collect();

    if filtered.is_empty() {
        return container(text(
            "No favorites yet. Search for wallpapers and add some!",
        ))
        .center(Length::Fill)
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
    }

    let cols = 5;
    let grid = scrollable(
        column(
            filtered
                .chunks(cols)
                .map(|chunk| {
                    row(chunk
                        .iter()
                        .map(|(orig_idx, wp)| {
                            let idx = *orig_idx;
                            let is_selected = selected_index == Some(idx);
                            let thumb: Element<'a, Message> =
                                if let Some(handle) = thumbnail_cache.get(&wp.id) {
                                    Image::new(handle.clone())
                                        .width(Length::Fill)
                                        .height(180)
                                        .content_fit(iced::ContentFit::Contain)
                                        .into()
                                } else {
                                    container(text("Loading..."))
                                        .width(Length::Fill)
                                        .height(180)
                                        .center(Length::Fill)
                                        .style(container::bordered_box)
                                        .into()
                                };

                            let is_multi = multi_selected.contains(&idx);
                            let style = if is_selected || is_multi {
                                button::secondary
                            } else {
                                button::text
                            };

                            container(
                                button(thumb)
                                    .on_press(Message::ThumbnailClicked(idx))
                                    .style(style)
                                    .padding(0),
                            )
                            .width(Length::FillPortion(1))
                            .into()
                        })
                        .collect::<Vec<Element<'a, Message>>>())
                    .spacing(8)
                    .into()
                })
                .collect::<Vec<Element<'a, Message>>>(),
        )
        .spacing(8)
        .padding(16),
    )
    .width(Length::Fill)
    .height(Length::Fill);

    let mut left_col: iced::widget::Column<'a, Message> = column![];

    if !multi_selected.is_empty() {
        let batch_bar = row![
            text(format!("{} selected", multi_selected.len())).size(14),
            Space::new().width(Length::Fill),
            button("Unfavorite All")
                .on_press(Message::BatchUnfavorite)
                .padding([4, 12]),
            button("Clear")
                .on_press(Message::ClearSelection)
                .padding([4, 12]),
        ]
        .spacing(8)
        .padding(8);
        left_col = left_col.push(batch_bar);
    }

    left_col = left_col.push(grid);

    left_col.width(Length::Fill).height(Length::Fill).into()
}

pub fn preview_content<'a>(
    wallpapers: &'a [Wallpaper],
    selected_index: Option<usize>,
    preview_handle: &'a Option<ImageHandle>,
    preview_loading: bool,
    crop_overlay_active: bool,
    crop_overlay_handle: &'a Option<ImageHandle>,
    crop_needed: bool,
) -> Option<Element<'a, Message>> {
    let sel_idx = selected_index?;
    let wp = wallpapers.get(sel_idx)?;

    let effective_handle = if crop_overlay_active && crop_needed {
        crop_overlay_handle.as_ref()
    } else {
        preview_handle.as_ref()
    };

    let preview_img: Element<'a, Message> = if let Some(handle) = effective_handle {
        Image::new(handle.clone())
            .width(Length::Fill)
            .content_fit(iced::ContentFit::Contain)
            .into()
    } else if preview_loading
        || (crop_overlay_active && crop_needed && crop_overlay_handle.is_none())
    {
        container(
            column![
                text("Loading preview...").size(18),
                text("Please wait while the image loads").size(12),
            ]
            .spacing(8)
            .align_x(iced::Alignment::Center),
        )
        .center(Length::Fill)
        .width(Length::Fill)
        .height(400)
        .style(container::bordered_box)
        .into()
    } else {
        container(text("Preview unavailable").size(14))
            .center(Length::Fill)
            .width(Length::Fill)
            .height(400)
            .into()
    };

    let mut header_row = row![
        text(format!("{}x{}", wp.width, wp.height)).size(13),
        Space::new().width(Length::Fill),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    if crop_needed {
        let label = if crop_overlay_active {
            "Show Full"
        } else {
            "Show Crop"
        };
        header_row = header_row.push(
            button(label)
                .on_press(Message::ToggleCropOverlay)
                .padding([4, 12]),
        );
    }

    header_row = header_row.push(
        button("Close")
            .on_press(Message::ClosePreview)
            .padding([4, 12]),
    );

    let mut info = column![text(format!("Source: {}", wp.source_type)).size(14),].spacing(4);

    if !wp.tags.is_empty() {
        info = info.push(text(format!("Tags: {}", wp.tags.join(", "))).size(14));
    }

    info = info.push(text(format!("Added: {}", wp.added_at)).size(14));

    if let Some(ref last) = wp.last_used {
        info = info.push(text(format!("Last used: {last}")).size(14));
    }

    info = info.push(text(format!("Used: {} times", wp.use_count)).size(14));

    if let Some(ref url) = wp.source_url {
        info = info.push(text(format!("URL: {url}")).size(12));
    }

    let actions = row![
        button("Apply").on_press(Message::Apply(wp.id.clone())),
        button("Unfavorite").on_press(Message::Unfavorite(wp.id.clone())),
    ]
    .spacing(8);

    Some(
        scrollable(
            column![header_row, preview_img, info, actions]
                .spacing(12)
                .width(Length::Fill),
        )
        .into(),
    )
}
