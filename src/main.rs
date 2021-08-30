use std::fs;

use druid::{AppLauncher, Target, WindowDesc};
use tokio::sync::mpsc;

use uwutalk::chat::MatrixClient;
use uwutalk::chat_gui::{self, Chat};

#[tokio::main]
async fn main() {
    let file = fs::read_to_string(".env").unwrap();
    let mut contents = file.split('\n');
    let access_token = contents.next().unwrap();
    let homeserver = contents.next().unwrap();

    let client = MatrixClient::new(homeserver, access_token);

    //let result = client.get_state(None).await.unwrap();
    //println!("{:#?}", result.rooms.join.iter().next().unwrap().1.timeline);

    let launcher =
        AppLauncher::with_window(WindowDesc::new(chat_gui::build_ui()).window_size((800., 600.)));

    let (tx, mut rx) = mpsc::channel(32);
    let event_sink = launcher.get_external_handle();

    let manager = tokio::spawn(async move {
        use uwutalk::chat_gui::ClientMessage::*;

        while let Some(msg) = rx.recv().await {
            match msg {
                Quit => break,

                SendMessage(room_id, msg, formatted) => {
                    let formatted = if formatted == msg {
                        None
                    } else {
                        Some(formatted)
                    };
                    let _ = client
                        .send_message(&room_id, &msg, formatted.as_ref())
                        .await;
                }

                ClientSync(next_batch, filter) => {
                    let next_batch = if next_batch.is_empty() {
                        None
                    } else {
                        Some(next_batch)
                    };
                    let filter = if filter.is_empty() {
                        None
                    } else {
                        Some(filter)
                    };

                    if event_sink
                        .submit_command(
                            chat_gui::SYNC,
                            client.get_state(next_batch, filter).await.unwrap(),
                            Target::Global,
                        )
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });

    launcher.launch(Chat::new(tx)).unwrap();
    manager.await.unwrap();
}
