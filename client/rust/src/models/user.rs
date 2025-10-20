/// User model for the MLS chat client.
/// Represents a local user and their credentials.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(Uuid);

impl UserId {
    pub fn new() -> Self {
        UserId(Uuid::new_v4())
    }

    pub fn from_string(s: &str) -> Self {
        UserId(Uuid::parse_str(s).unwrap_or_else(|_| Uuid::new_v4()))
    }

    pub fn to_string(&self) -> String {
        self.0.to_string()
    }
}

impl Default for UserId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub username: String,
    /// Public key for OpenMLS
    pub public_key: String,
    /// Local key material for OpenMLS operations
    pub local_key_material: Vec<u8>,
    pub created_at: DateTime<Utc>,
}

impl User {
    pub fn new(username: String, public_key: String, local_key_material: Vec<u8>) -> Self {
        User {
            id: UserId::new(),
            username,
            public_key,
            local_key_material,
            created_at: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_id_generation() {
        let id1 = UserId::new();
        let id2 = UserId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_user_creation() {
        let user = User::new(
            "alice".to_string(),
            "pk_abc123".to_string(),
            vec![1, 2, 3, 4, 5],
        );

        assert_eq!(user.username, "alice");
        assert_eq!(user.public_key, "pk_abc123");
        assert_eq!(user.local_key_material, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_user_serialization() {
        let user = User::new(
            "bob".to_string(),
            "pk_xyz789".to_string(),
            vec![6, 7, 8],
        );

        let json = serde_json::to_string(&user).unwrap();
        let deserialized: User = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.username, "bob");
        assert_eq!(deserialized.public_key, "pk_xyz789");
    }
}
