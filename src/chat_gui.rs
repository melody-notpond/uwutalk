use std::collections::VecDeque;
use std::hash::Hasher;

use iced::{Application, Clipboard, Command, Element, Length, Subscription, executor, widget::*};
use iced_futures::futures::stream::{self, BoxStream};
use iced_native::subscription::Recipe;
use tokio::sync::{mpsc, oneshot};
use reqwest::Error;

use super::chat::{Event, SyncState};

pub enum ClientMessage {
    SendMessage(String, oneshot::Sender<Result<Event, Error>>),
    ClientSync(String, oneshot::Sender<Result<SyncState, Error>>),
}

pub struct Chat {
    entry_text: String,
    entry_state: text_input::State,
    messages_state: scrollable::State,
    messages_queue: VecDeque<String>,
    tx: mpsc::Sender<ClientMessage>,
    event_uid: u64,
    next_batch: String,
}

#[derive(Debug, Clone)]
pub enum Message {
    None,
    InputChanged(String),
    Send,
    Dequeue,
    NewSync(SyncState)
}

pub enum ListenForEvents {
    MessageSend(u64, String, mpsc::Sender<ClientMessage>),
    ClientSync(u64, String, mpsc::Sender<ClientMessage>)
}

enum SendMessageState {
    Starting(String, mpsc::Sender<ClientMessage>),
    Waiting(oneshot::Receiver<Result<Event, Error>>),
    Finished(Event),
    FinishedForRealsies,
}

enum SyncClientState {
    Starting(String, mpsc::Sender<ClientMessage>),
    Waiting(oneshot::Receiver<Result<SyncState, Error>>),
    Finished(SyncState),
    FinishedForRealsies

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

            ListenForEvents::ClientSync(euid, next_batch, _) => {
                euid.hash(state);
                next_batch.hash(state);
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

                        SendMessageState::Finished(_) => {
                            Some((Message::Dequeue, SendMessageState::FinishedForRealsies))
                        }

                        SendMessageState::FinishedForRealsies => None,
                    }
                }))
            }

            ListenForEvents::ClientSync(_, next_batch, tx) => {
                Box::pin(stream::unfold(SyncClientState::Starting(next_batch, tx), async move |state| {
                    match state {
                        SyncClientState::Starting(next_batch, tx) => {
                            let (oneshot_tx, oneshot_rx) = oneshot::channel();
                            let message = ClientMessage::ClientSync(next_batch, oneshot_tx);
                            let _ = tx.send(message).await;
                            Some((Message::None, SyncClientState::Waiting(oneshot_rx)))
                        }

                        SyncClientState::Waiting(rx) => {
                            match rx.await {
                                Ok(Ok(v)) => Some((Message::None, SyncClientState::Finished(v))),

                                // TODO: try sending the message again
                                _ => None
                            }
                        }

                        SyncClientState::Finished(v) => {
                            Some((Message::NewSync(v), SyncClientState::FinishedForRealsies))
                        }

                        SyncClientState::FinishedForRealsies => None,
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
            messages_queue: VecDeque::new(),
            tx: flags,
            event_uid: 0,
            next_batch: String::new()
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
                if !self.entry_text.is_empty() {
                    self.event_uid += 1;
                    self.messages_queue.push_back(self.entry_text.clone());
                    self.entry_text = String::new();
                }
            }

            Message::Dequeue => {
                self.messages_queue.pop_front();
            }

            Message::NewSync(state) => {
                self.event_uid += 1;
                self.next_batch = state.next_batch;
            }
        }

        Command::none()
    }

    fn view(&mut self) -> Element<Self::Message> {
        let entry = TextInput::new(&mut self.entry_state, "Say hello!", &self.entry_text, Message::InputChanged)
            .width(Length::Fill)
            .on_submit(Message::Send);
        let mut messages = Scrollable::new(&mut self.messages_state)
            .width(Length::Fill)
            .height(Length::Fill);

        let messages_raw = vec![(String::from("avatar.jpg"), String::from("test@example.com"), String::from("this is my example message"))];
        for message in messages_raw {
            let avatar = Image::new(message.0).width(Length::Units(50)).height(Length::Units(50));
            let display_name = Text::new(message.1);
            let message = Text::new(message.2);
            let column = Column::new()
                .push(display_name)
                .push(message);
            let row = Row::new()
                .push(Container::new(avatar))
                .push(column);
            messages = messages.push(row);
        }

        let right_column = Column::new()
            .push(messages)
            .spacing(20)
            .push(entry);
        right_column.into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        if !self.messages_queue.is_empty() {
            iced::Subscription::from_recipe(ListenForEvents::MessageSend(self.event_uid, self.messages_queue.front().unwrap().clone(), self.tx.clone()))
        } else {
            Subscription::from_recipe(ListenForEvents::ClientSync(self.event_uid, self.next_batch.clone(), self.tx.clone()))
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
