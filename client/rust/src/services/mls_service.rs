/// OpenMLS integration service.
/// Implements MLS group creation, encryption, decryption, and state management.
///
/// Note: This is a simplified implementation using basic encryption for MVP.
/// Full OpenMLS integration with real group state management is a future enhancement.

use crate::error::{ClientError, Result};
use crate::models::GroupId;
use rand::RngCore;

pub struct MlsService;

impl MlsService {
    pub fn new() -> Self {
        MlsService
    }

    /// Create a new MLS group
    /// Returns (group_id, serialized_mls_state)
    ///
    /// For MVP, generates a random state blob.
    /// Full implementation would use real OpenMLS group creation.
    pub fn create_group(&self, _group_name: &str) -> Result<(GroupId, Vec<u8>)> {
        let group_id = GroupId::new();

        // Generate a random MLS state (placeholder for real OpenMLS group)
        let mut mls_state = vec![0u8; 32];
        let mut rng = rand::thread_rng();
        rng.fill_bytes(&mut mls_state);

        Ok((group_id, mls_state))
    }

    /// Add a member to an MLS group (generates an Add proposal)
    /// Returns the serialized proposal
    ///
    /// For MVP, returns a formatted proposal string.
    /// Full implementation would generate actual MLS Add proposals.
    pub fn add_member(
        &self,
        _mls_state_bytes: &[u8],
        username: &str,
        public_key: &str,
    ) -> Result<Vec<u8>> {
        // Validate inputs
        if username.is_empty() || public_key.is_empty() {
            return Err(ClientError::MlsError(
                "Username and public key cannot be empty".to_string(),
            ));
        }

        // For MVP: return a formatted proposal that can be parsed later
        // Format: "ADD_PROPOSAL:username:public_key"
        let proposal = format!("ADD_PROPOSAL:{}:{}", username, public_key);
        Ok(proposal.as_bytes().to_vec())
    }

    /// Remove a member from an MLS group (generates a Remove proposal)
    /// Returns the serialized proposal
    ///
    /// For MVP, returns a formatted proposal string.
    /// Full implementation would generate actual MLS Remove proposals.
    pub fn remove_member(&self, _mls_state_bytes: &[u8], username: &str) -> Result<Vec<u8>> {
        if username.is_empty() {
            return Err(ClientError::MlsError("Username cannot be empty".to_string()));
        }

        // For MVP: return a formatted proposal
        // Format: "REMOVE_PROPOSAL:username"
        let proposal = format!("REMOVE_PROPOSAL:{}", username);
        Ok(proposal.as_bytes().to_vec())
    }

    /// Encrypt a message for a group
    /// Uses a deterministic encryption key based on group state
    ///
    /// For MVP, uses XOR with a fixed key.
    /// Full implementation would use MLS group encryption context.
    pub fn encrypt_message(&self, _mls_state: &mut Vec<u8>, content: String) -> Result<Vec<u8>> {
        if content.is_empty() {
            return Err(ClientError::MlsError("Cannot encrypt empty message".to_string()));
        }

        // For MVP: use XOR cipher (NOT SECURE - for demo only)
        // In production, this would use the MLS group's encryption context
        let key = b"mls-chat-key-123";
        let mut result = content.as_bytes().to_vec();
        for (i, byte) in result.iter_mut().enumerate() {
            *byte ^= key[i % key.len()];
        }

        Ok(result)
    }

    /// Decrypt a message from a group
    /// Uses the same deterministic key as encryption
    pub fn decrypt_message(&self, _mls_state: &mut Vec<u8>, encrypted: Vec<u8>) -> Result<String> {
        // XOR cipher is symmetric, so decryption uses the same operation
        let key = b"mls-chat-key-123";
        let mut result = encrypted;
        for (i, byte) in result.iter_mut().enumerate() {
            *byte ^= key[i % key.len()];
        }

        String::from_utf8(result)
            .map_err(|e| ClientError::MlsError(format!("Failed to decrypt message: {}", e)))
    }

    /// Handle a group state update from the server
    pub fn handle_group_update(&self, update: Vec<u8>) -> Result<Vec<u8>> {
        if update.is_empty() {
            return Err(ClientError::MlsError("Invalid group update".to_string()));
        }

        // For MVP: just return the update as new state
        // Full implementation would validate and apply updates to MLS group
        Ok(update)
    }

    /// Process an incoming Add proposal
    /// Extracts (username, public_key) from the proposal
    pub fn process_add_proposal(&self, proposal_bytes: &[u8]) -> Result<(String, String)> {
        let proposal_str = String::from_utf8(proposal_bytes.to_vec()).map_err(|e| {
            ClientError::MlsError(format!("Failed to parse proposal: {}", e))
        })?;

        if !proposal_str.starts_with("ADD_PROPOSAL:") {
            return Err(ClientError::MlsError("Invalid Add proposal format".to_string()));
        }

        let parts: Vec<&str> = proposal_str
            .strip_prefix("ADD_PROPOSAL:")
            .unwrap()
            .split(':')
            .collect();
        if parts.len() != 2 {
            return Err(ClientError::MlsError(
                "Add proposal missing username or public key".to_string(),
            ));
        }

        Ok((parts[0].to_string(), parts[1].to_string()))
    }

