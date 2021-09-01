use std::fs;
use std::path::Path;

use druid::{AppLauncher, ImageBuf, Target, WindowDesc};
use tokio::sync::mpsc;

use directories::ProjectDirs;
use uwutalk::chat::MatrixClient;
use uwutalk::chat_gui::{self, Chat};

#[tokio::main]
async fn main() {
    let project = ProjectDirs::from("xyz", "lauwa", "uwutalk")
        .expect("project directories must exist for uwutalk to function");
    let cache = project.cache_dir();
    match fs::create_dir_all(&cache) {
        Ok(_) => (),
        Err(e) => {
            eprintln!("error creating cache directory: {:?}", e);
            std::process::exit(-1);
        }
    }

    let thumbnails = cache.join("thumbnails");
    match fs::create_dir_all(&thumbnails) {
        Ok(_) => (),
        Err(e) => {
            eprintln!("error creating thumbnails directory: {:?}", e);
            std::process::exit(-1);
        }
    }

    let file = fs::read_to_string(".env").unwrap();
    let mut contents = file.split('\n');
    let access_token = contents.next().unwrap();
    let homeserver = contents.next().unwrap();

    let client = MatrixClient::new(homeserver, access_token);

    //let result = client.get_state(None).await.unwrap();
    //println!("{:#?}", result.rooms.join.iter().next().unwrap().1.timeline);

    let launcher =
        AppLauncher::with_window(WindowDesc::new(chat_gui::build_ui()).window_size((800., 600.)));

    let (sync_tx, mut rx) = mpsc::channel(32);
    let event_sink = launcher.get_external_handle();

    let sync = tokio::spawn(async move {
        use uwutalk::chat_gui::Syncing::*;

        while let Some(msg) = rx.recv().await {
            match msg {
                Quit => break,

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

                    match client.get_state(next_batch, filter).await {
                        Ok(v) => {
                            if event_sink
                                .submit_command(chat_gui::SYNC, v, Target::Global)
                                .is_err()
                            {
                                break;
                            }
                        }

                        Err(e) => {
                            if event_sink
                                .submit_command(chat_gui::SYNC_FAIL, e, Target::Global)
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
            }
        }
    });

    let client = MatrixClient::new(homeserver, access_token);
    let (action_tx, mut rx) = mpsc::channel(32);
    //let event_sink = launcher.get_external_handle();

    let action = tokio::spawn(async move {
        use uwutalk::chat_gui::UserAction::*;

        while let Some(msg) = rx.recv().await {
            match msg {
                Quit => break,

                SendMessage(room_id, msg, formatted) => {
                    let formatted = if formatted == msg {
                        None
                    } else {
                        Some(formatted)
                    };

                    // TODO: error on send
                    let _ = client
                        .send_message(&room_id, &msg, formatted.as_ref())
                        .await;
                }

                EditMessage(room_id, event_id, msg, formatted) => {
                    let formatted = if formatted == msg {
                        None
                    } else {
                        Some(formatted)
                    };

                    // TODO: error on send
                    let _ = client
                        .edit_message(&room_id, &event_id, &msg, formatted.as_ref())
                        .await;
                }
            }
        }
    });

    let client = MatrixClient::new(homeserver, access_token);
    let (media_tx, mut rx) = mpsc::channel(32);
    let event_sink = launcher.get_external_handle();

    let media = tokio::spawn(async move {
        use uwutalk::chat_gui::MediaFetch::*;

        while let Some(msg) = rx.recv().await {
            match msg {
                Quit => break,

                FetchThumbnail(url, widget, width, height) => {
                    if let Some(url) = url.strip_prefix("mxc://") {
                        let mut split = url.split('/');
                        let server = split.next().unwrap_or("");
                        let media = split.next().unwrap_or("");

                        let mut thumbnails_dir = match thumbnails.read_dir() {
                            Ok(v) => v,
                            Err(e) => {
                                eprintln!("error reading cache directory: {:?}", e);
                                std::process::exit(-1);
                            }
                        };
                        let mut name = String::new();
                        name.push_str(server);
                        name.push('%');
                        name.push_str(media);
                        let content = if let Some(thumbnail) = thumbnails_dir.find(|v| match v {
                            Ok(v) => {
                                let filename = v.file_name();
                                let s = Path::new(&filename).to_str().unwrap();
                                s == name
                            }

                            Err(_) => false,
                        }) {
                            match fs::read(thumbnail.unwrap().path()) {
                                Ok(v) => Some(v),
                                Err(e) => {
                                    eprintln!("error reading cached thumbnail: {:?}", e);
                                    None
                                }
                            }
                        } else {
                            None
                        };

                        let content = match content {
                            Some(v) => v,
                            None => {
                                match client.thumbnail_mxc(server, media, width, height).await {
                                    Ok(v) => {
                                        let content = v.content;
                                        let path = thumbnails.join(name);
                                        match fs::write(path, &content) {
                                            Ok(_) => (),
                                            Err(e) => {
                                                eprintln!("error writing cache: {:?}", e);
                                            }
                                        }
                                        content
                                    }

                                    Err(e) => {
                                        if event_sink
                                            .submit_command(
                                                chat_gui::FETCH_THUMBNAIL_FAIL,
                                                e,
                                                Target::Widget(widget),
                                            )
                                            .is_err()
                                        {
                                            break;
                                        }
                                        continue;
                                    }
                                }
                            }
                        };

                        match ImageBuf::from_data(&content) {
                            Ok(v) => {
                                if event_sink
                                    .submit_command(
                                        chat_gui::FETCH_THUMBNAIL,
                                        v,
                                        Target::Widget(widget),
                                    )
                                    .is_err()
                                {
                                    break;
                                }
                            }

                            Err(e) => eprintln!("error loading image: {:?}", e),
                        }
                    }
                }
            }
        }
    });

    launcher.launch(Chat::new(sync_tx, action_tx, media_tx)).unwrap();
    sync.await.unwrap();
    action.await.unwrap();
    media.await.unwrap();
}
