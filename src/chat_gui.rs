use std::sync::Arc;

use druid::im::{HashMap, Vector};
use druid::keyboard_types::Key;
use druid::text::{Attribute, RichText};
use druid::widget::{CrossAxisAlignment, LineBreaking, ListIter};
use druid::{
    widget, Color, Data, Env, Event, EventCtx, FontFamily, FontStyle, FontWeight, ImageBuf, Lens,
    LensExt, Selector, TextAlignment, Widget, WidgetExt, WidgetId,
};
use image::DynamicImage;
use kuchiki::traits::TendrilSink;
use kuchiki::{NodeData, NodeRef};
use reqwest::Error;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
// use uwuifier::uwuify_str_sse;

use super::chat::{RoomEvent, SyncState};
use super::markdown;

pub const SYNC: Selector<SyncState> = Selector::new("uwutalk.matrix.sync");
pub const SYNC_FAIL: Selector<Error> = Selector::new("uwutalk.matrix.fail.sync");
pub const FETCH_THUMBNAIL: Selector<DynamicImage> = Selector::new("uwutalk.matrix.fetch_thumbnail");
pub const FETCH_THUMBNAIL_FAIL: Selector<Error> =
    Selector::new("uwutalk.matrix.fail.fetch_thumbnail");

pub enum ClientMessage {
    Quit,
    SendMessage(String, String, String),
    ClientSync(String, String),
    FetchThumbnail(String, WidgetId, u64, u64),
    EditMessage(String, String, String, String),
}

#[derive(Data, Clone, Lens)]
struct Channel {
    id: Arc<String>,
    name: Arc<String>,
    messages: Vector<Message>,
    unresolved_edits: Vector<Edit>,
}

#[derive(Data, Clone)]
enum ThumbnailState {
    None,
    Url(String, u64, u64),
    Processing(String, u64, u64),
    Image(Arc<ImageBuf>, u64, u64),
}

#[derive(Data, Clone)]
struct Edit {
    associated_event_id: Arc<String>,
    contents: Arc<String>,
    formatted: RichText,
}

#[derive(Data, Clone, Lens)]
struct Message {
    edit: Option<Edit>,
    sender: Arc<String>,
    avatar: Arc<ImageBuf>,
    event_id: Arc<String>,
    contents: Arc<String>,
    formatted: RichText,
    image: ThumbnailState,
    editing_message: Arc<String>,
    editing: bool,
    channel: Arc<String>,

