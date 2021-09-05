use std::sync::Arc;

use druid::im::{HashMap, Vector};
use druid::keyboard_types::Key;
use druid::text::{Attribute, RichText, RichTextBuilder};
use druid::widget::{Axis, CrossAxisAlignment, LineBreaking, ListIter};
use druid::{Color, Data, Env, Event, EventCtx, FontFamily, FontStyle, FontWeight, ImageBuf, Lens, LensExt, Point, Selector, TextAlignment, Widget, WidgetExt, WidgetId, widget};
use kuchiki::traits::TendrilSink;
use kuchiki::{NodeData, NodeRef};
use reqwest::Error;
use serde_json::json;
use ijson::{IString, IValue as Value};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
// use uwuifier::uwuify_str_sse;

use super::chat::{RoomEvent, RoomMessages, SyncState};
use super::markdown;

pub const SYNC: Selector<SyncState> = Selector::new("uwutalk.matrix.sync");
pub const SYNC_FAIL: Selector<Error> = Selector::new("uwutalk.matrix.fail.sync");
pub const FETCH_FROM_ROOM: Selector<(Arc<String>, RoomMessages)> = Selector::new("uwutalk.matrix.fetch_from_room");
pub const FETCH_FROM_ROOM_FAIL: Selector<Error> = Selector::new("uwutalk.matrix.fail.fetch_from_room");
pub const FETCH_THUMBNAIL: Selector<ImageBuf> = Selector::new("uwutalk.matrix.fetch_thumbnail");
pub const FETCH_THUMBNAIL_FAIL: Selector<Error> = Selector::new("uwutalk.matrix.fail.fetch_thumbnail");
const SCROLLED: Selector<()> = Selector::new("uwutalk.matrix.scrolled");
const LINK: Selector<Arc<str>> = Selector::new("uwutalk.matrix.link");

pub enum Syncing {
    Quit,
    ClientSync(Arc<String>, Arc<String>),
    FetchFromRoom(Arc<String>, Arc<String>, Arc<String>)
}

pub enum UserAction {
    Quit,
    SendMessage(Arc<String>, Arc<String>, Arc<String>),
    EditMessage(Arc<String>, Arc<String>, Arc<String>, Arc<String>),
}

pub enum MediaFetch {
    Quit,
    FetchThumbnail(Arc<String>, WidgetId, u64, u64),
    AvatarFetch(Arc<String>, WidgetId),
}

#[derive(Clone)]
struct Senders {
    sync_tx: mpsc::Sender<Syncing>,
    action_tx: mpsc::Sender<UserAction>,
    media_tx: mpsc::Sender<MediaFetch>,
}

#[derive(Data, Clone, Lens)]
struct Channel {
    id: Arc<String>,
    name: Arc<String>,
    messages: Vector<Message>,
    unresolved_edits: Vector<Edit>,
    prev_batch: Arc<String>,
    first_batch: Arc<String>,
    bottom: bool,
    fetching_old: bool,
    top: bool,
}

#[derive(Data, Clone)]
enum ThumbnailState {
    None,
    Url(Arc<String>, u64, u64),
    Processing(Arc<String>, u64, u64),
    Image(Arc<ImageBuf>, u64, u64),
}

#[derive(Data, Clone)]
struct Edit {
    associated_event_id: Arc<String>,
    contents: Arc<String>,
    formatted: RichText,
}

#[derive(Data, Clone)]
enum AvatarState {
    Name(Arc<String>),
    Processing(Arc<String>),
    Image(Arc<ImageBuf>),
}

#[derive(Data, Clone, Lens)]
struct Message {
    edit: Option<Edit>,
    sender: Arc<String>,
    avatar: AvatarState,
    event_id: Arc<String>,
    contents: Arc<String>,
    formatted: RichText,
    image: ThumbnailState,
    editing_message: Arc<String>,
    editing: bool,
    channel: Arc<String>,

    #[data(ignore)]
    txs: Senders,
}

#[derive(Data, Clone, Lens)]
pub struct Chat {
    editing_message: Arc<String>,
    channels_hashed: HashMap<Arc<String>, Channel>,
    channels: Vector<Arc<String>>,
    current_channel: Arc<String>,

    #[data(ignore)]
    scroll: Option<f64>,

    #[data(ignore)]
    txs: Senders,
}

