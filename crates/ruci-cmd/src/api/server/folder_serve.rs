use axum::{extract::Path, http::StatusCode, response::Html, routing::get, Router};
use log::info;
use std::fs;

async fn index(Path(folder): Path<String>) -> Html<String> {
    info!("index called, {folder} ");

    let folder_path = format!("static/{}", folder);
    let mut body = String::new();

    if let Ok(entries) = fs::read_dir(&folder_path) {
        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                let filename = path.file_name().unwrap().to_string_lossy();
                let is_dir = path.is_dir();
                if is_dir {
                    body.push_str(&format!(
                        "<a href=\"{folder}/{filename}\">{filename}/</a><br>",
                    ));
                } else {
                    body.push_str(&format!(
                        "<a href=\"/download/{folder}/{filename}\">{filename}</a><br>",
                    ));
                }
            }
        }
    }

    Html(format!(
        "<!DOCTYPE html><html><head><title>File Server</title></head><body>{}</body></html>",
        body
    ))
}

async fn download(Path(filename): Path<String>) -> Result<Vec<u8>, StatusCode> {
    let file_path = format!("static/{}", filename);

    if let Ok(data) = fs::read(&file_path) {
        Ok(data)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

pub async fn serve_static() {
    info!("serving folder");

    // RUST_LOG=tower_http=trace

    let app = Router::new()
        .route("/*folder", get(index))
        .route("/download/*filename", get(download));
    use tower_http::trace::TraceLayer;
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();

    axum::serve(listener, app.layer(TraceLayer::new_for_http()))
        .await
        .unwrap();

    info!("served folder");
}
