/// Message service for handling message operations.
/// Coordinates encryption, sending, receiving, storage, and control message routing.
///
/// Responsibilities:
/// - Encrypting outgoing messages with MLS
/// - Decrypting incoming messages with MLS
/// - Detecting and routing control messages (admin operations)
/// - Storing messages locally
/// - Providing message history and search

use crate::error::{ClientError, Result};
use crate::models::{ControlMessage, ControlMessageType, GroupId, Message};
use crate::services::{GroupService, MlsService, ServerClient, StorageService};
use std::sync::Arc;

pub struct MessageService {
    storage: Arc<StorageService>,
    server_client: Arc<ServerClient>,
    mls_service: Arc<MlsService>,
    group_service: Arc<GroupService>,
}

impl MessageService {
    pub fn new(
        storage: Arc<StorageService>,
        server_client: Arc<ServerClient>,
        mls_service: Arc<MlsService>,
        group_service: Arc<GroupService>,
    ) -> Self {
        MessageService {
            storage,
            server_client,
            mls_service,
            group_service,
        }
    }

    /// Send a message to a group
    pub async fn send_message(&self, group_id: GroupId, sender: String, content: String) -> Result<()> {
        // Validate inputs
        if content.is_empty() {
            return Err(ClientError::MessageError("Cannot send empty message".to_string()));
        }

        if sender.is_empty() {
            return Err(ClientError::MessageError("Sender cannot be empty".to_string()));
        }

        // Get group to access MLS state
        let group = self
            .storage
            .get_group(group_id)?
            .ok_or_else(|| ClientError::InvalidGroup(format!("Group not found: {:?}", group_id)))?;

        // Encrypt message with OpenMLS
        let mut mls_state = group.mls_state.clone();
        let encrypted_content = self
            .mls_service
            .encrypt_message(&mut mls_state, content.clone())?;

        // Convert encrypted bytes to base64 for transmission
        let encoded_content = base64_encode(&encrypted_content);

        // Send to server
        self.server_client
            .send_message(group_id.to_string(), sender.clone(), encoded_content)
            .await?;

        // Store locally
        let message = Message::new(group_id, sender, content);
        self.storage.save_message(&message)?;

        Ok(())
    }

    /// Get message history for a group
    pub async fn get_group_messages(&self, group_id: GroupId, limit: usize) -> Result<Vec<Message>> {
        self.storage.get_group_messages(group_id, limit)
    }

    /// Check if a message is a control message
    fn is_control_message(content: &str) -> bool {
        content.starts_with('{') && (
            content.contains("\"msg_type\"") ||
            content.contains("\"type\"") ||
            content.contains("ADD_PROPOSAL") ||
            content.contains("REMOVE_PROPOSAL")
        )
    }

    /// Process an incoming message from the server
    /// Routes to control message handler if it's an admin operation
    pub async fn process_incoming_message(
        &self,
        group_id: GroupId,
        sender: String,
        encrypted_content: String,
    ) -> Result<Message> {
        // Get group to access MLS state
        let group = self
            .storage
            .get_group(group_id)?
            .ok_or_else(|| ClientError::InvalidGroup(format!("Group not found: {:?}", group_id)))?;

        // Decode base64 content
        let encrypted_bytes = base64_decode(&encrypted_content)
            .map_err(|e| ClientError::MessageError(format!("Failed to decode content: {}", e)))?;

        // Decrypt with OpenMLS
        let mut mls_state = group.mls_state.clone();
        let decrypted_content = self
            .mls_service
            .decrypt_message(&mut mls_state, encrypted_bytes)?;

        // Check if this is a control message and route accordingly
        if Self::is_likely_control_message(&decrypted_content) {
            self.handle_control_message(group_id, &decrypted_content)
                .await?;
        }

        // Create message from decrypted content
        let message = Message::from_server(
            group_id,
            sender,
            decrypted_content,
            chrono::Utc::now(),
        );

        // Store in database
        self.storage.save_message(&message)?;

        Ok(message)
    }

