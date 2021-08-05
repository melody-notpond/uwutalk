use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;
use reqwest::{Client, Error};

pub struct MatrixClient {
    client: Client,
    homeserver: String,
    access_code: String
}

#[derive(Deserialize, Debug, Clone)]
pub struct Event {
    pub event_id: String
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
    pub age: i64,
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
    pub unsigned: UnsignedData
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
    pub notification_count: i64
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
    pub join: HashMap<String, JoinedRoom>,
    pub invite: HashMap<String, Value>,
    pub leave: HashMap<String, Value>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SyncState {
    pub next_batch: String,
    pub rooms: SyncRooms,
    pub presence: serde_json::Value,
    pub account_data: serde_json::Value,
    pub to_device: serde_json::Value,
    pub device_lists: serde_json::Value,
    pub device_one_time_keys_count: serde_json::Value,
}

impl MatrixClient {
    pub fn new(homeserver: &str, access_code: &str) -> MatrixClient {
        MatrixClient {
            client: Client::new(),
            homeserver: String::from(homeserver),
            access_code: String::from(access_code)
        }
    }

    pub async fn send_message(&self, room: &str, content: &str) -> Result<Event, Error> {
        let event = self.client.post(format!("https://{}/_matrix/client/r0/rooms/{}/send/m.room.message", self.homeserver, room))
            .body(format!("{{\"msgtype\": \"m.text\", \"body\": {:?}}}", content))
            .bearer_auth(&self.access_code)
            .send().await?.text().await?;
        Ok(serde_json::from_str(&event).unwrap())
    }

    async fn get_name(&self, room: &str) -> Option<String> {
        let name = self.client.get(format!("https://{}/_matrix/client/r0/rooms/{}/state/m.room.name", self.homeserver, room))
            .bearer_auth(&self.access_code)
            .send().await.ok()?;
        if name.status() == 200 {
            Some(String::from(serde_json::from_str::<Value>(&name.text().await.ok()?).ok()?["name"].as_str()?))
        } else {
            let name = self.client.get(format!("https://{}/_matrix/client/r0/rooms/{}/state/m.room.canonical_alias", self.homeserver, room))
                .bearer_auth(&self.access_code)
                .send().await.ok()?;

            if name.status() == 200 {
                Some(String::from(serde_json::from_str::<Value>(&name.text().await.ok()?).ok()?["alias"].as_str()?))
            } else {
                None
            }
        }
    }

    pub async fn get_state(&self, since: Option<String>) -> Result<SyncState, Error> {
        let mut queries = vec![];
        if let Some(since) = since {
            queries.push(("since", since));
        }

        let state = self.client.get(format!("https://{}/_matrix/client/r0/sync", self.homeserver))
            //.query(&[("filter", r#"{"room":{"timeline":{"limit":1}}}"#)])
            .query(&queries)
            .bearer_auth(&self.access_code)
            .send().await?.text().await?;
        let mut state: SyncState = serde_json::from_str(&state).unwrap();

        for (id, joined) in state.rooms.join.iter_mut() {
            joined.name = self.get_name(id).await;
        }

        Ok(state)
    }
}
