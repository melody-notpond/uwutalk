use std::fs;

use iced::{Application, Settings};
use tokio::sync::mpsc;

use uwutalk::chat::{MatrixClient, MatrixRoom};
use uwutalk::chat_gui::Chat;

#[tokio::main]
async fn main() -> iced::Result {
    let file = fs::read_to_string(".env").unwrap();
    let mut contents = file.split('\n');
    let access_token = contents.next().unwrap();
    let homeserver = contents.next().unwrap();
    let room = contents.next().unwrap();

    let room = MatrixRoom::new(homeserver, room);
    let client = MatrixClient::new(homeserver, access_token);

    let result = client.get_state().await.unwrap();
    println!("{:#?}", result.rooms.join.iter().next().unwrap().1.timeline);

    let (tx, mut rx) = mpsc::channel(32);

    let manager = tokio::spawn(async move {
        use uwutalk::chat_gui::ClientMessage::*;

        while let Some(msg) = rx.recv().await {
            match msg {
                SendMessage(msg, resp) => {
                    let _ = resp.send(client.send_message(&room, &msg).await);
                }
            }
        }
    });

    Chat::run(Settings {
        window: iced::window::Settings {
            size: (800, 600),
            min_size: Some((400, 300)),
            max_size: None,
            resizable: true,
            decorations: true,
            transparent: false,
            always_on_top: false,
            icon: None,
        },
        flags: tx,
        default_font: None,
        default_text_size: 16,
        exit_on_close_request: true,
        antialiasing: true,
    })?;

    manager.await.unwrap();
    Ok(())
}
