use reqwest::{Client, Error, Response};

pub struct MatrixClient {
    client: Client,
    access_code: String
}

pub struct MatrixRoom {
    homeserver: String,
    id: String
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
    pub fn new(access_code: &str) -> MatrixClient {
        MatrixClient {
            client: Client::new(),
            access_code: String::from(access_code)
        }
    }

    pub async fn send_message(&self, room: &MatrixRoom, content: &str) -> Result<Response, Error> {
        self.client.post(format!("https://{}/_matrix/client/r0/rooms/{}:{}/send/m.room.message", room.homeserver, room.id, room.homeserver))
            .body(format!("{{\"msgtype\": \"m.text\", \"body\": {:?}}}", content))
            .bearer_auth(&self.access_code)
            .send().await
    }
}
