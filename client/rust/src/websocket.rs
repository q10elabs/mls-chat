/// WebSocket message handler for real-time communication

use crate::error::Result;
use crate::models::MlsMessageEnvelope;
use futures::{SinkExt, StreamExt};
use serde::Serialize;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Serialize)]
struct SubscribeMessage {
    action: String,
    group_id: String,
}

/// WebSocket message handler
pub struct MessageHandler {
    sender: futures::channel::mpsc::UnboundedSender<Message>,
    receiver: futures::channel::mpsc::UnboundedReceiver<Message>,
}

impl MessageHandler {
    /// Connect to the server WebSocket
    pub async fn connect(server_url: &str, username: &str) -> Result<Self> {
        // Extract host and port from HTTP URL
        let url = if server_url.starts_with("http://") {
            format!("ws://{}/ws/{}", &server_url[7..], username)
        } else if server_url.starts_with("https://") {
            format!("wss://{}/ws/{}", &server_url[8..], username)
        } else {
            format!("ws://{}/ws/{}", server_url, username)
        };

        let (ws_stream, _) = connect_async(&url).await?;
        let (mut write, read) = ws_stream.split();

        let (tx, mut rx) = futures::channel::mpsc::unbounded::<Message>();
        let (tx_out, rx_out) = futures::channel::mpsc::unbounded::<Message>();

        // Spawn task to handle outgoing messages
        tokio::spawn(async move {
            while let Some(msg) = rx.next().await {
                if let Err(e) = write.send(msg).await {
                    log::error!("Failed to send WebSocket message: {}", e);
                    break;
                }
            }
        });

        // Spawn task to handle incoming messages
        tokio::spawn(async move {
            let mut read = read;
            while let Some(msg) = read.next().await {
                if let Ok(msg) = msg {
                    if let Err(_) = tx_out.unbounded_send(msg) {
                        log::error!("Failed to forward incoming message");
                        break;
                    }
                }
            }
        });

        Ok(Self {
            sender: tx,
            receiver: rx_out,
        })
    }

    /// Subscribe to a group
    pub async fn subscribe_to_group(&self, group_id: &str) -> Result<()> {
        let message = SubscribeMessage {
            action: "subscribe".to_string(),
            group_id: group_id.to_string(),
        };

        let json = serde_json::to_string(&message)?;
        let ws_message = Message::Text(json.into());

        self.sender.unbounded_send(ws_message)?;
        Ok(())
    }

    /// Send an MLS message envelope (application, welcome, or commit)
    pub async fn send_envelope(&self, envelope: &MlsMessageEnvelope) -> Result<()> {
        let json = serde_json::to_string(envelope)?;
        let ws_message = Message::Text(json.into());
        self.sender.unbounded_send(ws_message)?;
        Ok(())
    }

    /// Get the next incoming message envelope (supports discriminated MLS messages)
    ///
    /// Returns type-safe MLS message envelopes that can be pattern-matched to determine
    /// message type (ApplicationMessage, WelcomeMessage, or CommitMessage).
    pub async fn next_envelope(&mut self) -> Result<Option<MlsMessageEnvelope>> {
        if let Some(msg) = self.receiver.next().await {
            match msg {
                Message::Text(text) => {
                    let incoming: MlsMessageEnvelope = serde_json::from_str(&text)?;
                    Ok(Some(incoming))
                }
                Message::Close(_) => Ok(None),
                _ => Ok(None),
            }
        } else {
            Ok(None)
        }
    }
}
