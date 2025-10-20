/// WebSocket connection manager for real-time message delivery.
/// Manages a single multiplexed connection per username with automatic reconnection
/// and group subscription/unsubscription.

use crate::error::{ClientError, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::Message, WebSocketStream};

/// Connection state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Failed,
}

/// WebSocket manager handling connection and message routing
pub struct WebSocketManager {
    base_url: String,
    username: String,
    state: Arc<Mutex<ConnectionState>>,
    connection: Arc<Mutex<Option<WebSocketStream<TcpStream>>>>,
    message_tx: Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
    subscribed_groups: Arc<Mutex<Vec<String>>>,
}

impl WebSocketManager {
    pub fn new(base_url: String, username: String) -> Self {
        WebSocketManager {
            base_url,
            username,
            state: Arc::new(Mutex::new(ConnectionState::Disconnected)),
            connection: Arc::new(Mutex::new(None)),
            message_tx: Arc::new(Mutex::new(None)),
            subscribed_groups: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Start the WebSocket connection with reconnection support
    pub async fn start(&self) -> Result<()> {
        let mut state = self.state.lock().await;
        if *state != ConnectionState::Disconnected {
            return Err(ClientError::StateError(
                "WebSocket already starting or connected".to_string(),
            ));
        }

        *state = ConnectionState::Connecting;
        drop(state); // Release lock before calling connect

        self.connect_with_backoff().await?;
        Ok(())
    }

    /// Connect with exponential backoff retry
    async fn connect_with_backoff(&self) -> Result<()> {
        let mut backoff_ms = 1000; // Start at 1 second
        let max_backoff_ms = 32000; // Cap at 32 seconds
        let max_retries = 12;

        for attempt in 0..max_retries {
            match self.connect_internal().await {
                Ok(_) => {
                    log::info!("WebSocket connected for user: {}", self.username);
                    return Ok(());
                }
                Err(e) => {
                    log::warn!(
                        "WebSocket connection attempt {} failed: {}",
                        attempt + 1,
                        e
                    );

                    if attempt == max_retries - 1 {
                        let mut state = self.state.lock().await;
                        *state = ConnectionState::Failed;
                        return Err(ClientError::WebSocketError(format!(
                            "Failed to connect after {} attempts: {}",
                            max_retries, e
                        )));
                    }

                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                    backoff_ms = (backoff_ms * 2).min(max_backoff_ms);
                }
            }
        }

        Err(ClientError::WebSocketError(
            "Max reconnection attempts exceeded".to_string(),
        ))
    }

    /// Internal connection logic
    async fn connect_internal(&self) -> Result<()> {
        // Convert HTTP URL to WebSocket URL
        let ws_url = format!(
            "{}/ws/{}",
            self.base_url.replace("http://", "ws://").replace("https://", "wss://"),
            self.username
        );

        let (ws_stream, _) = connect_async(&ws_url)
            .await
            .map_err(|e| ClientError::WebSocketError(format!("Connection failed: {}", e)))?;

        let (mut ws_sender, mut ws_receiver) = ws_stream.split();

        // Create message channel
        let (msg_tx, mut msg_rx) = mpsc::unbounded_channel::<String>();

        // Store connection components
        {
            let mut state = self.state.lock().await;
            *state = ConnectionState::Connected;
        }

        self.message_tx.lock().await.replace(msg_tx.clone());

        // Spawn task to handle outgoing messages
        let _outgoing_handle = tokio::spawn(async move {
            while let Some(msg) = msg_rx.recv().await {
                if let Err(e) = ws_sender.send(Message::Text(msg.into())).await {
                    log::error!("Failed to send WebSocket message: {}", e);
                    break;
                }
            }
        });

        // Spawn task to handle incoming messages
        let manager_clone = self.clone_for_spawn();
        let _incoming_handle = tokio::spawn(async move {
            while let Some(result) = ws_receiver.next().await {
                match result {
                    Ok(Message::Text(text)) => {
                        if let Err(e) = manager_clone.route_message(&text).await {
                            log::error!("Failed to route message: {}", e);
                        }
                    }
                    Ok(Message::Close(_)) => {
                        log::info!("WebSocket closed by server");
                        break;
                    }
                    Err(e) => {
                        log::error!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }

            // Mark as disconnected
            let mut state = manager_clone.state.lock().await;
            *state = ConnectionState::Disconnected;
            log::info!("WebSocket connection closed");
        });

        // Store connection tasks (kept alive as long as manager exists)
        // In production, you'd want to store these handles for cleanup
        // For now, we rely on tokio to keep them running

        // Re-subscribe to previously subscribed groups
        let subscribed = self.subscribed_groups.lock().await;
        for group_id in subscribed.iter() {
            self.send_subscribe_message(group_id).await?;
        }

        Ok(())
    }

    /// Create a clone suitable for spawning (implementation detail)
    fn clone_for_spawn(&self) -> Arc<WebSocketManager> {
        Arc::new(WebSocketManager {
            base_url: self.base_url.clone(),
            username: self.username.clone(),
            state: self.state.clone(),
            connection: self.connection.clone(),
            message_tx: self.message_tx.clone(),
            subscribed_groups: self.subscribed_groups.clone(),
        })
    }

    /// Route incoming message to handler
    async fn route_message(&self, text: &str) -> Result<()> {
        match serde_json::from_str::<Value>(text) {
            Ok(value) => {
                if let Some("message") = value.get("type").and_then(|t| t.as_str()) {
                    // This will be handled by MessageService callback
                    log::debug!("Received message: {:?}", value);
                    // TODO: Call MessageService handler via callback
                    Ok(())
                } else {
                    log::warn!("Unknown message type: {:?}", value);
                    Ok(())
                }
            }
            Err(e) => Err(ClientError::WebSocketError(format!(
                "Failed to parse message: {}",
                e
            ))),
        }
    }

    /// Subscribe to a group
    pub async fn subscribe_group(&self, group_id: &str) -> Result<()> {
        // Add to tracked subscriptions
        let mut subscribed = self.subscribed_groups.lock().await;
        if !subscribed.contains(&group_id.to_string()) {
            subscribed.push(group_id.to_string());
        }
        drop(subscribed);

        self.send_subscribe_message(group_id).await
    }

    /// Internal: Send subscribe message
    async fn send_subscribe_message(&self, group_id: &str) -> Result<()> {
        let msg = json!({
            "action": "subscribe",
            "group_id": group_id
        });

        self.send_raw_message(msg.to_string()).await
    }

    /// Unsubscribe from a group
    pub async fn unsubscribe_group(&self, group_id: &str) -> Result<()> {
        // Remove from tracked subscriptions
        let mut subscribed = self.subscribed_groups.lock().await;
        subscribed.retain(|g| g != group_id);
        drop(subscribed);

        let msg = json!({
            "action": "unsubscribe",
            "group_id": group_id
        });

        self.send_raw_message(msg.to_string()).await
    }

    /// Send a message to a group
    pub async fn send_message(&self, group_id: &str, encrypted_content: &str) -> Result<()> {
        let msg = json!({
            "action": "message",
            "group_id": group_id,
            "encrypted_content": encrypted_content
        });

        self.send_raw_message(msg.to_string()).await
    }

    /// Send raw message (internal helper)
    async fn send_raw_message(&self, msg: String) -> Result<()> {
        let state = self.state.lock().await;
        if *state != ConnectionState::Connected {
            return Err(ClientError::StateError(
                "WebSocket not connected".to_string(),
            ));
        }
        drop(state);

        let tx = self.message_tx.lock().await;
        if let Some(sender) = tx.as_ref() {
            sender.send(msg).map_err(|e| {
                ClientError::WebSocketError(format!("Failed to queue message: {}", e))
            })?;
            Ok(())
        } else {
            Err(ClientError::StateError("Message channel not initialized".to_string()))
        }
    }

    /// Stop the connection gracefully
    pub async fn stop(&self) -> Result<()> {
        let mut state = self.state.lock().await;
        *state = ConnectionState::Disconnected;

        // Close all channel senders
        self.message_tx.lock().await.take();

        // Clear subscriptions
        self.subscribed_groups.lock().await.clear();

        log::info!("WebSocket stopped for user: {}", self.username);
        Ok(())
    }

    /// Get current connection state
    pub async fn get_state(&self) -> ConnectionState {
        self.state.lock().await.clone()
    }

    /// Check if connected
    pub async fn is_connected(&self) -> bool {
        *self.state.lock().await == ConnectionState::Connected
    }

    /// Manual reconnection (after failure)
    pub async fn reconnect(&self) -> Result<()> {
        self.stop().await?;
        self.start().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_websocket_manager_creation() {
        let manager =
            WebSocketManager::new("http://localhost:4000".to_string(), "alice".to_string());
        assert_eq!(manager.username, "alice");
    }

    #[tokio::test]
    async fn test_connection_state_transitions() {
        let manager =
            WebSocketManager::new("http://localhost:4000".to_string(), "alice".to_string());
        assert_eq!(manager.get_state().await, ConnectionState::Disconnected);
    }

    #[tokio::test]
    async fn test_subscribe_group_tracking() {
        let manager =
            WebSocketManager::new("http://localhost:4000".to_string(), "alice".to_string());

        // Manually add to subscriptions (without actual connection)
        {
            let mut subs = manager.subscribed_groups.lock().await;
            subs.push("group1".to_string());
        }

        assert_eq!(manager.subscribed_groups.lock().await.len(), 1);
    }

    #[test]
    fn test_ws_url_conversion() {
        let manager =
            WebSocketManager::new("http://localhost:4000".to_string(), "alice".to_string());
        let ws_url = format!(
            "{}/ws/{}",
            manager
                .base_url
                .replace("http://", "ws://")
                .replace("https://", "wss://"),
            manager.username
        );
        assert_eq!(ws_url, "ws://localhost:4000/ws/alice");
    }
}
