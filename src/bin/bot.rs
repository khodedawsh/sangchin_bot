use std::{env, sync::Arc};

use mime::{Mime, APPLICATION_OCTET_STREAM};
use redis::AsyncCommands;
use teloxide::{
    prelude::*,
    types::{MediaAnimation, MediaDocument, MediaKind, MediaVideo, MessageKind, ReplyParameters},
};

#[derive(Debug)]
struct TelegramFile<'a> {
    id: &'a str,
    name: Option<&'a str>,
    unique_id: &'a str,
    mime: Option<Mime>,
    size: u32,
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();
    log::info!("Starting...");

    let bot = Bot::from_env();
    let redis_address =
        Arc::new(env::var("REDIS_ADDRESS").unwrap_or("redis://127.0.0.1".to_owned()));
    let web_server_address = Arc::new(
        env::var("WEBSERVER_ADDRESS").unwrap_or("http://127.0.0.1:3030/files/".to_owned()),
    );

    teloxide::repl(bot, move |bot: Bot, msg: Message| {
        let redis_address = Arc::clone(&redis_address);
        let web_server_address = Arc::clone(&web_server_address);
        async move {
            if let MessageKind::Common(ref common_message) = msg.kind {
                let file = if let Some(file) = match &common_message.media_kind {
                    MediaKind::Document(MediaDocument { document, .. }) => Some(TelegramFile {
                        id: &document.file.id,
                        name: document.file_name.as_deref(),
                        unique_id: &document.file.unique_id,
                        mime: document.mime_type.clone(),
                        size: document.file.size,
                    }),
                    MediaKind::Video(MediaVideo { video, .. }) => Some(TelegramFile {
                        id: &video.file.id,
                        name: video.file_name.as_deref(),
                        unique_id: &video.file.unique_id,
                        mime: video.mime_type.clone(),
                        size: video.file.size,
                    }),
                    MediaKind::Animation(MediaAnimation { animation, .. }) => Some(TelegramFile {
                        id: &animation.file.id,
                        name: animation.file_name.as_deref(),
                        unique_id: &animation.file.unique_id,
                        mime: animation.mime_type.clone(),
                        size: animation.file.size,
                    }),
                    _ => None,
                } {
                    file
                } else {
                    bot.send_message(msg.chat.id, "not a file! send me a file.")
                        .reply_parameters(ReplyParameters::new(msg.id))
                        .await?;
                    return Ok(());
                };

                let redis_client = redis::Client::open(redis_address.as_str()).unwrap();
                let mut con = match redis_client.get_multiplexed_async_connection().await {
                    Ok(con) => con,
                    Err(_) => {
                        bot.send_message(msg.chat.id, "server error!").await?;
                        return Ok(());
                    }
                };
                let file_id = &file.id;

                let fetched_file = bot.get_file(file_id.to_string()).await?;
                println!(
                    "unique id: {}, path: {}, mime: {}",
                    file.unique_id,
                    fetched_file.path,
                    file.mime.as_ref().unwrap().to_string()
                );

                let _: () = con
                    .hset_multiple(
                        "file_".to_owned() + &file.unique_id,
                        &[
                            ("path", &fetched_file.path),
                            (
                                "mime",
                                &file
                                    .mime
                                    .as_ref()
                                    .unwrap_or(&APPLICATION_OCTET_STREAM)
                                    .to_string(),
                            ),
                            (
                                "name",
                                &file.name.unwrap_or(&"unnamed".to_owned()).to_owned(),
                            ),
                            ("token", &bot.token().to_string()),
                            ("size", &file.size.to_string()),
                        ],
                    )
                    .await
                    .unwrap();
                bot.send_message(
                    msg.chat.id,
                    "".to_owned() + web_server_address.as_str() + &file.unique_id,
                )
                .reply_parameters(ReplyParameters::new(msg.id))
                .await?;
            }

            Ok(())
        }
    })
    .await;
}
