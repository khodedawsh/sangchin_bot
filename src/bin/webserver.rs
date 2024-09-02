use redis::AsyncCommands;
use reqwest::Client;
use warp::http::header;

use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use warp::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use warp::hyper::{Body, Response};
use warp::reject::Rejection;
use warp::Filter;

#[derive(Debug)]
struct FileNotFoundError {}

impl warp::reject::Reject for FileNotFoundError {}

#[derive(Debug)]
struct ServerError {
    message: String,
}

impl warp::reject::Reject for ServerError {}

#[tokio::main]
async fn main() {
    let redis_address = Arc::new(env::var("REDIS_ADDRESS").unwrap_or("redis://127.0.0.1".to_owned()));

    let file_server = warp::path!("file" / String)
        .map(move |file_name: String| {
            let ra = Arc::clone(&redis_address);
            (file_name, ra)
        })
        .and_then(
            |(file_name, redis_address): (String, Arc<String>)| async move {
                let shared_address = Arc::clone(&redis_address);
                // redis connection and finding the file
                let client = Client::new();
                let redis_client =
                    redis::Client::open(shared_address.as_str()).expect("redis url invalid");
                let mut con = redis_client
                    .get_multiplexed_async_connection()
                    .await
                    .map_err(|_e| {
                        warp::reject::custom(ServerError {
                            message: "db connection failed".to_owned(),
                        })
                    })?;

                let values: HashMap<String, String> = con
                    .hgetall("file_".to_string() + &file_name)
                    .await
                    .map_err(|_e| warp::reject::custom(FileNotFoundError {}))?;

                let path: String = con
                    .hget("file_".to_string() + &file_name, "path")
                    .await
                    .map_err(|_e| warp::reject::custom(FileNotFoundError {}))?;

                // getting file info
                let token = values.get("token").ok_or(FileNotFoundError {})?;
                let mime_type = values.get("mime").ok_or(FileNotFoundError {})?;
                let real_file_name = values.get("name").ok_or(FileNotFoundError {})?;

                // sending the request to telegram
                let url = "https://api.telegram.org/file/bot".to_owned() + &token + "/" + &path;
                let req = client.get(&url);

                let res = req.send().await.map_err(|e| {
                    eprintln!("Request failed: {:?}", e);
                    FileNotFoundError {}
                })?;

                // sending over the file
                let warp_response = Response::builder()
                    .status(res.status().as_u16())
                    .header(CONTENT_TYPE, mime_type)
                    .header(
                        CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{}\"", real_file_name),
                    )
                    .body(Body::wrap_stream(res.bytes_stream()))
                    .unwrap();
                Ok::<_, Rejection>(warp_response)
            },
        )
        .recover(handle_rejection);

    warp::serve(file_server).run(([127, 0, 0, 1], 3030)).await;
}

async fn handle_rejection(err: warp::Rejection) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(e) = err.find::<ServerError>() {
        return Ok(warp::reply::with_header(
            format!("server error: {}", e.message),
            header::CONTENT_TYPE,
            "text/html",
        ));
    } else if let Some(_e) = err.find::<FileNotFoundError>() {
        return Ok(warp::reply::with_header(
            "file not found".to_owned(),
            header::CONTENT_TYPE,
            "text/html",
        ));
    }

    Ok(warp::reply::with_header(
        "nothing here".to_owned(),
        header::CONTENT_TYPE,
        "text/html",
    ))
}
