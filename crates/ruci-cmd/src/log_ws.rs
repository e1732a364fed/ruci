use std::{io, sync::Arc};

use axum::{
    extract::ws::{Message, WebSocket},
    response::IntoResponse,
    routing::get,
    Router,
};
use futures::StreamExt;
use tokio::sync::broadcast;
use tracing::info;
use tracing_subscriber::fmt::MakeWriter;

const LOG_CHANNEL_SIZE: usize = 1024;
pub const DEFAULT_ADDR: &str = "127.0.0.1:40682";

pub struct WebsocketLogger {
    tx: broadcast::Sender<String>,
}

impl WebsocketLogger {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(LOG_CHANNEL_SIZE);
        Self { tx }
    }

    pub fn get_writer(&self) -> ArcWebsocketLogWriter {
        ArcWebsocketLogWriter(Arc::new(WebsocketLogWriter {
            tx: self.tx.clone(),
        }))
    }

    pub fn serve(self, addr: &str) {
        let app = Router::new().route("/ws/logs", get(move |ws| ws_handler(ws, self.tx.clone())));

        let addr = addr.to_string();
        tokio::spawn(async move {
            let listener = tokio::net::TcpListener::bind(&addr)
                .await
                .expect("Failed to bind websocket logger port");
            info!("Websocket logger listening on {}", addr);
            println!("Websocket logger listening on {}", addr);
            axum::serve(listener, app)
                .await
                .expect("Failed to start websocket logger server");
        });
    }
}

pub struct ArcWebsocketLogWriter(Arc<WebsocketLogWriter>);

impl<'a> MakeWriter<'a> for ArcWebsocketLogWriter {
    type Writer = WebsocketLogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        WebsocketLogWriter {
            tx: self.0.tx.clone(),
        }
    }
}

pub struct WebsocketLogWriter {
    tx: broadcast::Sender<String>,
}

impl<'a> MakeWriter<'a> for WebsocketLogWriter {
    type Writer = WebsocketLogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        WebsocketLogWriter {
            tx: self.tx.clone(),
        }
    }
}

impl io::Write for WebsocketLogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Ok(s) = String::from_utf8(buf.to_vec()) {
            let _ = self.tx.send(s);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

async fn ws_handler(
    ws: axum::extract::ws::WebSocketUpgrade,
    tx: broadcast::Sender<String>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, tx))
}

async fn handle_socket(socket: WebSocket, tx: broadcast::Sender<String>) {
    let mut rx = tx.subscribe();
    let (mut sender, _) = socket.split();
    use futures::SinkExt;
    while let Ok(msg) = rx.recv().await {
        if sender.send(Message::Text(msg.into())).await.is_err() {
            break;
        }
    }
}
