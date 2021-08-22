use std::collections::{HashMap, VecDeque};
use std::hash::Hasher;

use iced::{Application, Clipboard, Command, Element, Length, Subscription, executor, widget::*};
use iced_futures::futures::stream::{self, BoxStream};
use iced_native::subscription::Recipe;
use tokio::sync::{mpsc, oneshot};
use reqwest::Error;
use uwuifier::uwuify_str_sse;

use super::chat::{Event, SyncState, RoomEvent};
use super::markdown;

pub enum ClientMessage {
    SendMessage(String, String, String, oneshot::Sender<Result<Event, Error>>),
    ClientSync(String, String, oneshot::Sender<Result<SyncState, Error>>),
}

struct Channel {
    id: String,
    name: String,
    button: button::State,
    messages: Vec<RoomEvent>,
}

pub struct Chat {
    entry_text: String,
    entry_state: text_input::State,
    messages_state: scrollable::State,
    channels_state: scrollable::State,
    messages_queue: VecDeque<(String, String)>,
    current_channel: String,
    channels: Vec<String>,
    channels_hashed: HashMap<String, Channel>,
    tx: mpsc::Sender<ClientMessage>,
    event_uid: u64,
    next_batch: String,
}

impl Chat {
    fn new(tx: mpsc::Sender<ClientMessage>) -> Chat {
        Chat {
            entry_text: String::new(),
            entry_state: text_input::State::new(),
            messages_state: scrollable::State::new(),
            channels_state: scrollable::State::new(),
            messages_queue: VecDeque::new(),
            current_channel: String::new(),
            channels: Vec::new(),
            channels_hashed: HashMap::new(),
            tx,
            event_uid: 0,
            next_batch: String::new()
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    None,
    InputChanged(String),
    Send,
    Dequeue,
    NewSync(SyncState),
    ChannelChanged(String)
}

pub enum ListenForEvents {
    MessageSend(u64, String, (String, String), mpsc::Sender<ClientMessage>),
    ClientSync(u64, String, String, mpsc::Sender<ClientMessage>)
}

enum SendMessageState {
    Starting(String, String, String, mpsc::Sender<ClientMessage>),
    Waiting(oneshot::Receiver<Result<Event, Error>>),
    Finished(Event),
    FinishedForRealsies,
}

enum SyncClientState {
    Starting(String, String, mpsc::Sender<ClientMessage>),
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
            ListenForEvents::MessageSend(euid, room_id, msg, _) => {
                euid.hash(state);
                room_id.hash(state);
                msg.hash(state);
            }

            ListenForEvents::ClientSync(euid, next_batch, filter, _) => {
                euid.hash(state);
                next_batch.hash(state);
                filter.hash(state);
            }
        }
    }

    fn stream(
        self: Box<Self>,
        _input: BoxStream<E>,
    ) -> BoxStream<Self::Output> {
        match *self {
            ListenForEvents::MessageSend(_, room_id, (msg, formatted), tx) => {
                Box::pin(stream::unfold(SendMessageState::Starting(room_id, msg, formatted, tx), async move |state| {
                    match state {
                        SendMessageState::Starting(room_id, msg, formatted, tx) => {
                            let (oneshot_tx, oneshot_rx) = oneshot::channel();
                            let message = ClientMessage::SendMessage(room_id, msg, formatted, oneshot_tx);
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

            ListenForEvents::ClientSync(_, next_batch, filter, tx) => {
                Box::pin(stream::unfold(SyncClientState::Starting(next_batch, filter, tx), async move |state| {
                    match state {
                        SyncClientState::Starting(next_batch, filter, tx) => {
                            let (oneshot_tx, oneshot_rx) = oneshot::channel();
                            let message = ClientMessage::ClientSync(next_batch, filter, oneshot_tx);
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
        (Chat::new(flags), Command::none())
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
                    let entry = if let Some(v) = self.entry_text.strip_prefix("/uwu ") {
                        uwuify_str_sse(v)
                    } else {
                        self.entry_text.clone()
                    };
                    let markdown = markdown::parse_markdown(&entry);
                    let html = markdown::markdown_to_html(markdown);
                    self.messages_queue.push_back((entry, html));

                    self.entry_text = String::new();
                }
            }

            Message::Dequeue => {
                self.messages_queue.pop_front();
            }

            Message::NewSync(state) => {
                self.event_uid += 1;
                self.next_batch = state.next_batch;

                for (id, joined) in state.rooms.join {
                    if !self.channels_hashed.contains_key(&id) {
                        self.channels_hashed.insert(id.clone(), Channel {
                            id: id.clone(),
                            name: match joined.name {
                                Some(v) => v,
                                None => String::from("<unnamed room>")
                            },
                            button: button::State::new(),
                            messages: joined.timeline.events,
                        });
                        self.channels.push(id);
                    } else {
                        self.channels_hashed.get_mut(&id).unwrap().messages.extend(joined.timeline.events);
                    }
                }
            }

            Message::ChannelChanged(id) => {
                self.current_channel = id;
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
            .height(Length::Fill)
            .spacing(10);

        if let Some(channel) = self.channels_hashed.get(&self.current_channel) {
            for message in channel.messages.iter() {
                let content = match message.content.get("body") {
                    Some(v) => v.as_str().unwrap(),
                    None => continue,
                };
                let avatar = Image::new("avatar.jpg").width(Length::Units(50)).height(Length::Units(50));
                let display_name = Text::new(message.sender.clone());
                let message = Text::new(content);
                let column = Column::new()
                    .push(display_name)
                    .push(message);
                let row = Row::new()
                    .push(Container::new(avatar))
                    .push(column)
                    .spacing(5);
                messages = messages.push(row);
            }
        }

        let mut channels = Scrollable::new(&mut self.channels_state)
            .width(Length::Units(200))
            .height(Length::Fill);

        for (id, channel) in self.channels_hashed.iter_mut() {
            let name = Text::new(&channel.name);
            let button = Button::new(&mut channel.button, name)
                .on_press(Message::ChannelChanged(id.clone()));
            channels = channels.push(button);
        }

        let mut right_column = Column::new()
            .push(messages)
            .spacing(20);

        if !self.current_channel.is_empty() {
            right_column = right_column.push(entry);
        }

        let row = Row::new()
            .push(channels)
            .spacing(10)
            .push(right_column);

        let container = Container::new(row)
            .padding(5)
            .width(Length::Fill)
            .height(Length::Fill);
        container.into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        if !self.messages_queue.is_empty() {
            iced::Subscription::from_recipe(ListenForEvents::MessageSend(self.event_uid, self.current_channel.clone(), self.messages_queue.front().unwrap().clone(), self.tx.clone()))
        } else {
            Subscription::from_recipe(ListenForEvents::ClientSync(self.event_uid, self.next_batch.clone(), String::from(r#"{"room": {"timeline": {"limit": 50, "types": ["m.room.message"]}}}"#), self.tx.clone()))
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
