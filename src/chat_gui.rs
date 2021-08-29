use std::sync::Arc;

use druid::keyboard_types::Key;
use druid::text::{Attribute, RichText};
use druid::widget::{CrossAxisAlignment, FlexParams, LineBreaking, ListIter};
use druid::{Color, Data, Env, Event as DruidEvent, EventCtx, FontStyle, FontWeight, ImageBuf, Lens, LensExt, Selector, TextAlignment, UnitPoint, Widget, WidgetExt, widget};
use druid::im::{HashMap, Vector};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
use html_parser::{Dom, ElementVariant, Node};
// use uwuifier::uwuify_str_sse;

use super::chat::{RoomEvent, SyncState};
use super::markdown;

pub const SYNC: Selector<SyncState> = Selector::new("uwutalk.matrix.sync");

pub enum ClientMessage {
    SendMessage(String, String, String),
    ClientSync(String, String),
}

#[derive(Data, Clone, Lens)]
struct Channel {
    id: Arc<String>,
    name: Arc<String>,
    messages: Vector<Message>,
}

#[derive(Data, Clone, Lens)]
struct Message {
    sender: Arc<String>,
    avatar: Arc<ImageBuf>,
    event_id: Arc<String>,
    contents: Arc<String>,
    formatted: RichText,
}

#[derive(Data, Clone, Lens)]
pub struct Chat {
    editing_message: Arc<String>,
    channels_hashed: HashMap<Arc<String>, Channel>,
    channels: Vector<Arc<String>>,
    current_channel: Arc<String>,

    #[data(ignore)]
    tx: mpsc::Sender<ClientMessage>,
}

impl Chat {
    pub fn new(tx: mpsc::Sender<ClientMessage>) -> Chat {
        Chat {
            editing_message: Arc::new(String::new()),
            channels_hashed: HashMap::new(),
            channels: Vector::new(),
            current_channel: Arc::new(String::new()),
            tx
        }
    }
}

struct CurrentChannel {
    channels_hashed: HashMap<Arc<String>, Channel>,
    current_channel: Arc<String>,
}

struct CurrentChannelLens;

impl Lens<Chat, CurrentChannel> for CurrentChannelLens {
    fn with<V, F: FnOnce(&CurrentChannel) -> V>(&self, data: &Chat, f: F) -> V {
        let current = CurrentChannel {
            channels_hashed: data.channels_hashed.clone(),
            current_channel: data.current_channel.clone(),
        };
        f(&current)
    }

    fn with_mut<V, F: FnOnce(&mut CurrentChannel) -> V>(&self, data: &mut Chat, f: F) -> V {
        let mut current = CurrentChannel {
            channels_hashed: data.channels_hashed.clone(),
            current_channel: data.current_channel.clone(),
        };
        let v = f(&mut current);
        data.channels_hashed = current.channels_hashed;
        data.current_channel = current.current_channel;
        v
    }
}

#[derive(Data, Clone)]
struct AllChannels {
    channels_hashed: HashMap<Arc<String>, Channel>,
    channels: Vector<Arc<String>>,
    current_channel: Arc<String>,
}

struct AllChannelsLens;

impl Lens<Chat, AllChannels> for AllChannelsLens {
    fn with<V, F: FnOnce(&AllChannels) -> V>(&self, data: &Chat, f: F) -> V {
        let all = AllChannels {
            channels_hashed: data.channels_hashed.clone(),
            channels: data.channels.clone(),
            current_channel: data.current_channel.clone(),
        };
        f(&all)
    }

    fn with_mut<V, F: FnOnce(&mut AllChannels) -> V>(&self, data: &mut Chat, f: F) -> V {
        let mut all = AllChannels {
            channels_hashed: data.channels_hashed.clone(),
            channels: data.channels.clone(),
            current_channel: data.current_channel.clone(),
        };
        let v = f(&mut all);
        data.channels_hashed = all.channels_hashed;
        data.channels = all.channels;
        data.current_channel = all.current_channel;
        v
    }
}

impl ListIter<(Arc<String>, Channel)> for AllChannels {
    fn for_each(&self, mut cb: impl FnMut(&(Arc<String>, Channel), usize)) {
        for (i, channel) in self.channels.iter().enumerate() {
            let val = (self.current_channel.clone(), self.channels_hashed.get(channel).unwrap().clone());
            cb(&val, i);
        }
    }