    #[data(ignore)]
    tx: mpsc::Sender<ClientMessage>,
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
            tx,
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
            let val = (
                self.current_channel.clone(),
                self.channels_hashed.get(channel).unwrap().clone(),
            );
            cb(&val, i);
        }
    }

    fn for_each_mut(&mut self, mut cb: impl FnMut(&mut (Arc<String>, Channel), usize)) {
        for (i, channel) in self.channels.iter().enumerate() {
            let mut val = (
                self.current_channel.clone(),
                self.channels_hashed.get(channel).unwrap().clone(),
            );
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
    name: String,
    attributes: std::collections::HashMap<String, String>,
}

fn extract_text_and_text_attributes_from_dom(
    node: NodeRef,
    buffer: &mut String,
    attrs: &mut Vec<((usize, usize), Element)>,
) {
    match node.data() {
        NodeData::Text(t) => buffer.push_str(&*t.borrow()),

        NodeData::Element(e) => {
            let index = attrs.len();
            attrs.push((
                (buffer.len(), 0),
                Element {
                    name: e.name.local.to_string(),
                    attributes: e
                        .attributes
                        .borrow()
                        .map
                        .iter()
                        .map(|(name, val)| (name.local.to_string(), val.value.clone()))
                        .collect(),
                },
            ));
            for child in node.children() {
                extract_text_and_text_attributes_from_dom(child, buffer, attrs);
            }
            attrs[index].0 .1 = buffer.len();
        }

        NodeData::Comment(_) => (),
        NodeData::ProcessingInstruction(_) => (),
        NodeData::Doctype(_) => (),

        NodeData::Document(_) => {
            for child in node.children() {
                extract_text_and_text_attributes_from_dom(child, buffer, attrs);
            }
        }

        NodeData::DocumentFragment => (),
    }
}

fn make_rich_text(
    formatted: Option<&Value>,
    default: Option<&Value>,
    mark_edited: bool,
) -> RichText {
    let mut attrs = vec![];
    let edited_message = "    (edited)";
    let formatted: Arc<str> = match formatted {
        Some(v) => {
            let root = kuchiki::parse_html().one(v.as_str().unwrap());
            let mut result = String::new();
            for child in root.children() {
                extract_text_and_text_attributes_from_dom(child, &mut result, &mut attrs);
            }
            if mark_edited {
                result.push_str(edited_message);
            }
            Arc::from(result)
        }

        None => {
            let default = default.map(|v| v.as_str().unwrap_or("")).unwrap_or("");
            if mark_edited {
                let mut default = String::from(default);
                default.push_str(edited_message);
                Arc::from(default)
            } else {
                Arc::from(default)
            }
        }
    };

    let mut formatted = RichText::new(formatted);
    for ((s, e), attr) in attrs {
        match attr.name.as_str() {
            "em" => {
                formatted.add_attribute(s..e, Attribute::Style(FontStyle::Italic));
            }

            "strong" => {
                formatted.add_attribute(s..e, Attribute::Weight(FontWeight::new(700)));
            }

            "u" => {
                formatted.add_attribute(s..e, Attribute::Underline(true));
            }

            "code" => {
                formatted.add_attribute(s..e, Attribute::FontFamily(FontFamily::MONOSPACE));
                formatted.add_attribute(s..e, Attribute::text_color(Color::grey8(200)));
            }

            "span" if attr.attributes.contains_key("data-mx-spoiler") => {
                // TODO
            }

            "a" => {
                // TODO
            }

            _ => (),
        }
    }

    if mark_edited {
        let range = formatted.len() - edited_message.len()..;
        formatted.add_attribute(range.clone(), Attribute::text_color(Color::GRAY));
        formatted.add_attribute(range, Attribute::size(10.0));
    }

    formatted
}

fn make_message(
    channel: Arc<String>,
    tx: mpsc::Sender<ClientMessage>,
) -> impl Fn(&RoomEvent) -> Message {
    move |event: &RoomEvent| {
        let formatted = make_rich_text(
            event.content.get("formatted_body"),
            event.content.get("body"),
            false,
        );
        let image = match event.content.get("msgtype") {
            Some(v) if matches!(v.as_str(), Some("m.image")) => {
                let url = event.content.get("url").unwrap().as_str().unwrap();
                let info = event.content.get("info").unwrap();
                let width = info.get("w").and_then(Value::as_u64).unwrap_or(0);
                let height = info.get("h").and_then(Value::as_u64).unwrap_or(0);
                ThumbnailState::Url(String::from(url), width, height)
            }

            _ => ThumbnailState::None,
        };

        let contents = match event.content.get("body") {
            Some(v) => Arc::new(String::from(v.as_str().unwrap())),
            None => Arc::new(String::new()),
        };

        let edit = match event
            .content
            .get("m.relates_to")
            .and_then(|v| v.get("rel_type"))
            .and_then(Value::as_str)
        {
            Some(v) if v == "m.replace" => {
                if let Some(new) = event.content.get("m.new_content") {
                    let contents =
                        Arc::new(String::from(new.get("body").unwrap().as_str().unwrap()));
                    let formatted =
                        make_rich_text(new.get("formatted_body"), new.get("body"), true);
                    Some(Edit {
                        associated_event_id: Arc::new(String::from(
                            event
                                .content
                                .get("m.relates_to")
                                .unwrap()
                                .get("event_id")
                                .unwrap()
                                .as_str()
                                .unwrap(),
                        )),
                        contents,
                        formatted,
                    })
                } else {
                    None
                }
            }

            _ => None,
        };

        Message {
            edit,
            sender: Arc::new(event.sender.clone()),
            avatar: Arc::new(ImageBuf::empty()),
            event_id: Arc::new(event.event_id.clone()),
            contents: contents.clone(),
            formatted,
            image,
            editing_message: contents,
            editing: false,
            channel: channel.clone(),
            tx: tx.clone(),
        }
    }
}

struct ChatController;

impl<W> widget::Controller<Chat, W> for ChatController
where
    W: widget::Widget<Chat>,
{
    fn event(
        &mut self,
        child: &mut W,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut Chat,
        env: &Env,
    ) {
        match event {
            Event::WindowConnected => {
                match data.tx.try_send(ClientMessage::ClientSync(
                    String::new(),
                    json!({
                        "room": {
                            "timeline": {
                                "limit": 50,
                                "types": [
                                    "m.room.message"
                                ]
                            }
                        }
                    })
                    .to_string(),
                )) {
                    Ok(_) => (),
                    Err(TrySendError::Full(_)) => panic!("idk what to do here :("),
                    Err(TrySendError::Closed(_)) => panic!("oh no"),
                }
            }

            Event::Command(cmd) if cmd.is(SYNC_FAIL) => {
                // TODO: something smarter than this
                match data.tx.try_send(ClientMessage::ClientSync(
                    String::new(),
                    json!({
                        "room": {
                            "timeline": {
                                "limit": 50,
                                "types": [
                                    "m.room.message"
                                ]
                            }
                        }
                    })
                    .to_string(),
                )) {
                    Ok(_) => (),
                    Err(TrySendError::Full(_)) => panic!("idk what to do here :("),
                    Err(TrySendError::Closed(_)) => panic!("oh no"),
                }
            }

            Event::Command(cmd) if cmd.is(SYNC) => {
                let sync = cmd.get_unchecked(SYNC);
                if let Some(rooms) = &sync.rooms {
                    if let Some(join) = &rooms.join {
                        for (id, joined) in join.iter() {
                            let (mut messages, mut edits) = (Vector::new(), Vector::new());
                            for m in joined
                                .timeline
                                .events
                                .iter()
                                .map(make_message(Arc::new(id.clone()), data.tx.clone()))
                            {
                                match m.edit {
                                    Some(e) => edits.push_back(e),
                                    None => messages.push_back(m),
                                }
                            }

                            if let Some(channel) = data.channels_hashed.get_mut(id) {
                                channel.messages.extend(messages);
                            } else {
                                data.channels_hashed.insert(
                                    Arc::new(id.clone()),
                                    Channel {
                                        id: Arc::new(id.clone()),
                                        name: Arc::new(match &joined.name {
                                            Some(v) => v.clone(),
                                            None => String::from("<unnamed room>"),
                                        }),
                                        messages,
                                        unresolved_edits: Vector::new(),
                                    },
                                );
                                data.channels.push_back(Arc::new(id.clone()));
                            }
                            if let Some(channel) = data.channels_hashed.get_mut(id) {
                                let mut resolved = vec![];
                                for (i, edit) in edits.iter().enumerate() {
                                    for msg in channel.messages.iter_mut() {
                                        if msg.event_id == edit.associated_event_id {
                                            msg.contents = edit.contents.clone();
                                            msg.formatted = edit.formatted.clone();
                                            resolved.push(i);
                                            break;
                                        }
                                    }
                                }

                                for (i, resolved) in resolved.into_iter().enumerate() {
                                    edits.remove(resolved - i);
                                }

                                channel.unresolved_edits = edits;
                            }
                        }
                    }
                }

                match data.tx.try_send(ClientMessage::ClientSync(
                    sync.next_batch.clone(),
                    json!({
                        "room": {
                            "timeline": {
                                "limit": 50,
                                "types": [
                                    "m.room.message"
                                ]
                            }
                        }
                    })
                    .to_string(),
                )) {
                    Ok(_) => (),
                    Err(TrySendError::Full(_)) => panic!("idk what to do here :("),
                    Err(TrySendError::Closed(_)) => panic!("oh no"),
                }
            }

            Event::WindowDisconnected => {
                while let Err(TrySendError::Full(_)) = data.tx.try_send(ClientMessage::Quit) {}
            }

            _ => (),
        }

        child.event(ctx, event, data, env)
    }
}

struct MessageEntryController;

impl<W> widget::Controller<Chat, W> for MessageEntryController
where
    W: Widget<Chat>,
{
    fn event(
        &mut self,
        child: &mut W,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut Chat,
        env: &Env,
    ) {
        match event {
            Event::KeyDown(key) if key.key == Key::Enter && !key.mods.shift() => {
                if !data.editing_message.is_empty() {
                    // TODO: do this based on current cursor position
                    let count = data.editing_message.match_indices("```").count();
                    if count % 2 == 0 {
                        let formatted = markdown::parse_markdown(&*data.editing_message);
                        let formatted = markdown::markdown_to_html(formatted);
                        match data.tx.try_send(ClientMessage::SendMessage(
                            (*data.current_channel).clone(),
                            (*data.editing_message).clone(),
                            formatted,
                        )) {
                            Ok(_) => (),
                            Err(TrySendError::Full(_)) => panic!("idk what to do here :("),
                            Err(TrySendError::Closed(_)) => panic!("oh no"),
                        }
                        data.editing_message = Arc::new(String::new());
                        ctx.set_handled();
                    }
                } else {
                    ctx.set_handled();
                }
            }

            _ => (),
        }
        child.event(ctx, event, data, env);
    }
}

struct EditEntryController;

impl<W> widget::Controller<Message, W> for EditEntryController
where
    W: Widget<Message>,
{
    fn event(
        &mut self,
        child: &mut W,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut Message,
        env: &Env,
    ) {
        match event {
            Event::KeyDown(key) if key.key == Key::Enter && !key.mods.shift() => {
                if !data.editing_message.is_empty() {
                    // TODO: do this based on current cursor position
                    let count = data.editing_message.match_indices("```").count();
                    if count % 2 == 0 {
                        let formatted = markdown::parse_markdown(&*data.editing_message);
                        let formatted = markdown::markdown_to_html(formatted);
                        match data.tx.try_send(ClientMessage::EditMessage(
                            (*data.channel).clone(),
                            (*data.event_id).clone(),
                            (*data.editing_message).clone(),
                            formatted,
                        )) {
                            Ok(_) => (),
                            Err(TrySendError::Full(_)) => panic!("idk what to do here :("),
                            Err(TrySendError::Closed(_)) => panic!("oh no"),
                        }
                        data.editing_message = Arc::new(String::new());
                        data.editing = false;
                        ctx.set_handled();
                    }
                } else {
                    ctx.set_handled();
                }
            }

            _ => (),
        }
        child.event(ctx, event, data, env)
    }
}

fn create_channel_listing() -> impl Widget<(Arc<String>, Channel)> {
    widget::Button::dynamic(|data: &(Arc<String>, Channel), _| (*data.1.name).clone())
        .on_click(|_, (current_channel, channel), _| *current_channel = channel.id.clone())
}

#[derive(Data, Clone, Copy, PartialEq)]
enum ContentState {
    Text,
    Editing,
    Spinner,
    Image,
}

struct MediaController;

impl<W> widget::Controller<Message, W> for MediaController
where
    W: Widget<Message>,
{
    fn event(
        &mut self,
        child: &mut W,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut Message,
        env: &Env,
    ) {
        match event {
            Event::Command(cmd) if cmd.is(FETCH_THUMBNAIL_FAIL) => {
                if let ThumbnailState::Url(url, width, height) = &data.image {
                    match data.tx.try_send(ClientMessage::FetchThumbnail(
                        url.clone(),
                        ctx.widget_id(),
                        *width,
                        *height,
                    )) {
                        Ok(_) => (),
                        Err(TrySendError::Full(_)) => panic!("oh no"),
                        Err(TrySendError::Closed(_)) => panic!("oh no"),
                    }
                }
            }

            Event::Command(cmd) if cmd.is(SYNC) => {
                if let ThumbnailState::Url(url, width, height) = &data.image {
                    match data.tx.try_send(ClientMessage::FetchThumbnail(
                        url.clone(),
                        ctx.widget_id(),
                        *width,
                        *height,
                    )) {
                        Ok(_) => (),
                        Err(TrySendError::Full(_)) => panic!("oh no"),
                        Err(TrySendError::Closed(_)) => panic!("oh no"),
                    }
                    data.image = ThumbnailState::Processing(url.clone(), *width, *height);
                    ctx.set_handled();
                } else {
                    child.event(ctx, event, data, env);
                }
            }

            Event::Command(cmd) if cmd.is(FETCH_THUMBNAIL) => {
                let image = cmd.get_unchecked(FETCH_THUMBNAIL);
                let (width, height) = match data.image {
                    ThumbnailState::None => panic!("eeeeee"),
                    ThumbnailState::Url(_, w, h)
                    | ThumbnailState::Processing(_, w, h)
                    | ThumbnailState::Image(_, w, h) => (w, h),
                };

                let image = Arc::from(image.as_rgba8().unwrap().get(..).unwrap());
                data.image = ThumbnailState::Image(
                    Arc::new(ImageBuf::from_raw(
                        image,
                        druid::piet::ImageFormat::RgbaSeparate,
                        width as usize,
                        height as usize,
                    )),
                    width,
                    height,
                );
                ctx.set_handled();
            }

            _ => child.event(ctx, event, data, env),
        }
    }
}

fn create_message() -> impl Widget<Message> {
    let contents = widget::ViewSwitcher::new(
        |data: &Message, _| {
            if data.editing {
                ContentState::Editing
            } else {
                match data.image {
                    ThumbnailState::None => ContentState::Text,
                    ThumbnailState::Url(_, _, _) => ContentState::Spinner,
                    ThumbnailState::Processing(_, _, _) => ContentState::Spinner,
                    ThumbnailState::Image(_, _, _) => ContentState::Image,
                }
            }
        },
        |state, data, _| match state {
            ContentState::Text => widget::RawLabel::new()
                .with_text_alignment(TextAlignment::Start)
                .with_line_break_mode(LineBreaking::WordWrap)
                .lens(Message::formatted)
                .boxed(),

            ContentState::Editing => widget::TextBox::multiline()
                .lens(Message::editing_message)
                .controller(EditEntryController)
                .expand_width()
                .boxed(),

            ContentState::Spinner => widget::Spinner::new().controller(MediaController).boxed(),

            ContentState::Image => {
                let buffer = match &data.image {
                    ThumbnailState::Image(buffer, _, _) => (**buffer).clone(),
                    _ => panic!("nyaaa :("),
                };

                widget::Image::new(buffer).boxed()
            }
        },
    );
    let sender = widget::Label::dynamic(|v: &Message, _| (*v.sender).clone())
        .with_text_alignment(TextAlignment::Start);
    let edit_button = widget::Button::new("...")
        .on_click(|_, data: &mut Message, _| {
            data.editing ^= true;
            if data.editing {
                data.editing_message = data.contents.clone();
            }
        })
        .align_right();
    let mut row = widget::Flex::row()
        .with_child(sender)
        .with_flex_spacer(1.0)
        .with_child(edit_button);
    row.set_cross_axis_alignment(CrossAxisAlignment::Start);
    let mut column = widget::Flex::column()
        .with_child(row)
        .with_spacer(2.0)
        .with_child(contents);
    column.set_cross_axis_alignment(CrossAxisAlignment::Start);
    let avatar = widget::Image::new(ImageBuf::empty())
        .lens(Message::avatar)
        .fix_size(50.0, 50.0);
    let mut row = widget::Flex::row()
        .with_child(avatar)
        .with_spacer(2.0)
        .with_flex_child(column, 1.0);
    row.set_cross_axis_alignment(CrossAxisAlignment::Start);
    widget::Container::new(row).padding(5.0).expand_width()
}

pub fn build_ui() -> impl Widget<Chat> {
    let messages = widget::List::new(create_message)
        .lens(CurrentChannelLens.map(
            |v| {
                if let Some(v) = v.channels_hashed.get(&v.current_channel) {
                    v.messages.clone()
                } else {
                    Vector::new()
                }
            },
            |state, data| {
                if let Some(v) = state.channels_hashed.get_mut(&state.current_channel) {
                    v.messages = data;
                }
            },
        ))
        .scroll()
        .vertical()
        .expand_height();
    let textbox = widget::TextBox::multiline()
        .with_placeholder("Say hello!")
        .lens(Chat::editing_message)
        .expand_width()
        .controller(MessageEntryController)
        .scroll()
        .vertical();
    let right = widget::Flex::column()
        .with_flex_child(messages, 1.0)
        .with_child(textbox);

    let channels = widget::List::new(create_channel_listing).lens(AllChannelsLens);
    let channels = widget::Scroll::new(channels).vertical();
    widget::Split::columns(channels, right)
        .split_point(0.2)
        .controller(ChatController)
        .padding(5.0)
        // .debug_paint_layout()
}
