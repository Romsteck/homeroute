//! WebSocket proxy: relays an axum WebSocket to an upstream tokio-tungstenite connection.
//! Used in thin-shell mode to forward agent/host/terminal WS to hr-orchestrator.

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message as TungsteniteMessage;

const ORCHESTRATOR_HOST: &str = "127.0.0.1:4001";

/// Proxy an axum WebSocket to an upstream path on hr-orchestrator (port 4001).
pub async fn proxy_ws_to_orchestrator(client: WebSocket, path: &str) {
    let url = format!("ws://{ORCHESTRATOR_HOST}{path}");
    proxy_ws(client, &url).await;
}

/// Convert axum Message -> tungstenite Message.
/// Both use Utf8Bytes/Bytes but they are distinct types, so we go through String/Vec<u8>.
fn axum_to_tungstenite(msg: Message) -> Option<TungsteniteMessage> {
    Some(match msg {
        Message::Text(t) => TungsteniteMessage::Text(t.to_string().into()),
        Message::Binary(b) => TungsteniteMessage::Binary(b.to_vec().into()),
        Message::Ping(p) => TungsteniteMessage::Ping(p.to_vec().into()),
        Message::Pong(p) => TungsteniteMessage::Pong(p.to_vec().into()),
        Message::Close(_) => return None,
    })
}

/// Convert tungstenite Message -> axum Message.
fn tungstenite_to_axum(msg: TungsteniteMessage) -> Option<Message> {
    Some(match msg {
        TungsteniteMessage::Text(t) => Message::Text(t.to_string().into()),
        TungsteniteMessage::Binary(b) => Message::Binary(b.to_vec().into()),
        TungsteniteMessage::Ping(p) => Message::Ping(p.to_vec().into()),
        TungsteniteMessage::Pong(p) => Message::Pong(p.to_vec().into()),
        TungsteniteMessage::Close(_) => return None,
        TungsteniteMessage::Frame(_) => return None,
    })
}

async fn proxy_ws(client: WebSocket, upstream_url: &str) {
    // Split the axum WebSocket into separate read/write halves
    let (mut client_tx, mut client_rx) = client.split();

    let (ws_stream, _) = match tokio_tungstenite::connect_async(upstream_url).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("WS proxy: failed to connect to {upstream_url}: {e}");
            let _ = client_tx.send(Message::Close(None)).await;
            return;
        }
    };

    let (mut upstream_tx, mut upstream_rx) = ws_stream.split();

    let client_to_upstream = async {
        while let Some(Ok(msg)) = client_rx.next().await {
            let Some(tung_msg) = axum_to_tungstenite(msg) else {
                let _ = upstream_tx.send(TungsteniteMessage::Close(None)).await;
                break;
            };
            if upstream_tx.send(tung_msg).await.is_err() {
                break;
            }
        }
    };

    let upstream_to_client = async {
        while let Some(Ok(msg)) = upstream_rx.next().await {
            let Some(axum_msg) = tungstenite_to_axum(msg) else {
                break;
            };
            if client_tx.send(axum_msg).await.is_err() {
                break;
            }
        }
    };

    tokio::select! {
        _ = client_to_upstream => {}
        _ = upstream_to_client => {}
    }
}
