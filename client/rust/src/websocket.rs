/// WebSocket message handler for real-time communication

use crate::error::Result;
use crate::models::MlsMessageEnvelope;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Serialize)]
struct SubscribeMessage {
    action: String,
    group_id: String,
}

#[derive(Serialize)]
struct SendMessage {
    action: String,
    group_id: String,
    encrypted_content: String,
}

/// Incoming message envelope from WebSocket (discriminated by type)
pub type IncomingMessageEnvelope = MlsMessageEnvelope;

#[derive(Deserialize)]
pub struct IncomingMessage {
    pub sender: String,
    pub group_id: String,
    pub encrypted_content: String,
}

/// WebSocket message handler
pub struct MessageHandler {
    sender: futures::channel::mpsc::UnboundedSender<Message>,
    receiver: futures::channel::mpsc::UnboundedReceiver<Message>,
}

impl MessageHandler {
    /// Connect to the server WebSocket
    pub async fn connect(server_url: &str, username: &str) -> Result<Self> {
        let url = format!("ws://{}/ws/{}", server_url, username);
        
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

    /// Send a message to a group
    pub async fn send_message(&self, group_id: &str, encrypted_content: &str) -> Result<()> {
        let message = SendMessage {
            action: "message".to_string(),
            group_id: group_id.to_string(),
            encrypted_content: encrypted_content.to_string(),
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

    /// Get the next incoming message
    pub async fn next_message(&mut self) -> Result<Option<IncomingMessage>> {
        if let Some(msg) = self.receiver.next().await {
            match msg {
                Message::Text(text) => {
                    let incoming: IncomingMessage = serde_json::from_str(&text)?;
                    Ok(Some(incoming))
                }
                Message::Close(_) => Ok(None),
                _ => Ok(None),
            }
        } else {
            Ok(None)
        }
    }

    /// Get the next incoming message envelope (supports discriminated MLS messages)
    pub async fn next_envelope(&mut self) -> Result<Option<IncomingMessageEnvelope>> {
        if let Some(msg) = self.receiver.next().await {
            match msg {
                Message::Text(text) => {
                    let incoming: IncomingMessageEnvelope = serde_json::from_str(&text)?;
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
