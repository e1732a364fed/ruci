use axum::{extract::Path, http::StatusCode, response::Html, routing::get, Router};
use std::fs;
use tracing::info;

/// serve files in a folder named "static"
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

async fn download(Path(filename): Path<String>) -> impl axum::response::IntoResponse {
    let file_path = format!("static/{}", filename);

    //https://github.com/tokio-rs/axum/discussions/2759#discussioncomment-9622639

    match tokio::fs::File::open(&file_path).await {
        Ok(file) => {
            info!("download called, ok, {filename} ");

            let fl = file.metadata().await.unwrap().len();
            let stream = tokio_util::io::ReaderStream::new(file);
            let body = axum::body::Body::from_stream(stream);

            let s = format!("attachment; filename=\"{:?}\"", filename);

            let mut headers = axum::http::header::HeaderMap::new();
            headers.insert(
                axum::http::header::CONTENT_TYPE,
                "application/octet-stream".parse().unwrap(),
            );

            headers.insert(
                axum::http::header::CONTENT_LENGTH,
                fl.to_string().parse().unwrap(),
            );

            headers.insert(axum::http::header::CONTENT_DISPOSITION, s.parse().unwrap());

            Ok((headers, body))
        }
        Err(err) => {
            info!("download called, not found, {filename} ");
            return Err((StatusCode::NOT_FOUND, format!("File not found: {}", err)));
        }
    }
}

/// non-blocking, default is 0.0.0.0:18143
pub async fn serve_static(listen_addr: Option<String>) {
    let addr = listen_addr
        .clone()
        .unwrap_or_else(|| String::from("0.0.0.0:18143"));

    info!("serving folder {addr}");

    // RUST_LOG=tower_http=trace

    let app = Router::new()
        .route("/*folder", get(index)) // 任何一级请求都被视为对 static 下的子文件夹的目录的请求
        .route("/download/*filename", get(download));
    use tower_http::trace::TraceLayer;
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app.layer(TraceLayer::new_for_http()))
            .await
            .unwrap();
    });

    info!("served folder {addr}");
}
