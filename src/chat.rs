use std::collections::HashMap;

use reqwest::{Client, Error};
use serde::Deserialize;
use serde_json::{json, Value};

pub struct MatrixClient {
    client: Client,
    homeserver: String,
    access_code: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Event {
    pub event_id: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct StateEvent {
    pub content: Value,

    #[serde(rename = "type")]
    pub type_: String,

    pub event_id: String,
    pub sender: String,
    pub origin_server_ts: u64,
    pub unsigned: UnsignedData,
    pub prev_content: Option<Value>,
    pub state_key: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct State {
    pub events: Vec<StateEvent>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct UnsignedData {
    pub age: Option<i64>,
    pub redacted_because: Option<Event>,
    pub transaction_id: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RoomEvent {
    pub content: Value,

    #[serde(rename = "type")]
    pub type_: String,
    pub event_id: String,
    pub sender: String,
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
    pub name: Option<String>,
    pub summary: HashMap<String, Value>,
    pub state: State,
    pub timeline: Timeline,
    pub ephemeral: Ephemeral,
    pub account_data: Value,
    pub unread_notifications: UnreadNotificationCounts,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SyncRooms {
    pub join: Option<HashMap<String, JoinedRoom>>,
    pub invite: Option<HashMap<String, Value>>,
    pub leave: Option<HashMap<String, Value>>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SyncState {
    pub next_batch: String,
    pub rooms: Option<SyncRooms>,
    pub presence: Option<serde_json::Value>,
    pub account_data: Option<serde_json::Value>,
    pub to_device: Option<serde_json::Value>,
    pub device_lists: Option<serde_json::Value>,
    pub device_one_time_keys_count: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct Content {
    pub type_: String,
    pub disposition: String,
    pub content: Vec<u8>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RoomMessages {
    pub start: String,
    pub end: String,
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
        formatted: Option<&String>,
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
        Ok(serde_json::from_str(&event).unwrap())
    }

    pub async fn edit_message(
        &self,
        room: &str,
        event_id: &str,
        content: &str,
        formatted: Option<&String>,
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
        Ok(serde_json::from_str(&event).unwrap())
    }

    async fn get_name(&self, room: &str) -> Option<String> {
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
            Some(String::from(
                serde_json::from_str::<Value>(&name.text().await.ok()?).ok()?["name"].as_str()?,
            ))
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
                Some(String::from(
                    serde_json::from_str::<Value>(&name.text().await.ok()?).ok()?["alias"]
                        .as_str()?,
                ))
            } else {
                None
            }
        }
    }

    pub async fn get_state(
        &self,
        since: Option<String>,
        filter: Option<String>,
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

        let mut state: SyncState = match tokio::task::spawn_blocking(move|| serde_json::from_str(&state)).await {
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
                    joined.name = self.get_name(id).await;
                }
            }
        }

        Ok(state)
    }

    pub async fn get_room_messages(&self, room_id: &str, from: &str, dir: RoomDirection, to: Option<&String>, limit: Option<u64>, filter: Option<&String>) -> Result<RoomMessages, Error> {
        let dir = match dir {
            RoomDirection::Forwards => "f",
            RoomDirection::Backwards => "b",
        };

        let limit = match limit {
            Some(v) => format!("{}", v),
            None => String::from("10"),
        };
        let filter = match filter {
            Some(v) => v.as_str(),
            None => "",
        };
        let mut queries = vec![("from", from), ("dir", dir), ("limit", &limit), ("filter", filter)];
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

        let state = match tokio::task::spawn_blocking(move|| serde_json::from_str(&state)).await {
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
            type_: String::from(
                response
                    .headers()
                    .get("Content-Type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or(""),
            ),
            disposition: String::from(
                response
                    .headers()
                    .get("Content-Disposition")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or(""),
            ),
            content: vec![],
        };

        while let Some(chunk) = response.chunk().await? {
            content.content.extend(chunk);
        }

        Ok(content)
    }
}