    fn for_each_mut(&mut self, mut cb: impl FnMut(&mut (Arc<String>, Channel), usize)) {
        for (i, channel) in self.channels.iter().enumerate() {
            let mut val = (self.current_channel.clone(), self.channels_hashed.get(channel).unwrap().clone());
            cb(&mut val, i);
            self.current_channel = val.0;
            *self.channels_hashed.get_mut(channel).unwrap() = val.1;
        }
    }

    fn data_len(&self) -> usize {
        self.channels.len()
    }
}

struct Element {
    id: Option<String>,
    name: String,
    is_void: bool,
    attributes: std::collections::HashMap<String, Option<String>>,
    classes: Vec<String>,
}

fn extract_text_and_text_attributes_from_dom(node: &Node, buffer: &mut String, attrs: &mut Vec<((usize, usize), Element)>) {
    match node {
        Node::Text(t) => buffer.push_str(t),

        Node::Element(e) => {
            let index = attrs.len();
            attrs.push(((buffer.len(), 0), Element {
                id: e.id.clone(),
                name: e.name.clone(),
                is_void: matches!(e.variant, ElementVariant::Void),
                attributes: e.attributes.clone(),
                classes: e.classes.clone(),
            }));
            for child in e.children.iter() {
                extract_text_and_text_attributes_from_dom(child, buffer, attrs);
            }
            attrs[index].0.1 = buffer.len();
        }

        Node::Comment(_) => (),
    }
}

fn make_message(event: &RoomEvent) -> Message {
    let mut attrs = vec![];
    let formatted: Arc<str> = match event.content.get("formatted_body") {
        Some(v) => {
            let dom = Dom::parse(v.as_str().unwrap()).unwrap();
            let mut result = String::new();
            for child in dom.children.iter() {
                extract_text_and_text_attributes_from_dom(child, &mut result, &mut attrs);
            }
            Arc::from(result)
        }

        None => Arc::from(event.content.get("body").map(|v| v.as_str().unwrap_or("")).unwrap_or("")),
    };

    let mut formatted = RichText::new(formatted);
    for ((start, end), attr) in attrs {
        let range = start..end;
        match attr.name.as_str() {
            "em" => {
                formatted.add_attribute(range, Attribute::Style(FontStyle::Italic));
            }

            "strong" => {
                formatted.add_attribute(range, Attribute::Weight(FontWeight::new(700)));
            }

            "u" => {
                formatted.add_attribute(range, Attribute::Underline(true));
            }

            _ => (),
        }
    }

    Message {
        sender: Arc::new(event.sender.clone()),
        avatar: Arc::new(ImageBuf::empty()),
        event_id: Arc::new(event.event_id.clone()),
        contents: match event.content.get("body") {
            Some(v) => Arc::new(String::from(v.as_str().unwrap())),
            None => Arc::new(String::new()),
        },
        formatted,
    }
}

struct ChatController;

