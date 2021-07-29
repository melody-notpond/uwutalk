use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;
use reqwest::{Client, Error};

pub struct MatrixClient {
    client: Client,
    homeserver: String,
    access_code: String
}

pub struct MatrixRoom {
    homeserver: String,
    id: String
}

#[derive(Deserialize, Debug)]
pub struct Event {
    pub event_id: String
}

#[derive(Debug, Deserialize)]
pub struct State {
    pub events: Vec<Value>,
}

#[derive(Debug, Deserialize)]
pub struct UnsignedData {
    pub age: i64,
    pub redacted_because: Option<Event>,
    pub transaction_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RoomEvent {
    pub content: Value,

    #[serde(rename = "type")]
    pub type_: String,
    pub event_id: String,
    pub sender: String,
    pub origin_server_ts: i64,
    pub unsigned: UnsignedData
}

#[derive(Debug, Deserialize)]
pub struct Timeline {
    pub events: Vec<RoomEvent>,
    pub limited: bool,
    pub prev_batch: String,
}

#[derive(Deserialize, Debug)]
pub struct Ephemeral {
    pub events: Vec<Value>,
}

#[derive(Deserialize, Debug)]
pub struct UnreadNotificationCounts {
    pub highlight_count: i64,
    pub notification_count: i64
}

#[derive(Deserialize, Debug)]
pub struct JoinedRoom {
    pub summary: HashMap<String, Value>,
    pub state: State,
    pub timeline: Timeline,
    pub ephemeral: Ephemeral,
    pub account_data: Value,
    pub unread_notifications: UnreadNotificationCounts,
}

#[derive(Deserialize, Debug)]
pub struct SyncRooms {
    pub join: HashMap<String, JoinedRoom>,
    pub invite: HashMap<String, Value>,
    pub leave: HashMap<String, Value>,
}

#[derive(Deserialize, Debug)]
pub struct SyncState {
    pub next_batch: String,
    pub rooms: SyncRooms,
    pub presence: serde_json::Value,
    pub account_data: serde_json::Value,
    pub to_device: serde_json::Value,
    pub device_lists: serde_json::Value,
    pub device_one_time_keys_count: serde_json::Value,
}

impl MatrixRoom {
    pub fn new(homeserver: &str, id: &str) -> MatrixRoom {
        MatrixRoom {
            homeserver: String::from(homeserver),
            id: String::from(id)
        }
    }
}

impl MatrixClient {
    pub fn new(homeserver: &str, access_code: &str) -> MatrixClient {
        MatrixClient {
            client: Client::new(),
            homeserver: String::from(homeserver),
            access_code: String::from(access_code)
        }
    }

    pub async fn send_message(&self, room: &MatrixRoom, content: &str) -> Result<Event, Error> {
        let event = self.client.post(format!("https://{}/_matrix/client/r0/rooms/{}:{}/send/m.room.message", self.homeserver, room.id, room.homeserver))
            .body(format!("{{\"msgtype\": \"m.text\", \"body\": {:?}}}", content))
            .bearer_auth(&self.access_code)
            .send().await?.text().await?;
        Ok(serde_json::from_str(&event).unwrap())
    }

    pub async fn get_state(&self) -> Result<SyncState, Error> {
        let state = self.client.get(format!(r#"https://{}/_matrix/client/r0/sync"#, self.homeserver))
            .query(&[("filter", r#"{"room":{"timeline":{"limit":1}}}"#)])
            .bearer_auth(&self.access_code)
            .send().await?.text().await?;
        Ok(serde_json::from_str(&state).unwrap())
    }
}
