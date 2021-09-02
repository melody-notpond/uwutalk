use std::{collections::HashMap, sync::Arc};

use reqwest::{Client, Error};
use serde::Deserialize;
use serde_json::json;
use ijson::IValue as Value;

pub struct MatrixClient {
    client: Client,
    homeserver: String,
    access_code: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Event {
    pub event_id: Arc<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct StateEvent {
    pub content: Value,

    #[serde(rename = "type")]
    pub type_: Arc<String>,

    pub event_id: Arc<String>,
    pub sender: Arc<String>,
    pub origin_server_ts: u64,
    pub unsigned: UnsignedData,
    pub prev_content: Option<Value>,
    pub state_key: Arc<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct State {
    pub events: Vec<StateEvent>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct UnsignedData {
    pub age: Option<i64>,
    pub redacted_because: Option<Event>,
    pub transaction_id: Option<Arc<String>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RoomEvent {
    pub content: Value,

    #[serde(rename = "type")]
    pub type_: Arc<String>,
    pub event_id: Arc<String>,
    pub sender: Arc<String>,
    pub origin_server_ts: u64,
    pub unsigned: UnsignedData,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Timeline {
    pub events: Vec<RoomEvent>,
    pub limited: bool,
    pub prev_batch: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Ephemeral {
    pub events: Vec<Value>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct UnreadNotificationCounts {
    pub highlight_count: i64,
    pub notification_count: i64,
}

#[derive(Deserialize, Debug, Clone)]
pub struct JoinedRoom {
    pub name: Option<Arc<String>>,
    pub summary: HashMap<String, Value>,
    pub state: State,
    pub timeline: Timeline,
    pub ephemeral: Ephemeral,
    pub account_data: Value,
    pub unread_notifications: UnreadNotificationCounts,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SyncRooms {
    pub join: Option<HashMap<Arc<String>, JoinedRoom>>,
    pub invite: Option<HashMap<Arc<String>, Value>>,
    pub leave: Option<HashMap<Arc<String>, Value>>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SyncState {
    pub next_batch: Arc<String>,
    pub rooms: Option<SyncRooms>,
    pub presence: Option<Value>,
    pub account_data: Option<Value>,
    pub to_device: Option<Value>,
    pub device_lists: Option<Value>,
    pub device_one_time_keys_count: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct Content {
    pub type_: Arc<String>,
    pub disposition: Arc<String>,
    pub content: Vec<u8>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RoomMessages {
    pub start: Arc<String>,
    pub end: Arc<String>,
    pub chunk: Vec<RoomEvent>,
    pub state: Option<Vec<StateEvent>>,
}

#[derive(Debug, Clone, Copy)]
pub enum RoomDirection {
    Forwards,
    Backwards
}

impl MatrixClient {
    pub fn new(homeserver: &str, access_code: &str) -> MatrixClient {
        MatrixClient {
            client: Client::new(),
            homeserver: String::from(homeserver),
            access_code: String::from(access_code),
        }
    }

    pub async fn send_message(
        &self,
        room: &str,
        content: &str,
        formatted: Option<Arc<String>>,
    ) -> Result<Event, Error> {
        let body = if let Some(formatted) = formatted {
            json!({
                "msgtype": "m.text",
                "body": content,
                "format": "org.matrix.custom.html",
                "formatted_body": formatted,
            })
            .to_string()
        } else {
            json!({
                "msgtype": "m.text",
                "body": content,
            })
            .to_string()
        };

        let event = self
            .client
            .post(format!(
                "https://{}/_matrix/client/r0/rooms/{}/send/m.room.message",
                self.homeserver, room
            ))
            .body(body)
            .bearer_auth(&self.access_code)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        Ok(serde_json::from_str::<Value>(&event).and_then(|v| ijson::from_value(&v)).unwrap())
    }

    pub async fn edit_message(
        &self,
        room: &str,
        event_id: &str,
        content: &str,
        formatted: Option<Arc<String>>,
    ) -> Result<Event, Error> {
        let body = if let Some(formatted) = formatted {
            json!({
                "m.new_content": {
                    "msgtype": "m.text",
                    "body": content,
                    "format": "org.matrix.custom.html",
                    "formatted_body": formatted,
                },
                "m.relates_to": {
                    "rel_type": "m.replace",
                    "event_id": event_id,
                },
                "msgtype": "m.text",
                "body": format!(" * {}", content),
                "format": "org.matrix.custom.html",
                "formatted_body": format!(" * {}", formatted),
            })
            .to_string()
        } else {
            json!({
                "m.new_content": {
                    "msgtype": "m.text",
                    "body": content,
                },
                "m.relates_to": {
                    "rel_type": "m.replace",
                    "event_id": event_id,
                },
                "msgtype": "m.text",
                "body": format!(" * {}", content),
            })
            .to_string()
        };

        let event = self
            .client
            .post(format!(
                "https://{}/_matrix/client/r0/rooms/{}/send/m.room.message",
                self.homeserver, room
            ))
            .body(body)
            .bearer_auth(&self.access_code)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        Ok(serde_json::from_str::<Value>(&event).and_then(|v| ijson::from_value(&v)).unwrap())
    }

    async fn get_name(&self, room: &str) -> Option<Arc<String>> {
        let name = self
            .client
            .get(format!(
                "https://{}/_matrix/client/r0/rooms/{}/state/m.room.name",
                self.homeserver, room
            ))
            .bearer_auth(&self.access_code)
            .send()
            .await
            .ok()?;
        if name.status() == 200 {
            Some(Arc::new(String::from(
                serde_json::from_str::<Value>(&name.text().await.ok()?).ok()?.get("name")?.as_string()?.as_str(),
            )))
        } else {
            let name = self
                .client
                .get(format!(
                    "https://{}/_matrix/client/r0/rooms/{}/state/m.room.canonical_alias",
                    self.homeserver, room
                ))
                .bearer_auth(&self.access_code)
                .send()
                .await
                .ok()?;

            if name.status() == 200 {
                Some(Arc::new(String::from(
                    serde_json::from_str::<Value>(&name.text().await.ok()?).ok()?.get("alias")?
                        .as_string()?.as_str(),
                )))
            } else {
                None
            }
        }
    }

    pub async fn get_state(
        &self,
        since: Option<Arc<String>>,
        filter: Option<Arc<String>>,
    ) -> Result<SyncState, Error> {
        let mut queries = vec![];
        if let Some(since) = since {
            queries.push(("since", since));
        }
        if let Some(filter) = filter {
            queries.push(("filter", filter));
        }

        let state = self
            .client
            .get(format!(
                "https://{}/_matrix/client/r0/sync",
                self.homeserver
            ))
            .query(&queries)
            .bearer_auth(&self.access_code)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        let mut state: SyncState = match tokio::task::spawn_blocking(move|| serde_json::from_str::<Value>(&state).and_then(|v| ijson::from_value::<SyncState>(&v))).await {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                panic!("oh no: {}", e);
            }
            Err(e) => {
                panic!("oh no: {}", e);
            }
        };

        if let Some(rooms) = &mut state.rooms {
            if let Some(join) = &mut rooms.join {
                for (id, joined) in join.iter_mut() {
                    joined.name = if let Some(v) = self.get_name(id).await {
                        Some(v)
                    } else {
                        joined.summary.get("m.heroes").map(|v| v.as_array().unwrap().iter().map(|v| v.as_string().unwrap().as_str()).collect::<Vec<&str>>().join(", ")).map(Arc::new)
                    }
                }
            }
        }

        Ok(state)
    }

    pub async fn get_room_messages(&self, room_id: &str, from: &str, dir: RoomDirection, to: Option<&String>, limit: Option<u64>, filter: Option<Arc<String>>) -> Result<RoomMessages, Error> {
        let dir = match dir {
            RoomDirection::Forwards => "f",
            RoomDirection::Backwards => "b",
        };

        let limit = match limit {
            Some(v) => format!("{}", v),
            None => String::from("10"),
        };
        let filter_ = match filter.as_ref() {
            Some(v) => v.as_str(),
            None => "",
        };
        let mut queries = vec![("from", from), ("dir", dir), ("limit", &limit), ("filter", filter_)];
        if let Some(to) = to {
            queries.push(("to", to));
        }

        let state = self
            .client
            .get(format!(
                "https://{}/_matrix/client/r0/rooms/{}/messages",
                self.homeserver,
                room_id,
            ))
            .query(&queries)
            .bearer_auth(&self.access_code)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        let state = match tokio::task::spawn_blocking(move|| serde_json::from_str::<Value>(&state).and_then(|v| ijson::from_value::<RoomMessages>(&v))).await {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                panic!("oh no: {}", e);
            }
            Err(e) => {
                panic!("oh no: {}", e);
            }
        };

        Ok(state)
    }

    pub async fn thumbnail_mxc(
        &self,
        server_name: &str,
        media_id: &str,
        width: u64,
        height: u64,
    ) -> Result<Content, Error> {
        let mut response = self
            .client
            .get(format!(
                "https://{}/_matrix/media/r0/thumbnail/{}/{}",
                self.homeserver, server_name, media_id,
            ))
            .query(&[("width", width), ("height", height)])
            .send()
            .await?
            .error_for_status()?;
        let mut content = Content {
            type_: Arc::new(String::from(
                response
                    .headers()
                    .get("Content-Type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or(""),
            )),
            disposition: Arc::new(String::from(
                response
                    .headers()
                    .get("Content-Disposition")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or(""),
            )),
            content: vec![],
        };

        while let Some(chunk) = response.chunk().await? {
            content.content.extend(chunk);
        }

        Ok(content)
    }
}
