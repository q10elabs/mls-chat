/// Message model for the MLS chat client.
/// Represents a message in a group conversation.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::GroupId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(Uuid);

impl MessageId {
    pub fn new() -> Self {
        MessageId(Uuid::new_v4())
    }

    pub fn from_string(s: &str) -> Self {
        MessageId(Uuid::parse_str(s).unwrap_or_else(|_| Uuid::new_v4()))
    }

    pub fn to_string(&self) -> String {
        self.0.to_string()
    }
}

impl Default for MessageId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub group_id: GroupId,
    pub sender: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    /// True if message is only in local DB, not yet confirmed by server
    pub local_only: bool,
}

impl Message {
    /// Create a new message from decrypted content
    pub fn new(group_id: GroupId, sender: String, content: String) -> Self {
        Message {
            id: MessageId::new(),
            group_id,
            sender,
            content,
            timestamp: Utc::now(),
            local_only: true,
        }
    }

    /// Create a message from server (already confirmed)
    pub fn from_server(group_id: GroupId, sender: String, content: String, timestamp: DateTime<Utc>) -> Self {
        Message {
            id: MessageId::new(),
            group_id,
            sender,
            content,
            timestamp,
            local_only: false,
        }
    }

    /// Mark message as confirmed by server
    pub fn confirm(&mut self) {
        self.local_only = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_id_generation() {
        let id1 = MessageId::new();
        let id2 = MessageId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_message_creation() {
        let group_id = GroupId::new();
        let msg = Message::new(group_id, "alice".to_string(), "hello".to_string());

        assert_eq!(msg.sender, "alice");
        assert_eq!(msg.content, "hello");
        assert_eq!(msg.group_id, group_id);
        assert!(msg.local_only);
    }

    #[test]
    fn test_message_from_server() {
        let group_id = GroupId::new();
        let now = Utc::now();
        let msg = Message::from_server(group_id, "bob".to_string(), "hi".to_string(), now);

        assert_eq!(msg.sender, "bob");
        assert!(!msg.local_only);
    }

    #[test]
    fn test_message_confirm() {
        let group_id = GroupId::new();
        let mut msg = Message::new(group_id, "alice".to_string(), "hello".to_string());

        assert!(msg.local_only);
        msg.confirm();
        assert!(!msg.local_only);
    }

    #[test]
    fn test_message_serialization() {
        let group_id = GroupId::new();
        let msg = Message::new(group_id, "alice".to_string(), "hello world".to_string());

        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.sender, "alice");
        assert_eq!(deserialized.content, "hello world");
    }
}
