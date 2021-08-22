use std::fs;

use iced::{Application, Settings};
use tokio::sync::mpsc;

use uwutalk::markdown;
use uwutalk::chat::MatrixClient;
use uwutalk::chat_gui::Chat;

#[tokio::main]
async fn main() -> iced::Result {
    let markdown = markdown::parse_markdown("> uwu **test** *test2* ***test3*** `my code`
    > ```rs
    let (x, mut y) = (2, 3);
    y += 4;
    ```
    __underline__ and ~~strikethrough~~ and ||spoilers||
    # header 1
    ## header 2
    ### header 3

    - bullet point
        - bullet point
        ---
        [my **awesome** link](lauwa.xyz)
");
    println!("{:?}", markdown);

    let file = fs::read_to_string(".env").unwrap();
    let mut contents = file.split('\n');
    let access_token = contents.next().unwrap();
    let homeserver = contents.next().unwrap();

    let client = MatrixClient::new(homeserver, access_token);

    //let result = client.get_state(None).await.unwrap();
    //println!("{:#?}", result.rooms.join.iter().next().unwrap().1.timeline);

    let (tx, mut rx) = mpsc::channel(32);

    let manager = tokio::spawn(async move {
        use uwutalk::chat_gui::ClientMessage::*;

        while let Some(msg) = rx.recv().await {
            match msg {
                SendMessage(room_id, msg, resp) => {
                    let _ = resp.send(client.send_message(&room_id, &msg).await);
                }

                ClientSync(next_batch, filter, resp) => {
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
                    let _ = resp.send(client.get_state(next_batch, filter).await);
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
