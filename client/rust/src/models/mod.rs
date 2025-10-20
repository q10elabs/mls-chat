/// Data models for the MLS chat client.
/// Defines User, Group, Message, and related structures.

pub mod user;
pub mod group;
pub mod message;

pub use user::{User, UserId};
pub use group::{Group, GroupId, Member, MemberRole};
pub use message::{Message, MessageId};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Universally unique identifier wrapper
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Id(Uuid);

impl Id {
    pub fn new() -> Self {
        Id(Uuid::new_v4())
    }

    pub fn from_string(s: String) -> Self {
        Id(Uuid::parse_str(&s).unwrap_or_else(|_| Uuid::new_v4()))
    }

    pub fn to_string(&self) -> String {
        self.0.to_string()
    }
}

impl Default for Id {
    fn default() -> Self {
        Self::new()
    }
}

/// Response from server on message receipt (wire format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePayload {
    pub group_id: String,
    pub sender: String,
    pub encrypted_content: String,
}

/// User registration request for server
#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterUserRequest {
    pub username: String,
    pub public_key: String,
}

/// User registration response from server
#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterUserResponse {
    pub id: i64,
    pub username: String,
    pub created_at: String,
}

/// User key lookup response from server
#[derive(Debug, Serialize, Deserialize)]
pub struct UserKeyResponse {
    pub username: String,
    pub public_key: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_id_generation() {
        let id1 = Id::new();
        let id2 = Id::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_id_string_conversion() {
        let id = Id::new();
        let id_str = id.to_string();
        let id_from_str = Id::from_string(id_str.clone());
        assert_eq!(id, id_from_str);
    }

    #[test]
    fn test_message_payload_serialization() {
        let payload = MessagePayload {
            group_id: "group_123".to_string(),
            sender: "alice".to_string(),
            encrypted_content: "encrypted_data".to_string(),
        };

        let json = serde_json::to_string(&payload).unwrap();
        let deserialized: MessagePayload = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.group_id, "group_123");
        assert_eq!(deserialized.sender, "alice");
    }
}
