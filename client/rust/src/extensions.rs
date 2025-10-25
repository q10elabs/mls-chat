/// Custom extension for group-level metadata
/// Stored in GroupContext extensions (encrypted in group state)
/// Type ID: 0xff00 (private use range per RFC 9420, well into private use area)

use serde::{Deserialize, Serialize};

pub const GROUP_METADATA_EXTENSION_TYPE: u16 = 0xff00;

/// Extensible group metadata stored in group context extensions
/// Serialized as JSON and stored in UnknownExtension
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMetadata {
    /// Human-readable group name (encrypted in group state)
    pub name: String,

    /// Unix timestamp when group was created
    pub created_at: u64,

    /// Version for detecting changes/rollbacks
    pub version: u32,

    // Future fields can be added here without breaking old clients
    // (clients will just ignore unknown fields during deserialization)
}

impl GroupMetadata {
    /// Create new group metadata
    pub fn new(name: String) -> Self {
        Self {
            name,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            version: 1,
        }
    }

    /// Serialize to bytes for storage in UnknownExtension
    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Deserialize from extension bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_group_metadata_serialization() {
        let metadata = GroupMetadata::new("Test Group".to_string());
        let bytes = metadata.to_bytes().unwrap();
        let deserialized = GroupMetadata::from_bytes(&bytes).unwrap();
        assert_eq!(metadata.name, deserialized.name);
        assert_eq!(metadata.version, deserialized.version);
    }

    #[test]
    fn test_group_metadata_round_trip() {
        let metadata = GroupMetadata {
            name: "Complex Name".to_string(),
            created_at: 1234567890,
            version: 5,
        };

        let bytes = metadata.to_bytes().unwrap();
        let deserialized = GroupMetadata::from_bytes(&bytes).unwrap();

        assert_eq!(metadata.name, deserialized.name);
        assert_eq!(metadata.created_at, deserialized.created_at);
        assert_eq!(metadata.version, deserialized.version);
    }
}