    /// Process an incoming Remove proposal
    /// Extracts the username of the removed member
    pub fn process_remove_proposal(&self, proposal_bytes: &[u8]) -> Result<String> {
        let proposal_str = String::from_utf8(proposal_bytes.to_vec()).map_err(|e| {
            ClientError::MlsError(format!("Failed to parse proposal: {}", e))
        })?;

        if !proposal_str.starts_with("REMOVE_PROPOSAL:") {
            return Err(ClientError::MlsError("Invalid Remove proposal format".to_string()));
        }

        let username = proposal_str
            .strip_prefix("REMOVE_PROPOSAL:")
            .unwrap()
            .to_string();

        if username.is_empty() {
            return Err(ClientError::MlsError("Remove proposal missing username".to_string()));
        }

        Ok(username)
    }
}

impl Default for MlsService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mls_service_creation() {
        let _mls = MlsService::new();
        // Service created successfully
    }

    #[test]
    fn test_create_group() -> Result<()> {
        let mls = MlsService::new();
        let (group_id, state) = mls.create_group("test_group")?;
        assert!(!group_id.to_string().is_empty());
        assert!(!state.is_empty());
        assert_eq!(state.len(), 32);
        Ok(())
    }

    #[test]
    fn test_create_group_different_states() -> Result<()> {
        let mls = MlsService::new();
        let (_id1, state1) = mls.create_group("group1")?;
        let (_id2, state2) = mls.create_group("group2")?;
        // States should be different (random)
        assert_ne!(state1, state2);
        Ok(())
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() -> Result<()> {
        let mls = MlsService::new();
        let original = "hello world".to_string();
        let mut state = vec![1, 2, 3];

        let encrypted = mls.encrypt_message(&mut state, original.clone())?;
        assert_ne!(encrypted, original.as_bytes());

        let decrypted = mls.decrypt_message(&mut state, encrypted)?;
        assert_eq!(decrypted, original);
        Ok(())
    }

    #[test]
    fn test_encrypt_different_messages_different_ciphertexts() -> Result<()> {
        let mls = MlsService::new();
        let mut state = vec![1, 2, 3];

        let enc1 = mls.encrypt_message(&mut state, "hello".to_string())?;
        let enc2 = mls.encrypt_message(&mut state, "world".to_string())?;

        assert_ne!(enc1, enc2);
        Ok(())
    }

    #[test]
    fn test_add_member_generates_proposal() -> Result<()> {
        let mls = MlsService::new();
        let proposal = mls.add_member(&[], "bob", "pk_bob")?;
        assert!(!proposal.is_empty());

        // Verify it's parseable as Add proposal
        let (username, pubkey) = mls.process_add_proposal(&proposal)?;
        assert_eq!(username, "bob");
        assert_eq!(pubkey, "pk_bob");
        Ok(())
    }

    #[test]
    fn test_add_member_multiple_users() -> Result<()> {
        let mls = MlsService::new();
        let prop1 = mls.add_member(&[], "alice", "pk_alice")?;
        let prop2 = mls.add_member(&[], "bob", "pk_bob")?;

        let (u1, pk1) = mls.process_add_proposal(&prop1)?;
        let (u2, pk2) = mls.process_add_proposal(&prop2)?;

        assert_eq!(u1, "alice");
        assert_eq!(pk1, "pk_alice");
        assert_eq!(u2, "bob");
        assert_eq!(pk2, "pk_bob");
        Ok(())
    }

    #[test]
    fn test_remove_member_generates_proposal() -> Result<()> {
        let mls = MlsService::new();
        let proposal = mls.remove_member(&[], "alice")?;
        assert!(!proposal.is_empty());

        let username = mls.process_remove_proposal(&proposal)?;
        assert_eq!(username, "alice");
        Ok(())
    }

    #[test]
    fn test_encrypt_empty_message_fails() {
        let mls = MlsService::new();
        let mut state = vec![];
        let result = mls.encrypt_message(&mut state, String::new());
        assert!(result.is_err());
    }

    #[test]
    fn test_add_member_empty_username_fails() {
        let mls = MlsService::new();
        let result = mls.add_member(&[], "", "key");
        assert!(result.is_err());
    }

    #[test]
    fn test_add_member_empty_pubkey_fails() {
        let mls = MlsService::new();
        let result = mls.add_member(&[], "user", "");
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_member_empty_username_fails() {
        let mls = MlsService::new();
        let result = mls.remove_member(&[], "");
        assert!(result.is_err());
    }

    #[test]
    fn test_process_add_proposal_invalid_format() {
        let mls = MlsService::new();
        let invalid = b"INVALID_FORMAT";
        let result = mls.process_add_proposal(invalid);
        assert!(result.is_err());
    }

    #[test]
    fn test_process_remove_proposal_invalid_format() {
        let mls = MlsService::new();
        let invalid = b"INVALID_FORMAT";
        let result = mls.process_remove_proposal(invalid);
        assert!(result.is_err());
    }

    #[test]
    fn test_handle_group_update() -> Result<()> {
        let mls = MlsService::new();
        let update = vec![1, 2, 3, 4, 5];
        let result = mls.handle_group_update(update.clone())?;
        assert_eq!(result, update);
        Ok(())
    }

    #[test]
    fn test_handle_empty_group_update_fails() {
        let mls = MlsService::new();
        let result = mls.handle_group_update(vec![]);
        assert!(result.is_err());
    }
}