    /// Poll for new messages from the server (HTTP fallback)
    pub async fn poll_messages(&self, group_id: GroupId) -> Result<Vec<Message>> {
        let group_id_str = group_id.to_string();

        // Poll server for raw messages
        let raw_messages = self.server_client.poll_group_messages(&group_id_str).await?;

        let mut processed_messages = Vec::new();
        for raw_msg in raw_messages {
            // Process each message
            if let Ok(message) = self
                .process_incoming_message(
                    group_id,
                    raw_msg.sender.clone(),
                    raw_msg.encrypted_content,
                )
                .await
            {
                processed_messages.push(message);
            }
        }

        Ok(processed_messages)
    }

    /// Delete a message
    pub async fn delete_message(&self, _message: &Message) -> Result<()> {
        // TODO: Implement message deletion
        // For now, we don't support deletion
        Err(ClientError::OperationFailed(
            "Message deletion not yet implemented".to_string(),
        ))
    }

    /// Search messages in a group
    pub async fn search_messages(
        &self,
        group_id: GroupId,
        query: String,
        limit: usize,
    ) -> Result<Vec<Message>> {
        let messages = self.get_group_messages(group_id, limit).await?;

        let query_lower = query.to_lowercase();
        let results: Vec<Message> = messages
            .into_iter()
            .filter(|msg| {
                msg.content.to_lowercase().contains(&query_lower)
                    || msg.sender.to_lowercase().contains(&query_lower)
            })
            .collect();

        Ok(results)
    }

    /// Process a control message received from the server
    /// Extracts metadata and returns (message_type, target_user, reason)
    pub fn extract_control_message_metadata(content: &str) -> Result<(String, String, Option<String>)> {
        // Try to parse as JSON ControlMessage
        if let Ok(control_msg) = serde_json::from_str::<serde_json::Value>(content) {
            if let (Some(msg_type), Some(target_user)) = (
                control_msg.get("msg_type").and_then(|v| v.as_str()),
                control_msg.get("target_user").and_then(|v| v.as_str()),
            ) {
                let reason = control_msg.get("reason").and_then(|v| v.as_str()).map(|s| s.to_string());
                return Ok((msg_type.to_string(), target_user.to_string(), reason));
            }
        }

        // Try to parse as ADD_PROPOSAL
        if content.starts_with("ADD_PROPOSAL:") {
            return Ok(("ADD_PROPOSAL".to_string(), "system".to_string(), None));
        }

        // Try to parse as REMOVE_PROPOSAL
        if content.starts_with("REMOVE_PROPOSAL:") {
            return Ok(("REMOVE_PROPOSAL".to_string(), "system".to_string(), None));
        }

        Err(ClientError::MessageError(
            "Content is not a control message".to_string(),
        ))
    }

    /// Check if content is a control message without throwing error
    pub fn is_likely_control_message(content: &str) -> bool {
        Self::is_control_message(content) ||
        content.starts_with("ADD_PROPOSAL:") ||
        content.starts_with("REMOVE_PROPOSAL:")
    }

    /// Handle a control message (kick, mod add/remove, proposals)
    /// Updates group state via GroupService
    async fn handle_control_message(&self, group_id: GroupId, content: &str) -> Result<()> {
        // Try to extract control message metadata
        match Self::extract_control_message_metadata(content) {
            Ok((msg_type, target_user, _reason)) => {
                // Parse the message type string to ControlMessageType
                let control_type = match msg_type.as_str() {
                    "Kick" => ControlMessageType::Kick,
                    "ModAdd" => ControlMessageType::ModAdd,
                    "ModRemove" => ControlMessageType::ModRemove,
                    _ => return Ok(()), // Unknown control message type, skip
                };

                // Create ControlMessage struct
                let control_msg = ControlMessage {
                    msg_type: control_type,
                    target_user,
                    reason: None,
                };

                // Serialize and pass to GroupService
                let control_json = serde_json::to_string(&control_msg)
                    .map_err(|e| ClientError::MessageError(format!("Failed to serialize control message: {}", e)))?;

                // Process control message with group_id
                self.group_service
                    .process_control_message(group_id, &control_json)
                    .await?;
            }
            Err(_) => {
                // Not a valid control message, skip
            }
        }

        Ok(())
    }

