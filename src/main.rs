use iced::{Application, Settings};

use uwutalk::chat_gui::Chat;

pub fn main() -> iced::Result {
    Chat::run(Settings {
        window: iced::window::Settings {
            size: (800, 600),
            min_size: Some((400, 300)),
            max_size: None,
            resizable: true,
            decorations: true,
            transparent: false,
            always_on_top: false,
            icon: None,
        },
        flags: (),
        default_font: None,
        default_text_size: 16,
        exit_on_close_request: true,
        antialiasing: true,
    })
}