impl Chat {
    pub fn new(sync_tx: mpsc::Sender<Syncing>, action_tx: mpsc::Sender<UserAction>, media_tx: mpsc::Sender<MediaFetch>) -> Chat {
        Chat {
            editing_message: Arc::new(String::new()),
            channels_hashed: HashMap::new(),
            channels: Vector::new(),
            current_channel: Arc::new(String::new()),
            scroll: None,
            txs: Senders {
                sync_tx,
                action_tx,
                media_tx,
            },
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
        if data.current_channel != all.current_channel {
            if let Some(channel) = data.channels_hashed.get_mut(&data.current_channel) {
                channel.bottom = true;
                channel.top = false;
                channel.fetching_old = false;
                if channel.messages.len() > 50 {
                    channel.messages = channel.messages.skip(channel.messages.len() - 50);
                }
                channel.prev_batch = channel.first_batch.clone();
            }
            data.current_channel = all.current_channel;
        }
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

fn extract_text_and_text_attributes_from_dom(
    node: NodeRef,
    builder: &mut RichTextBuilder,
    current_pos: &mut usize,
) {
    match node.data() {
        NodeData::Text(t) => {
            let t = t.borrow();
            builder.push(&*t);
            *current_pos += t.len();
        }

        NodeData::Element(e) => {
            let start = *current_pos;
            for child in node.children() {
                extract_text_and_text_attributes_from_dom(child, builder, current_pos);
            }
            let end = *current_pos;
            if e.name.local.as_ref().starts_with('h') || e.name.local.as_ref() == "br" {
                builder.push("\n");
                *current_pos += 1;
            }

            let text_size = 16.0;
            match e.name.local.as_ref() {
                "em" => {
                    builder.add_attributes_for_range(start..end)
                        .add_attr(Attribute::Style(FontStyle::Italic));
                }

                "strong" => {
                    builder.add_attributes_for_range(start..end)
                        .add_attr(Attribute::Weight(FontWeight::new(700)));
                }

                "u" => {
                    builder.add_attributes_for_range(start..end)
                        .add_attr(Attribute::Underline(true));
                }

                "code" => {
                    builder.add_attributes_for_range(start..end)
                        .add_attr(Attribute::FontFamily(FontFamily::MONOSPACE))
                        .add_attr(Attribute::text_color(Color::grey8(200)));
                }

                "h1" => {
                    builder.add_attributes_for_range(start..end)
                        .add_attr(Attribute::size(text_size * 2.0))
                        .add_attr(Attribute::Weight(FontWeight::new(700)));
                }

                "h2" => {
                    builder.add_attributes_for_range(start..end)
                        .add_attr(Attribute::size(text_size * 1.5))
                        .add_attr(Attribute::Weight(FontWeight::new(700)));
                }

                "h3" => {
                    builder.add_attributes_for_range(start..end)
                        .add_attr(Attribute::size(text_size * 1.17))
                        .add_attr(Attribute::Weight(FontWeight::new(700)));
                }

                "h4" => {
                    builder.add_attributes_for_range(start..end)
                        .add_attr(Attribute::size(text_size))
                        .add_attr(Attribute::Weight(FontWeight::new(700)));
                }

                "h5" => {
                    builder.add_attributes_for_range(start..end)
                        .add_attr(Attribute::size(text_size * 0.83))
                        .add_attr(Attribute::Weight(FontWeight::new(700)));
                }

                "h6" => {
                    builder.add_attributes_for_range(start..end)
                        .add_attr(Attribute::size(text_size * 0.67))
                        .add_attr(Attribute::Weight(FontWeight::new(700)));
                }

                "span" if e.attributes.borrow().contains("data-mx-spoiler") => {
                    // TODO
                }

                "a" => {
                    let attrs = e.attributes.borrow();
                    let mut href = attrs.get("href").unwrap_or("");
                    let mut buffer = String::new();
                    if !href.is_empty() && !href.contains("://") {
                        buffer.push_str("https://");
                        buffer.push_str(href);
                        href = buffer.as_str();
                    }

                    builder.add_attributes_for_range(start..end)
                        .add_attr(Attribute::text_color(Color::BLUE))
                        .add_attr(Attribute::Underline(true))
                        .link(LINK.with(Arc::from(href)));
                }

                _ => (),
            }
        }

        NodeData::Comment(_) => (),
        NodeData::ProcessingInstruction(_) => (),
        NodeData::Doctype(_) => (),

        NodeData::Document(_) => {
            for child in node.children() {
                extract_text_and_text_attributes_from_dom(child, builder, current_pos);
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
    let edited_message = "    (edited)";

    match formatted {
        Some(v) => {
            let root = kuchiki::parse_html().one(v.as_string().unwrap().as_str());
            let mut builder = RichTextBuilder::new();
            let mut current_pos = 0;
            extract_text_and_text_attributes_from_dom(root, &mut builder, &mut current_pos);
            if mark_edited {
                builder.push(edited_message);
                builder.add_attributes_for_range(current_pos..)
                    .add_attr(Attribute::text_color(Color::GRAY))
                    .add_attr(Attribute::size(10.0));
            }
            builder.build()
        }

        None => {
            let mut builder = RichTextBuilder::new();
            let default = default.and_then(|v| v.as_string()).map(IString::as_str).unwrap_or("");
            builder.push(default);
            if mark_edited {
                builder.push(edited_message);
                builder.add_attributes_for_range(default.len()..)
                    .add_attr(Attribute::text_color(Color::GRAY))
                    .add_attr(Attribute::size(10.0));
            }
            builder.build()
        }
    }
}

fn make_message(
    channel: Arc<String>,
    txs: Senders,
) -> impl Fn(&RoomEvent) -> Message {
    move |event: &RoomEvent| {
        let formatted = make_rich_text(
            event.content.get("formatted_body"),
            event.content.get("body"),
            false,
        );
        let image = match event.content.get("msgtype") {
            Some(v) if matches!(v.as_string(), Some(v) if v.as_str() == "m.image") => {
                let url = event.content.get("url").unwrap().as_string().unwrap().as_str();
                let info = event.content.get("info").unwrap();
                let width = info.get("w").and_then(Value::to_u64).unwrap_or(0);
                let height = info.get("h").and_then(Value::to_u64).unwrap_or(0);
                ThumbnailState::Url(Arc::new(String::from(url)), width, height)
            }

            _ => ThumbnailState::None,
        };

        let contents = match event.content.get("body") {
            Some(v) => Arc::new(String::from(v.as_string().unwrap().as_str())),
            None => Arc::new(String::new()),
        };

        let edit = match event
            .content
            .get("m.relates_to")
            .and_then(|v| v.get("rel_type"))
            .and_then(Value::as_string)
            .map(IString::as_str)
        {
            Some(v) if v == "m.replace" => {
                if let Some(new) = event.content.get("m.new_content") {
                    let contents =
                        Arc::new(String::from(new.get("body").unwrap().as_string().unwrap().as_str()));
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
                                .as_string()
                                .unwrap()
                                .as_str(),
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
            sender: event.sender.clone(),
            avatar: AvatarState::Name(event.sender.clone()),
            event_id: event.event_id.clone(),
            contents: contents.clone(),
            formatted,
            image,
            editing_message: contents,
            editing: false,
            channel: channel.clone(),
            txs: txs.clone(),
        }
    }
}

struct MessageScrollController;

impl<W> widget::Controller<Chat, widget::Scroll<Chat, W>> for MessageScrollController
    where W: Widget<Chat>
{
    fn event(&mut self, child: &mut widget::Scroll<Chat, W>, ctx: &mut EventCtx, event: &Event, data: &mut Chat, env: &Env) {
        match event {
            Event::Wheel(wheel) => {
                if let Some(channel) = data.channels_hashed.get_mut(&data.current_channel) {
                    if channel.bottom && wheel.wheel_delta.y < 0.0 {
                        channel.bottom = false;
                    }
                }
            }

            Event::Command(cmd) if cmd.is(SCROLLED) && data.scroll.is_some() => {
                data.scroll = None;
                if let Some(channel) = data.channels_hashed.get_mut(&data.current_channel) {
                    channel.fetching_old = false;
                }
            }

            Event::Command(cmd) if cmd.is(FETCH_FROM_ROOM) => {
                let (channel, state) = cmd.get_unchecked(FETCH_FROM_ROOM);
                if let Some(channel) = data.channels_hashed.get_mut(channel) {
                    channel.prev_batch = state.end.clone();
                    if channel.first_batch.is_empty() {
                        channel.first_batch = channel.prev_batch.clone();
                    }
                    channel.top = state.chunk.is_empty();
                    data.scroll = Some(child.child_size().height);

                    let mut messages = Vector::new();
                    for m in state
                        .chunk
                        .iter()
                        .map(make_message(channel.id.clone(), data.txs.clone()))
                    {
                        match m.edit {
                            Some(e) => channel.unresolved_edits.push_back(e),
                            None => messages.push_front(m),
                        }
                    }

                    messages.extend(channel.messages.clone());
                    channel.messages = messages;
                    let mut resolved = vec![];
                    for (i, edit) in channel.unresolved_edits.iter().enumerate() {
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
                        channel.unresolved_edits.remove(resolved - i);
                    }
                }
            }

            _ => (),
        }

        child.event(ctx, event, data, env);

        if let Some(channel) = data.channels_hashed.get_mut(&data.current_channel) {
            if !channel.bottom && child.viewport_rect().contains(Point {
                x: 0.0,
                y: child.child_size().height - 0.01,
            }) {
                channel.bottom = true;
            }

            if !channel.fetching_old && !channel.top && (child.viewport_rect().contains(Point {
                x: 0.0,
                y: 0.0,
            }) || child.child_size().height == 0.0) {
                match data.txs.sync_tx.try_send(Syncing::FetchFromRoom(channel.id.clone(), channel.prev_batch.clone(), Arc::new(json!({
                    "limit": 50,
                    "types": [
                        "m.room.message"
                    ]
                }).to_string()))) {
                    Ok(_) => (),
                    Err(TrySendError::Full(_)) => panic!("oh no"),
                    Err(TrySendError::Closed(_)) => panic!("aaaaa"),
                }

                channel.fetching_old = true;
            }
        }
    }

    fn lifecycle(
        &mut self,
        child: &mut widget::Scroll<Chat, W>,
        ctx: &mut druid::LifeCycleCtx,
        event: &druid::LifeCycle,
        data: &Chat,
        env: &Env,
    ) {
        child.lifecycle(ctx, event, data, env);
        if let Some(channel) = data.channels_hashed.get(&data.current_channel) {
            if channel.bottom {
                child.scroll_to_on_axis(Axis::Vertical, f64::INFINITY);
            } else if let Some(scroll) = data.scroll {
                if (scroll - child.child_size().height).abs() > 0.001 {
                    child.scroll_to_on_axis(Axis::Vertical, child.child_size().height - scroll);
                    ctx.submit_command(SCROLLED);
                }
            }
        }
    }

    fn update(&mut self, child: &mut widget::Scroll<Chat, W>, ctx: &mut druid::UpdateCtx, old_data: &Chat, data: &Chat, env: &Env) {
        child.update(ctx, old_data, data, env);
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
                match data.txs.sync_tx.try_send(Syncing::ClientSync(
                    Arc::new(String::new()),
                    Arc::new(json!({
                        "presence": {
                            "limit": 0,
                        },
                        "room": {
                            "ephemeral": {
                                "limit": 0,
                            },
                            "state": {
                                "limit": 0,
                            },
                            "timeline": {
                                "limit": 0,
                            },
                        },
                    }).to_string()),
                )) {
                    Ok(_) => (),
                    Err(TrySendError::Full(_)) => panic!("idk what to do here :("),
                    Err(TrySendError::Closed(_)) => panic!("oh no"),
                }
            }

            Event::Command(cmd) if cmd.is(SYNC_FAIL) => {
                // TODO: something smarter than this
                match data.txs.sync_tx.try_send(Syncing::ClientSync(
                    Arc::new(String::new()),
                    Arc::new(json!({
                        "room": {
                            "timeline": {
                                "limit": 50,
                                "types": [
                                    "m.room.message"
                                ]
                            }
                        }
                    })
                    .to_string()),
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
                                .map(make_message(id.clone(), data.txs.clone()))
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
                                    id.clone(),
                                    Channel {
                                        id: id.clone(),
                                        name: match &joined.name {
                                            Some(v) => v.clone(),
                                            None => Arc::new(String::from("<unnamed room>")),
                                        },
                                        messages,
                                        unresolved_edits: Vector::new(),
                                        prev_batch: Arc::new(String::new()),
                                        first_batch: Arc::new(String::new()),
                                        bottom: true,
                                        fetching_old: false,
                                        top: false,
                                    },
                                );
                                data.channels.push_back(id.clone());
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

                match data.txs.sync_tx.try_send(Syncing::ClientSync(
                    sync.next_batch.clone(),
                    Arc::new(json!({
                        "room": {
                            "timeline": {
                                "limit": 50,
                                "types": [
                                    "m.room.message"
                                ]
                            }
                        }
                    })
                    .to_string()),
                )) {
                    Ok(_) => (),
                    Err(TrySendError::Full(_)) => panic!("idk what to do here :("),
                    Err(TrySendError::Closed(_)) => panic!("oh no"),
                }
            }

            Event::Command(cmd) if cmd.is(LINK) => {
                let link = cmd.get_unchecked(LINK);
                if open::that(&**link).is_err() {
                    eprintln!("error opening link {}", link);
                }
            }

            Event::WindowDisconnected => {
                while let Err(TrySendError::Full(_)) = data.txs.sync_tx.try_send(Syncing::Quit) {}
                while let Err(TrySendError::Full(_)) = data.txs.action_tx.try_send(UserAction::Quit) {}
                while let Err(TrySendError::Full(_)) = data.txs.media_tx.try_send(MediaFetch::Quit) {}
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
                        match data.txs.action_tx.try_send(UserAction::SendMessage(
                            data.current_channel.clone(),
                            data.editing_message.clone(),
                            Arc::new(formatted),
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
                        match data.txs.action_tx.try_send(UserAction::EditMessage(
                            data.channel.clone(),
                            data.event_id.clone(),
                            data.editing_message.clone(),
                            Arc::new(formatted),
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
                    match data.txs.media_tx.try_send(MediaFetch::FetchThumbnail(
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
                    match data.txs.media_tx.try_send(MediaFetch::FetchThumbnail(
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

                data.image = ThumbnailState::Image(
                    Arc::new(image.clone()),
                    width,
                    height,
                );
                ctx.set_handled();
            }

            _ => child.event(ctx, event, data, env),
        }
    }
}

struct AvatarController;

impl<W> widget::Controller<Message, W> for AvatarController
    where W: widget::Widget<Message>
{
    fn event(&mut self, child: &mut W, ctx: &mut EventCtx, event: &Event, data: &mut Message, env: &Env) {
        match event {
            Event::Command(cmd) if cmd.is(SYNC) => {
                if let AvatarState::Name(name) = &data.avatar {
                    match data.txs.media_tx.try_send(MediaFetch::AvatarFetch(
                        name.clone(),
                        ctx.widget_id(),
                    )) {
                        Ok(_) => (),
                        Err(TrySendError::Full(_)) => panic!("oh no"),
                        Err(TrySendError::Closed(_)) => panic!("oh no"),
                    }
                    data.avatar = AvatarState::Processing(name.clone());
                    ctx.set_handled();
                } else {
                    child.event(ctx, event, data, env);
                }
            }

            Event::Command(cmd) if cmd.is(FETCH_THUMBNAIL) => {
                let image = cmd.get_unchecked(FETCH_THUMBNAIL);

                data.avatar = AvatarState::Image(
                    Arc::new(image.clone()),
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
    let avatar = widget::ViewSwitcher::new(|data: &Message, _| matches!(data.avatar, AvatarState::Image(_)), |_, data, _| {
        match &data.avatar {
            AvatarState::Name(_)
            | AvatarState::Processing(_) => widget::Image::new(ImageBuf::empty())
                .boxed(),
            AvatarState::Image(buffer) => widget::Image::new((**buffer).clone())
                .boxed(),
        }
    })
        .controller(AvatarController)
        .fix_size(32.0, 32.0);
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
        ));
    let messages = widget::Flex::column()
        .with_child(widget::Either::new(|data: &Chat, _| {
            data.current_channel.is_empty() || if let Some(channel) = data.channels_hashed.get(&data.current_channel) {
                channel.top
            } else {
                false
            }
        }, widget::Image::new(ImageBuf::empty()), widget::Spinner::new()))
        .with_child(messages);
    let messages = widget::Either::new(|data, _| {
        if let Some(channel) = data.channels_hashed.get(&data.current_channel) {
            channel.messages.is_empty()
        } else {
            false
        }
    }, widget::Spinner::new(), messages)
        .scroll()
        .vertical()
        .controller(MessageScrollController)
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