    /// Handle an incoming WebSocket message
    /// Expects message in server broadcast format: {"type": "message", "sender": "...", "group_id": "...", "encrypted_content": "..."}
    pub async fn handle_websocket_message(
        &self,
        message_json: &str,
    ) -> Result<()> {
        // Parse the WebSocket message
        let value = serde_json::from_str::<serde_json::Value>(message_json)
            .map_err(|e| ClientError::MessageError(format!("Failed to parse WebSocket message: {}", e)))?;

        // Extract fields
        let sender = value
            .get("sender")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ClientError::MessageError("Missing sender in WebSocket message".to_string()))?;

        let group_id_str = value
            .get("group_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ClientError::MessageError("Missing group_id in WebSocket message".to_string()))?;

        let encrypted_content = value
            .get("encrypted_content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ClientError::MessageError("Missing encrypted_content in WebSocket message".to_string()))?;

        // Convert group_id string to GroupId
        let group_id = GroupId::from_string(group_id_str);

        // Process as incoming message (handles decryption, storage, control routing)
        self.process_incoming_message(group_id, sender.to_string(), encrypted_content.to_string())
            .await?;

        Ok(())
    }
}

/// Simple base64 encoding/decoding
fn base64_encode(data: &[u8]) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut result = String::new();

    for chunk in data.chunks(3) {
        let b1 = chunk[0];
        let b2 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b3 = if chunk.len() > 2 { chunk[2] } else { 0 };

        result.push(CHARSET[(b1 >> 2) as usize] as char);
        result.push(CHARSET[(((b1 & 0x03) << 4) | (b2 >> 4)) as usize] as char);

        if chunk.len() > 1 {
            result.push(CHARSET[(((b2 & 0x0f) << 2) | (b3 >> 6)) as usize] as char);
        } else {
            result.push('=');
        }

        if chunk.len() > 2 {
            result.push(CHARSET[(b3 & 0x3f) as usize] as char);
        } else {
            result.push('=');
        }
    }

    result
}

fn base64_decode(data: &str) -> std::result::Result<Vec<u8>, &'static str> {
    let mut result = Vec::new();
    let bytes = data.as_bytes();

    for chunk in bytes.chunks(4) {
        if chunk.len() < 2 {
            return Err("Invalid base64 input");
        }

        let b1 = decode_char(chunk[0])?;
        let b2 = decode_char(chunk[1])?;
        let b3 = if chunk.len() > 2 && chunk[2] != b'=' {
            decode_char(chunk[2])?
        } else {
            0
        };
        let b4 = if chunk.len() > 3 && chunk[3] != b'=' {
            decode_char(chunk[3])?
        } else {
            0
        };

        result.push(((b1 << 2) | (b2 >> 4)) as u8);

        if chunk.len() > 2 && chunk[2] != b'=' {
            result.push((((b2 & 0x0f) << 4) | (b3 >> 2)) as u8);

            if chunk.len() > 3 && chunk[3] != b'=' {
                result.push((((b3 & 0x03) << 6) | b4) as u8);
            }
        }
    }

    Ok(result)
}

