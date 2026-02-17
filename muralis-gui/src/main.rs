mod app;
mod message;
mod views;

use iced::Size;

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "muralis_gui=info".into()),
        )
        .init();

    iced::application(app::App::new, app::App::update, app::App::view)
        .title("Muralis")
        .theme(app::App::theme)
        .subscription(app::App::subscription)
        .window_size(Size::new(1200.0, 800.0))
        .run()
}
