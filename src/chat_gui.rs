use std::collections::VecDeque;
use std::hash::Hasher;

use iced::{Application, Clipboard, Command, Element, Length, Subscription, executor, widget::*};
use iced_futures::futures::stream::{self, BoxStream};
use iced_native::subscription::Recipe;
use tokio::sync::{mpsc, oneshot};
use reqwest::{Error, Response};

pub enum ClientMessage {
    SendMessage(String, oneshot::Sender<Result<Response, Error>>),
}

pub struct Chat {
    entry_text: String,
    entry_state: text_input::State,
    messages_state: scrollable::State,
    messages: VecDeque<String>,
    tx: mpsc::Sender<ClientMessage>,
    event_uid: u64,
}

#[derive(Debug, Clone)]
pub enum Message {
    None,
    InputChanged(String),
    Send,
    Dequeue
}

pub enum ListenForEvents {
    MessageSend(u64, String, mpsc::Sender<ClientMessage>)
}

enum SendMessageState {
    Starting(String, mpsc::Sender<ClientMessage>),
    Waiting(oneshot::Receiver<Result<Response, Error>>),
    Finished(Response),
    FinishedForRealsies,
}

impl<H, E> Recipe<H, E> for ListenForEvents
where H: Hasher {
    type Output = Message;

    fn hash(&self, state: &mut H) {
        use std::hash::Hash;

        match self {
            ListenForEvents::MessageSend(euid, msg, _) => {
                euid.hash(state);
                0.hash(state);
                msg.hash(state);
            }
        }
    }

    fn stream(
        self: Box<Self>,
        _input: BoxStream<E>,
    ) -> BoxStream<Self::Output> {
        match *self {
            ListenForEvents::MessageSend(_, msg, tx) => {
                Box::pin(stream::unfold(SendMessageState::Starting(msg, tx), async move |state| {
                    match state {
                        SendMessageState::Starting(msg, tx) => {
                            let (oneshot_tx, oneshot_rx) = oneshot::channel();
                            let message = ClientMessage::SendMessage(msg, oneshot_tx);
                            let _ = tx.send(message).await;
                            Some((Message::None, SendMessageState::Waiting(oneshot_rx)))
                        }

                        SendMessageState::Waiting(rx) => {
                            match rx.await {
                                Ok(Ok(v)) => Some((Message::None, SendMessageState::Finished(v))),
                                _ => None,
                            }
                        }

                        SendMessageState::Finished(v) => {
                            println!("{}", v.text().await.unwrap());
                            Some((Message::Dequeue, SendMessageState::FinishedForRealsies))
                        }

                        SendMessageState::FinishedForRealsies => None,
                    }
                }))
            }
        }
    }
}

impl Application for Chat {
    type Executor = executor::Default;
    type Message = Message;
    type Flags = mpsc::Sender<ClientMessage>;

    fn new(flags: Self::Flags) -> (Chat, Command<Self::Message>) {
        (Chat {
            entry_text: String::new(),
            entry_state: text_input::State::new(),
            messages_state: scrollable::State::new(),
            messages: VecDeque::new(),
            tx: flags,
            event_uid: 0,
        }, Command::none())
    }

    fn title(&self) -> String {
        String::from("uwutalk")
    }

    fn update(&mut self, message: Self::Message, _clipboard: &mut Clipboard) -> Command<Self::Message> {
        match message {
            Message::None => (),

            Message::InputChanged(input) => self.entry_text = input,
            Message::Send => {
                self.event_uid += 1;
                self.messages.push_back(self.entry_text.clone());
                self.entry_text = String::new();
            }

            Message::Dequeue => {
                self.messages.pop_front();
            }
        }

        Command::none()
    }

    fn view(&mut self) -> Element<Self::Message> {
        let entry = TextInput::new(&mut self.entry_state, "Say hello!", &self.entry_text, Message::InputChanged)
            .width(Length::Fill)
            .on_submit(Message::Send);
        let messages = Scrollable::new(&mut self.messages_state)
            .width(Length::Fill)
            .height(Length::Fill);
        let right_column = Column::new()
            .push(messages)
            .spacing(20)
            .push(entry);
        right_column.into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        if !self.messages.is_empty() {
            iced::Subscription::from_recipe(ListenForEvents::MessageSend(self.event_uid, self.messages.front().unwrap().clone(), self.tx.clone()))
        } else {
            Subscription::none()
        }
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