fn decode_char(c: u8) -> std::result::Result<u8, &'static str> {
    match c {
        b'A'..=b'Z' => Ok(c - b'A'),
        b'a'..=b'z' => Ok(c - b'a' + 26),
        b'0'..=b'9' => Ok(c - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        b'=' => Ok(0),
        _ => Err("Invalid base64 character"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_encode_decode() {
        let original = b"hello world";
        let encoded = base64_encode(original);
        let decoded = base64_decode(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_base64_empty() {
        let encoded = base64_encode(b"");
        let decoded: Vec<u8> = base64_decode(&encoded).unwrap();
        assert_eq!(decoded, Vec::<u8>::new());
    }

    #[tokio::test]
    async fn test_message_service_creation() {
        let storage = Arc::new(StorageService::in_memory().unwrap());
        let server = Arc::new(ServerClient::new("http://localhost:4000".to_string()));
        let mls = Arc::new(MlsService::new());
        let group_service = Arc::new(GroupService::new(storage.clone(), mls.clone(), server.clone()));
        let _msg_service = MessageService::new(storage, server, mls, group_service);
        // Service created successfully
    }

    #[test]
    fn test_is_control_message_detection() {
        // JSON control message
        let control_json = r#"{"msg_type":"Kick","target_user":"alice","reason":null}"#;
        assert!(MessageService::is_likely_control_message(control_json));

        // ADD_PROPOSAL
        assert!(MessageService::is_likely_control_message("ADD_PROPOSAL:alice:pk_alice"));

        // REMOVE_PROPOSAL
        assert!(MessageService::is_likely_control_message("REMOVE_PROPOSAL:bob"));

        // Regular message
        assert!(!MessageService::is_likely_control_message("hello world"));
    }

    #[test]
    fn test_extract_control_message_kick() {
        let control_json = r#"{"msg_type":"Kick","target_user":"alice","reason":"spam"}"#;
        let result = MessageService::extract_control_message_metadata(control_json);
        assert!(result.is_ok());

        let (msg_type, target_user, reason) = result.unwrap();
        assert_eq!(msg_type, "Kick");
        assert_eq!(target_user, "alice");
        assert_eq!(reason, Some("spam".to_string()));
    }

    #[test]
    fn test_extract_control_message_mod_add() {
        let control_json = r#"{"msg_type":"ModAdd","target_user":"bob","reason":null}"#;
        let result = MessageService::extract_control_message_metadata(control_json);
        assert!(result.is_ok());

        let (msg_type, target_user, reason) = result.unwrap();
        assert_eq!(msg_type, "ModAdd");
        assert_eq!(target_user, "bob");
        assert_eq!(reason, None);
    }

    #[test]
    fn test_extract_add_proposal() {
        let proposal = "ADD_PROPOSAL:alice:pk_alice";
        let result = MessageService::extract_control_message_metadata(proposal);
        assert!(result.is_ok());

        let (msg_type, _, _) = result.unwrap();
        assert_eq!(msg_type, "ADD_PROPOSAL");
    }

    #[test]
    fn test_extract_remove_proposal() {
        let proposal = "REMOVE_PROPOSAL:bob";
        let result = MessageService::extract_control_message_metadata(proposal);
        assert!(result.is_ok());

        let (msg_type, _, _) = result.unwrap();
        assert_eq!(msg_type, "REMOVE_PROPOSAL");
    }

    #[test]
    fn test_extract_control_message_invalid() {
        let regular_message = "hello world";
        let result = MessageService::extract_control_message_metadata(regular_message);
        assert!(result.is_err());
    }

    #[test]
    fn test_is_control_message_with_type_field() {
        let msg_with_type = r#"{"type":"control","msg_type":"Kick","target_user":"alice"}"#;
        assert!(MessageService::is_likely_control_message(msg_with_type));
    }

    #[test]
    fn test_base64_various_inputs() {
        let inputs: Vec<Vec<u8>> = vec![
            vec![],
            vec![b'a'],
            vec![b'a', b'b'],
            vec![b'a', b'b', b'c'],
            vec![b'a', b'b', b'c', b'd'],
            "test message".as_bytes().to_vec(),
            vec![0u8, 1, 2, 255],
        ];

        for input in inputs {
            let encoded = base64_encode(&input);
            let decoded = base64_decode(&encoded).expect("decode failed");
            assert_eq!(decoded, input, "Failed for input: {:?}", input);
        }
    }

    #[test]
    fn test_websocket_message_format_validation() {
        // Valid WebSocket message format
        let valid_msg = r#"{"type":"message","sender":"alice","group_id":"550e8400-e29b-41d4-a716-446655440000","encrypted_content":"abcd1234"}"#;
        let parsed = serde_json::from_str::<serde_json::Value>(valid_msg);
        assert!(parsed.is_ok());

        // Missing sender
        let missing_sender = r#"{"type":"message","group_id":"550e8400-e29b-41d4-a716-446655440000","encrypted_content":"abcd1234"}"#;
        let value = serde_json::from_str::<serde_json::Value>(missing_sender).unwrap();
        assert!(value.get("sender").is_none());

        // Missing group_id
        let missing_group = r#"{"type":"message","sender":"alice","encrypted_content":"abcd1234"}"#;
        let value = serde_json::from_str::<serde_json::Value>(missing_group).unwrap();
        assert!(value.get("group_id").is_none());

        // Missing encrypted_content
        let missing_content = r#"{"type":"message","sender":"alice","group_id":"550e8400-e29b-41d4-a716-446655440000"}"#;
        let value = serde_json::from_str::<serde_json::Value>(missing_content).unwrap();
        assert!(value.get("encrypted_content").is_none());
    }
}