impl<W> widget::Controller<Chat, W> for ChatController
    where W: widget::Widget<Chat>
{
    fn event(&mut self, child: &mut W, ctx: &mut EventCtx, event: &DruidEvent, data: &mut Chat, env: &Env) {
        match event {
            DruidEvent::WindowConnected => {
                match data.tx.try_send(ClientMessage::ClientSync(String::new(), String::from(r#"{"room": {"timeline": {"limit": 50, "types": ["m.room.message"]}}}"#))) {
                    Ok(_) => (),
                    Err(TrySendError::Full(_)) => panic!("idk what to do here :("),
                    Err(TrySendError::Closed(_)) => panic!("oh no"),
                }

                child.event(ctx, event, data, env)
            }

            DruidEvent::Command(cmd) => {
                if let Some(sync) = cmd.get(SYNC) {
                    if let Some(rooms) = &sync.rooms {
                        if let Some(join) = &rooms.join {
                            for (id, joined) in join.iter() {
                                if !data.channels_hashed.contains_key(id) {
                                    data.channels_hashed.insert(Arc::new(id.clone()), Channel {
                                        id: Arc::new(id.clone()),
                                        name: Arc::new(match &joined.name {
                                            Some(v) => v.clone(),
                                            None => String::from("<unnamed room>")
                                        }),
                                        messages: joined.timeline.events.iter().map(make_message).collect(),
                                    });
                                    data.channels.push_back(Arc::new(id.clone()));
                                } else {
                                    data.channels_hashed.get_mut(id).unwrap().messages.extend(joined.timeline.events.iter().map(make_message));
                                }
                            }
                        }
                    }

                    match data.tx.try_send(ClientMessage::ClientSync(sync.next_batch.clone(), String::from(r#"{"room": {"timeline": {"limit": 50, "types": ["m.room.message"]}}}"#))) {
                        Ok(_) => (),
                        Err(TrySendError::Full(_)) => panic!("idk what to do here :("),
                        Err(TrySendError::Closed(_)) => panic!("oh no"),
                    }
                } else {
                    child.event(ctx, event, data, env)
                }
            }

            _ => child.event(ctx, event, data, env),
        }
    }
}

struct MessageEntryController;

impl<W> widget::Controller<Chat, W> for MessageEntryController
    where W: Widget<Chat>
{
    fn event(&mut self, child: &mut W, ctx: &mut EventCtx, event: &DruidEvent, data: &mut Chat, env: &Env) {
        match event {
            DruidEvent::KeyDown(key) if key.key == Key::Enter && !key.mods.shift() => {
                if !data.editing_message.is_empty() {
                    let count = data.editing_message.match_indices("```").count();
                    if count % 2 == 0 {
                        let formatted = markdown::parse_markdown(&*data.editing_message);
                        let formatted = markdown::markdown_to_html(formatted);
                        match data.tx.try_send(ClientMessage::SendMessage((*data.current_channel).clone(), (*data.editing_message).clone(), formatted)) {
                            Ok(_) => (),
                            Err(TrySendError::Full(_)) => panic!("idk what to do here :("),
                            Err(TrySendError::Closed(_)) => panic!("oh no"),
                        }
                        Arc::make_mut(&mut data.editing_message).clear();
                    } else {
                        child.event(ctx, event, data, env);
                    }
                }
            }

            _ => child.event(ctx, event, data, env),
        }
    }
}

fn create_channel_listing() -> impl Widget<(Arc<String>, Channel)> {
    widget::Button::dynamic(|data: &(Arc<String>, Channel), _| (*data.1.name).clone())
        .on_click(|_, (current_channel, channel), _| *current_channel = channel.id.clone())
}

fn create_message() -> impl Widget<Message> {
    let contents = widget::RawLabel::new()
        .with_text_alignment(TextAlignment::Start)
        .with_line_break_mode(LineBreaking::WordWrap)
        .lens(Message::formatted);
    let sender = widget::Label::dynamic(|v: &Message, _| (*v.sender).clone())
        .with_text_alignment(TextAlignment::Start);
    let mut column = widget::Flex::column()
        .with_child(sender)
        .with_spacer(2.0)
        .with_child(contents);
    column.set_cross_axis_alignment(CrossAxisAlignment::Start);
    let avatar = widget::Image::new(ImageBuf::empty())
        .lens(Message::avatar)
        .fix_size(50.0, 50.0);
    let edit_button = widget::Button::new("...")
        .on_click(|_, data: &mut Message, _| println!("{}", data.event_id))
        .align_right();
    let mut row = widget::Flex::row()
        .with_child(avatar)
        .with_spacer(2.0)
        .with_child(column)
        .with_flex_spacer(1.0)
        .with_child(edit_button);
    row.set_cross_axis_alignment(CrossAxisAlignment::Start);
    widget::Container::new(row.expand_width())
        .expand_width()
}

pub fn build_ui() -> impl Widget<Chat> {
    let messages = widget::List::new(create_message)
        .lens(CurrentChannelLens.map(|v| {
            if let Some(v) = v.channels_hashed.get(&v.current_channel) {
                v.messages.clone()
            } else {
                Vector::new()
            }
        }, |_, _| {}));
    let messages = widget::Scroll::new(messages)
        .vertical()
        .expand();
    let textbox = widget::TextBox::multiline()
        .with_placeholder("Say hello!")
        .lens(Chat::editing_message)
        .expand_width();
    let textbox = widget::ControllerHost::new(textbox, MessageEntryController);
    let textbox = widget::Scroll::new(textbox)
        .vertical();
    let right = widget::Flex::column()
        .with_flex_child(messages, 1.0)
        .with_flex_child(textbox.align_vertical(UnitPoint::BOTTOM_LEFT), 0.1)
        .expand_width();

    let channels = widget::List::new(create_channel_listing)
        .lens(AllChannelsLens);
    let channels = widget::Scroll::new(channels)
        .vertical();
    let top = widget::Split::columns(channels, right)
        .split_point(0.2);
    widget::ControllerHost::new(top, ChatController)
        // .debug_paint_layout()
}
