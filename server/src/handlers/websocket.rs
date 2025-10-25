/// WebSocket handler for real-time message distribution.
/// Manages client connections, group subscriptions, and message broadcasting.

use crate::db::{Database, DbPool};
use actix::prelude::*;
use actix_web::{web, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Message types for WebSocket communication
#[derive(Message, Clone, Debug)]
#[rtype(result = "()")]
pub struct WsMessage {
    pub sender: String,
    pub group_id: String,
    pub encrypted_content: String,
}

/// WebSocket server state - manages client connections and routing
pub struct WsServer {
    pub clients: Arc<RwLock<HashMap<String, tokio::sync::mpsc::UnboundedSender<String>>>>,
    pub groups: Arc<RwLock<HashMap<String, HashSet<String>>>>,
    pub pool: Arc<web::Data<DbPool>>,
}

impl WsServer {
    pub fn new(pool: Arc<web::Data<DbPool>>) -> Self {
        WsServer {
            clients: Arc::new(RwLock::new(HashMap::new())),
            groups: Arc::new(RwLock::new(HashMap::new())),
            pool,
        }
    }

    /// Register a client connection
    pub async fn register(&self, client_id: String, tx: tokio::sync::mpsc::UnboundedSender<String>) {
        let mut clients = self.clients.write().await;
        clients.insert(client_id, tx);
    }

    /// Unregister a client connection
    pub async fn unregister(&self, client_id: &str) {
        let mut clients = self.clients.write().await;
        clients.remove(client_id);

        // Remove from all groups
        let mut groups = self.groups.write().await;
        for members in groups.values_mut() {
            members.remove(client_id);
        }
    }

    /// Subscribe a client to a group
    pub async fn subscribe(&self, client_id: String, group_id: String) {
        let mut groups = self.groups.write().await;
        groups
            .entry(group_id)
            .or_insert_with(HashSet::new)
            .insert(client_id);
    }

    /// Unsubscribe a client from a group
    pub async fn unsubscribe(&self, client_id: &str, group_id: &str) {
        let mut groups = self.groups.write().await;
        if let Some(members) = groups.get_mut(group_id) {
            members.remove(client_id);
        }
    }

    /// Broadcast message to all clients in a group
    pub async fn broadcast_to_group(&self, group_id: &str, message: &str) {
        let groups = self.groups.read().await;
        if let Some(members) = groups.get(group_id) {
            let clients = self.clients.read().await;
            for member in members {
                if let Some(tx) = clients.get(member) {
                    let _ = tx.send(message.to_string());
                }
            }
        }
    }

    /// Store message to database
    pub async fn persist_message(
        &self,
        group_id: &str,
        sender: &str,
        encrypted_content: &str,
    ) -> bool {
        // Get or create group
        let group = match Database::get_group(self.pool.as_ref().as_ref(), group_id).await {
            Ok(Some(g)) => g,
            Ok(None) => {
                match Database::create_group(self.pool.as_ref().as_ref(), group_id, group_id).await {
                    Ok(g) => g,
                    Err(e) => {
                        log::error!("Failed to create group: {}", e);
                        return false;
                    }
                }
            }
            Err(e) => {
                log::error!("Failed to get group: {}", e);
                return false;
            }
        };

        // Get sender user
        let user = match Database::get_user(self.pool.as_ref().as_ref(), sender).await {
            Ok(Some(u)) => u,
            Ok(None) => {
                log::warn!("Sender not found: {}", sender);
                return false;
            }
            Err(e) => {
                log::error!("Failed to get user: {}", e);
                return false;
            }
        };

        // Store message
        match Database::store_message(self.pool.as_ref().as_ref(), group.id, user.id, encrypted_content)
            .await
        {
            Ok(_) => true,
            Err(e) => {
                log::error!("Failed to store message: {}", e);
                false
            }
        }
    }
}

/// WebSocket actor for individual client connections
pub struct WsActor {
    pub client_id: String,
    pub username: String,
    pub server: web::Data<WsServer>,
}

impl Actor for WsActor {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        log::info!("WebSocket connection started: {}", self.client_id);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let addr = ctx.address();
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                addr.do_send(IncomingMessage(msg));
            }
        });

        let server = self.server.clone();
        let client_id = self.client_id.clone();
        let fut = async move {
            server.register(client_id, tx).await;
        };
        let _ = actix::spawn(fut);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        log::info!("WebSocket connection stopped: {}", self.client_id);
        let server = self.server.clone();
        let client_id = self.client_id.clone();
        let fut = async move {
            server.unregister(&client_id).await;
        };
        let _ = actix::spawn(fut);
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WsActor {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Text(text)) => {
                match serde_json::from_str::<serde_json::Value>(&text) {
                    Ok(value) => {
                        // Check if this is an action-based control message (subscription/unsubscription)
                        if let Some(action) = value.get("action").and_then(|a| a.as_str()) {
                            match action {
                                "subscribe" => {
                                    if let Some(group_id) = value.get("group_id").and_then(|g| g.as_str()) {
                                        let server = self.server.clone();
                                        let client_id = self.client_id.clone();
                                        let group_id = group_id.to_string();
                                        actix::spawn(async move {
                                            server.subscribe(client_id, group_id).await;
                                        });
                                    }
                                }
                                "unsubscribe" => {
                                    if let Some(group_id) = value.get("group_id").and_then(|g| g.as_str())
                                    {
                                        let server = self.server.clone();
                                        let client_id = self.client_id.clone();
                                        let group_id = group_id.to_string();
                                        actix::spawn(async move {
                                            server.unsubscribe(&client_id, &group_id).await;
                                        });
                                    }
                                }
                                _ => {
                                    log::warn!("Unknown action: {}", action);
                                }
                            }
                        } else if let Some(msg_type) = value.get("type").and_then(|t| t.as_str()) {
                            // Handle MLS envelope-based messages (discriminated by type field)
                            match msg_type {
                                "application" => {
                                    if let Some(group_id) = value.get("group_id").and_then(|g| g.as_str())
                                    {
                                        if let Some(encrypted_content) = value.get("encrypted_content").and_then(|c| c.as_str()) {
                                            let server = self.server.clone();
                                            let username = self.username.clone();
                                            let group_id = group_id.to_string();
                                            let encrypted_content = encrypted_content.to_string();
                                            actix::spawn(async move {
                                                let persisted = server
                                                    .persist_message(&group_id, &username, &encrypted_content)
                                                    .await;

                                                if persisted {
                                                    let msg = json!({
                                                        "type": "application",
                                                        "sender": username,
                                                        "group_id": group_id.clone(),
                                                        "encrypted_content": encrypted_content
                                                    })
                                                    .to_string();
                                                    server.broadcast_to_group(&group_id, &msg).await;
                                                }
                                            });
                                        }
                                    }
                                }
                                "welcome" => {
                                    if let Some(welcome_blob) = value.get("welcome_blob").and_then(|w| w.as_str()) {
                                        if let Some(inviter) = value.get("inviter").and_then(|i| i.as_str()) {
                                            if let Some(invitee) = value.get("invitee").and_then(|i| i.as_str()) {
                                                if let Some(ratchet_tree) = value.get("ratchet_tree_blob").and_then(|r| r.as_str()) {
                                                    let server = self.server.clone();
                                                    let inviter = inviter.to_string();
                                                    let invitee = invitee.to_string();
                                                    let welcome_blob = welcome_blob.to_string();
                                                    let ratchet_tree = ratchet_tree.to_string();
                                                    actix::spawn(async move {
                                                        let msg = json!({
                                                            "type": "welcome",
                                                            "inviter": inviter,
                                                            "invitee": invitee.clone(),
                                                            "welcome_blob": welcome_blob,
                                                            "ratchet_tree_blob": ratchet_tree
                                                        })
                                                        .to_string();
                                                        // Send Welcome message directly to the invitee
                                                        server.broadcast_to_group(&invitee, &msg).await;
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                                "commit" => {
                                    if let Some(group_id) = value.get("group_id").and_then(|g| g.as_str())
                                    {
                                        if let Some(commit_blob) = value.get("commit_blob").and_then(|c| c.as_str()) {
                                            if let Some(sender) = value.get("sender").and_then(|s| s.as_str()) {
                                                let server = self.server.clone();
                                                let group_id = group_id.to_string();
                                                let commit_blob = commit_blob.to_string();
                                                let sender = sender.to_string();
                                                actix::spawn(async move {
                                                    let msg = json!({
                                                        "type": "commit",
                                                        "group_id": group_id.clone(),
                                                        "sender": sender,
                                                        "commit_blob": commit_blob
                                                    })
                                                    .to_string();
                                                    server.broadcast_to_group(&group_id, &msg).await;
                                                });
                                            }
                                        }
                                    }
                                }
                                _ => {
                                    log::warn!("Unknown message type: {}", msg_type);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to parse WebSocket message: {}", e);
                        ctx.text(
                            json!({
                                "error": "Invalid message format"
                            })
                            .to_string(),
                        );
                    }
                }
            }
            Ok(ws::Message::Close(_)) => {
                ctx.stop();
            }
            Err(e) => {
                log::error!("WebSocket error: {}", e);
                ctx.stop();
            }
            _ => {}
        }
    }
}

#[derive(Message)]
#[rtype(result = "()")]
struct IncomingMessage(String);

impl Handler<IncomingMessage> for WsActor {
    type Result = ();

    fn handle(&mut self, msg: IncomingMessage, ctx: &mut Self::Context) {
        ctx.text(msg.0);
    }
}

/// WebSocket connection handler
pub async fn ws_connect(
    req: HttpRequest,
    stream: web::Payload,
    username: web::Path<String>,
    server: web::Data<WsServer>,
) -> actix_web::Result<HttpResponse> {
    let client_id = format!("{}_{}", username, uuid::Uuid::new_v4());

    let actor = WsActor {
        client_id: client_id.clone(),
        username: username.into_inner(),
        server: server.clone(),
    };

    let resp = ws::start(actor, &req, stream)?;
    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ws_server_register() {
        let pool = Arc::new(web::Data::new(crate::db::create_test_pool()));
        let server = WsServer::new(pool);

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        server.register("client1".to_string(), tx).await;

        let clients = server.clients.read().await;
        assert!(clients.contains_key("client1"));
    }

    #[tokio::test]
    async fn test_ws_server_unregister() {
        let pool = Arc::new(web::Data::new(crate::db::create_test_pool()));
        let server = WsServer::new(pool);

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        server.register("client1".to_string(), tx).await;
        server.unregister("client1").await;

        let clients = server.clients.read().await;
        assert!(!clients.contains_key("client1"));
    }

    #[tokio::test]
    async fn test_ws_server_subscribe() {
        let pool = Arc::new(web::Data::new(crate::db::create_test_pool()));
        let server = WsServer::new(pool);

        server
            .subscribe("client1".to_string(), "group1".to_string())
            .await;

        let groups = server.groups.read().await;
        assert!(groups.get("group1").unwrap().contains("client1"));
    }

    #[tokio::test]
    async fn test_ws_server_broadcast() {
        let pool = Arc::new(web::Data::new(crate::db::create_test_pool()));
        let server = Arc::new(WsServer::new(pool));

        let (tx1, mut rx1) = tokio::sync::mpsc::unbounded_channel();
        let (tx2, mut rx2) = tokio::sync::mpsc::unbounded_channel();

        server.register("client1".to_string(), tx1).await;
        server.register("client2".to_string(), tx2).await;

        server
            .subscribe("client1".to_string(), "group1".to_string())
            .await;
        server
            .subscribe("client2".to_string(), "group1".to_string())
            .await;

        server.broadcast_to_group("group1", "test message").await;

        assert_eq!(rx1.recv().await, Some("test message".to_string()));
        assert_eq!(rx2.recv().await, Some("test message".to_string()));
    }
}
