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

async fn proxy_ws(mut client: WebSocket, upstream_url: &str) {
    let (ws_stream, _) = match tokio_tungstenite::connect_async(upstream_url).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("WS proxy: failed to connect to {upstream_url}: {e}");
            return;
        }
    };

    // Use channels to bridge the two WebSocket connections without split().
    // split() creates BiLock-based halves that can cause contention/deadlocks
    // when both directions are active simultaneously.
    let (client_out_tx, mut client_out_rx) = tokio::sync::mpsc::channel::<Message>(256);
    let (upstream_out_tx, mut upstream_out_rx) =
        tokio::sync::mpsc::channel::<TungsteniteMessage>(256);

    // Handle the upstream (tungstenite) WebSocket in a single task
    let upstream_handle = tokio::spawn(async move {
        let (mut upstream_tx, mut upstream_rx) = ws_stream.split();

        // Spawn a writer for the upstream side
        let write_task = tokio::spawn(async move {
            while let Some(msg) = upstream_out_rx.recv().await {
                if upstream_tx.send(msg).await.is_err() {
                    break;
                }
            }
        });

        // Read from upstream and forward to client channel
        while let Some(Ok(msg)) = upstream_rx.next().await {
            let Some(axum_msg) = tungstenite_to_axum(msg) else {
                break;
            };
            if client_out_tx.send(axum_msg).await.is_err() {
                break;
            }
        }

        write_task.abort();
    });

    // Handle the axum WebSocket in the current task (no split needed)
    // Use a simple loop that alternates between reading and writing
    loop {
        tokio::select! {
            biased;

            // Prioritize reading from client to avoid blocking
            msg = client.recv() => {
                match msg {
                    Some(Ok(msg)) => {
                        let Some(tung_msg) = axum_to_tungstenite(msg) else { break };
                        if upstream_out_tx.send(tung_msg).await.is_err() { break }
                    }
                    _ => break,
                }
            }

            // Forward messages from upstream to client
            Some(msg) = client_out_rx.recv() => {
                if client.send(msg).await.is_err() { break }
            }
        }
    }

    upstream_handle.abort();
}
