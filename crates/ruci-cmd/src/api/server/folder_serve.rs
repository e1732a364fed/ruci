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
                if path.is_dir() {
                    body.push_str(&format!(
                        "<a href=\"{folder}/{filename}\">{filename}/</a><br>",
                    ));
                } else if path.is_file() {
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

/// non-blocking
pub async fn serve_static() {
    let addr = "0.0.0.0:6000";
    info!("serving folder {addr}");

    // RUST_LOG=tower_http=trace

    let app = Router::new()
        .route("/*folder", get(index))
        .route("/download/*filename", get(download));
    use tower_http::trace::TraceLayer;
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app.layer(TraceLayer::new_for_http()))
            .await
            .unwrap();
    });

    info!("served folder {addr}");
}
