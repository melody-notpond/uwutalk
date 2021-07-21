use iced::{executor, Application, Clipboard, Command, Element, Length, widget::*};

pub struct Chat {
    entry_state: text_input::State,
    messages_state: scrollable::State,
}

impl Application for Chat {
    type Executor = executor::Default;
    type Message = ();
    type Flags = ();

    fn new(_flags: ()) -> (Chat, Command<Self::Message>) {
        (Chat {
            entry_state: text_input::State::new(),
            messages_state: scrollable::State::new()
        }, Command::none())
    }

    fn title(&self) -> String {
        String::from("A cool application")
    }

    fn update(&mut self, _message: Self::Message, _clipboard: &mut Clipboard) -> Command<Self::Message> {
        Command::none()
    }

    fn view(&mut self) -> Element<Self::Message> {
        let entry = TextInput::new(&mut self.entry_state, "Say hello!", "", |_| {})
            .width(Length::Fill);
        let messages = Scrollable::new(&mut self.messages_state)
            .width(Length::Fill)
            .height(Length::Fill);
        let right_column = Column::new()
            .push(messages)
            .spacing(20)
            .push(entry);
        right_column.into()
    }

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        iced::Subscription::none()
    }

    fn mode(&self) -> iced::window::Mode {
        iced::window::Mode::Windowed
    }

    fn background_color(&self) -> iced::Color {
        iced::Color::WHITE
    }

    fn scale_factor(&self) -> f64 {
        1.0
    }

    fn should_exit(&self) -> bool {
        false
    }
}
