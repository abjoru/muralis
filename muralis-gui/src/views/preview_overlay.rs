use iced::widget::{container, mouse_area, stack, Space};
use iced::{Background, Border, Color, Element, Length, Theme};

use crate::message::Message;

fn scrim_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.6))),
        ..Default::default()
    }
}

fn panel_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(palette.background.base.color)),
        border: Border {
            color: palette.background.strong.color,
            width: 1.0,
            radius: 12.0.into(),
        },
        ..Default::default()
    }
}

pub fn wrap_with_overlay<'a>(
    base: Element<'a, Message>,
    preview_content: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let Some(content) = preview_content else {
        return base;
    };

    let scrim = mouse_area(
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(scrim_style),
    )
    .on_press(Message::ClosePreview);

    let panel = container(
        container(content)
            .width(900)
            .height(700)
            .padding(20)
            .style(panel_style),
    )
    .center(Length::Fill)
    .width(Length::Fill)
    .height(Length::Fill);

    stack![base, scrim, panel].into()
}
