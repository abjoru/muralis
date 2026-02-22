use std::collections::{HashMap, HashSet};

use iced::widget::image::Handle as ImageHandle;
use iced::widget::{
    button, column, container, pick_list, row, scrollable, slider, text, text_input, Image, Space,
};
use iced::{Element, Length};

use muralis_core::models::WallpaperPreview;

use crate::message::{AspectRatioFilter, Message};

#[allow(clippy::too_many_arguments)]
pub fn view<'a>(
    search_query: &'a str,
    results: &'a [WallpaperPreview],
    thumbnail_cache: &'a HashMap<String, ImageHandle>,
    selected_index: Option<usize>,
    search_loading: bool,
    current_page: u32,
    page_input_str: &'a str,
    multi_selected: &'a HashSet<usize>,
    aspect_filter: AspectRatioFilter,
    is_feed: bool,
    thumbnail_zoom: f32,
) -> Element<'a, Message> {
    let search_bar = row![
        text_input("Search wallpapers...", search_query)
            .on_input(Message::SearchQueryChanged)
            .on_submit(Message::SearchSubmit)
            .width(Length::Fill),
        pick_list(
            AspectRatioFilter::ALL,
            Some(aspect_filter),
            Message::AspectFilterChanged,
        )
        .width(Length::Shrink),
        button(if search_loading {
            "Searching..."
        } else {
            "Search"
        })
        .on_press_maybe(if search_loading {
            None
        } else {
            Some(Message::SearchSubmit)
        }),
    ]
    .spacing(8)
    .padding([12, 8]);

    let filtered: Vec<(usize, &WallpaperPreview)> = results
        .iter()
        .enumerate()
        .filter(|(_, p)| aspect_filter.matches(p.width, p.height))
        .collect();

    let content_area: Element<'a, Message> = if search_loading {
        container(text("Searching..."))
            .center(Length::Fill)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else if filtered.is_empty() {
        let msg = if !results.is_empty() {
            "No results match the selected aspect ratio"
        } else if is_feed {
            "No wallpapers found"
        } else {
            "Enter a search query to find wallpapers"
        };
        container(text(msg))
            .center(Length::Fill)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        let thumb_w: f32 = 220.0 * thumbnail_zoom;
        let aspect_height = aspect_filter.ratio_value().map(|r| thumb_w / r as f32);
        let cells: Vec<Element<'a, Message>> = filtered
            .iter()
            .map(|(orig_idx, preview)| {
                let idx = *orig_idx;
                let is_selected = selected_index == Some(idx);

                let thumb: Element<'a, Message> =
                    if let Some(handle) = thumbnail_cache.get(&preview.source_id) {
                        let mut img = Image::new(handle.clone())
                            .width(thumb_w)
                            .content_fit(iced::ContentFit::Contain);
                        if let Some(h) = aspect_height {
                            img = img.height(h);
                        }
                        img.into()
                    } else {
                        container(text("Loading..."))
                            .width(thumb_w)
                            .height(aspect_height.unwrap_or(thumb_w * 9.0 / 16.0))
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

                button(thumb)
                    .on_press(Message::ThumbnailClicked(idx))
                    .style(style)
                    .padding(0)
                    .into()
            })
            .collect();

        let grid =
            scrollable(container(row(cells).spacing(8).wrap().vertical_spacing(8)).padding(16))
                .width(Length::Fill)
                .height(Length::Fill);

        grid.into()
    };

    let has_next = !results.is_empty();

    let bottom_bar = if is_feed {
        row![
            text("Zoom").size(12),
            slider(0.5..=2.5, thumbnail_zoom, Message::ZoomChanged)
                .step(0.1)
                .width(80),
            Space::new().width(Length::Fill),
        ]
        .spacing(8)
        .padding([6, 16])
        .align_y(iced::Alignment::Center)
    } else {
        let page_input = text_input("pg", page_input_str)
            .on_input(Message::PageInputChanged)
            .on_submit(Message::PageInputSubmit)
            .width(38);

        row![
            text("Zoom").size(12),
            slider(0.5..=2.5, thumbnail_zoom, Message::ZoomChanged)
                .step(0.1)
                .width(80),
            Space::new().width(Length::Fill),
            button("< Prev").on_press_maybe(if current_page > 1 {
                Some(Message::PrevPage)
            } else {
                None
            }),
            page_input,
            button("Next >").on_press_maybe(if has_next {
                Some(Message::NextPage)
            } else {
                None
            }),
        ]
        .spacing(8)
        .padding([6, 16])
        .align_y(iced::Alignment::Center)
    };

    let mut left = column![];

    if !is_feed {
        left = left.push(search_bar);
    }

    if !multi_selected.is_empty() {
        let batch_bar = row![
            text(format!("{} selected", multi_selected.len())).size(14),
            Space::new().width(Length::Fill),
            button("Favorite All")
                .on_press(Message::BatchFavorite)
                .padding([4, 12]),
            button("Blacklist All")
                .on_press(Message::BatchBlacklist)
                .padding([4, 12]),
            button("Clear")
                .on_press(Message::ClearSelection)
                .padding([4, 12]),
        ]
        .spacing(8)
        .padding(8);
        left = left.push(batch_bar);
    }

    left = left.push(content_area);
    left = left.push(bottom_bar);

    left.width(Length::Fill).into()
}

pub fn preview_content<'a>(
    results: &'a [WallpaperPreview],
    selected_index: Option<usize>,
    preview_handle: &'a Option<ImageHandle>,
    preview_loading: bool,
    crop_overlay_active: bool,
    crop_overlay_handle: &'a Option<ImageHandle>,
    crop_needed: bool,
) -> Option<Element<'a, Message>> {
    let sel_idx = selected_index?;
    let preview = results.get(sel_idx)?;

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
                text("Please wait while the full image downloads").size(12),
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
        text(format!("{}x{}", preview.width, preview.height)).size(13),
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

    let mut info = column![text(format!("Source: {}", preview.source_type)).size(14),].spacing(4);

    if !preview.tags.is_empty() {
        info = info.push(text(format!("Tags: {}", preview.tags.join(", "))).size(14));
    }

    info = info.push(text(format!("URL: {}", preview.source_url)).size(12));

    let actions = row![
        button("Favorite").on_press(Message::Favorite(preview.clone())),
        button("Blacklist").on_press(Message::Blacklist(preview.clone())),
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
