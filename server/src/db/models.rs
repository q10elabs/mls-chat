/// Data models for database operations.
/// Represents users, groups, messages, and backups.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub key_package: Vec<u8>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: i64,
    pub group_id: String,
    pub name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: i64,
    pub group_id: i64,
    pub sender_id: i64,
    pub encrypted_content: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Backup {
    pub id: i64,
    pub username: String,
    pub encrypted_state: String,
    pub timestamp: String,
}

// Request/Response DTOs
#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterUserRequest {
    pub username: String,
    pub key_package: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterUserResponse {
    pub id: i64,
    pub username: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserKeyResponse {
    pub username: String,
    pub key_package: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StoreBackupRequest {
    pub encrypted_state: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupResponse {
    pub username: String,
    pub encrypted_state: String,
    pub timestamp: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_struct_creation() {
        let key_package = vec![0x01, 0x02, 0x03, 0x04];
        let user = User {
            id: 1,
            username: "alice".to_string(),
            key_package: key_package.clone(),
            created_at: "2025-10-20T10:00:00Z".to_string(),
        };

        assert_eq!(user.id, 1);
        assert_eq!(user.username, "alice");
        assert_eq!(user.key_package, key_package);
    }

    #[test]
    fn test_register_user_request_serialization() {
        let key_package = vec![0x05, 0x06, 0x07, 0x08];
        let request = RegisterUserRequest {
            username: "bob".to_string(),
            key_package: key_package.clone(),
        };

        let json = serde_json::to_string(&request).expect("Serialization failed");
        let deserialized: RegisterUserRequest =
            serde_json::from_str(&json).expect("Deserialization failed");

        assert_eq!(deserialized.username, "bob");
        assert_eq!(deserialized.key_package, key_package);
    }
}
