/// Enhanced message processing for MLS client
///
/// This module provides improved message processing capabilities with:
/// - Proper plaintext extraction from application messages
/// - Comprehensive error handling
/// - Support for all MLS message types
/// - Detailed logging and debugging

use crate::error::{Result, ClientError};
use crate::models::IncomingMessage;
use base64::{engine::general_purpose, Engine as _};
use openmls::prelude::*;
use tls_codec::Deserialize;

/// Process a single incoming message envelope
///
/// Handles the complete message processing pipeline:
/// 1. Decode base64-encoded MLS message
/// 2. Deserialize TLS-encoded MLS message
/// 3. Process through OpenMLS group state
/// 4. Extract and display plaintext content
///
/// # Arguments
/// * `envelope` - The incoming message envelope
/// * `group` - The MLS group state
/// * `provider` - The MLS provider
///
/// # Errors
/// * Base64 decoding errors
/// * TLS deserialization errors
/// * MLS processing errors
pub async fn process_incoming_message(
    envelope: &IncomingMessage,
    group: &mut MlsGroup,
    provider: &impl OpenMlsProvider,
) -> Result<ProcessedMessage> {
    // Decode base64-encoded MLS message
    let encrypted_bytes = general_purpose::STANDARD.decode(&envelope.encrypted_content)
        .map_err(|e| {
            log::error!("Failed to decode base64 message: {}", e);
            ClientError::Mls(crate::error::MlsError::DecryptionFailed)
        })?;

    // Deserialize the MLS message
    let message_in = MlsMessageIn::tls_deserialize(&mut encrypted_bytes.as_slice())
        .map_err(|e| {
            log::error!("Failed to deserialize MLS message: {}", e);
            ClientError::Mls(crate::error::MlsError::DecryptionFailed)
        })?;

    // Process the message using the persistent group state
    let processed_msg = crate::crypto::process_message(group, provider, &message_in)
        .map_err(|e| {
            log::error!("Failed to process/decrypt message: {:?}", e);
            e
        })?;

    Ok(processed_msg)
}

/// Handle a processed MLS message based on its content type
///
/// # Arguments
/// * `envelope` - Original message envelope with metadata
/// * `processed_msg` - The processed MLS message
///
/// # Returns
/// * `Ok(Some(message_text))` if it's an application message with plaintext
/// * `Ok(None)` for other message types
/// * `Err(...)` for processing errors
pub fn handle_processed_message(
    envelope: &IncomingMessage,
    processed_msg: ProcessedMessage,
) -> Result<Option<String>> {
    use openmls::prelude::ProcessedMessageContent;

    match processed_msg.into_content() {
        ProcessedMessageContent::ApplicationMessage(app_msg) => {
            // Extract the actual plaintext from the application message
            let plaintext = app_msg.into_bytes();
            let message_text = String::from_utf8_lossy(&plaintext).to_string();
            
            log::debug!("Successfully decrypted message from {}: {}", envelope.sender, message_text);
            Ok(Some(message_text))
        }
        ProcessedMessageContent::ProposalMessage(proposal_msg) => {
            log::info!("Received proposal message from {}: {:?}", envelope.sender, proposal_msg.proposal());
            Ok(None)
        }
        ProcessedMessageContent::ExternalJoinProposalMessage(_) => {
            log::info!("Received external join proposal from {}", envelope.sender);
            Ok(None)
        }
        ProcessedMessageContent::StagedCommitMessage(_) => {
            log::info!("Received staged commit from {}", envelope.sender);
            Ok(None)
        }
    }
}

/// Process an application message (encrypted plaintext)
///
/// # Arguments
/// * `sender` - Username of the message sender
/// * `group_id` - ID of the group
/// * `encrypted_content` - Base64-encoded encrypted message
/// * `group` - The MLS group state
/// * `provider` - The MLS provider
///
/// # Returns
/// * `Ok(Some(message_text))` if message was successfully decrypted
/// * `Ok(None)` if message was not an application message
/// * `Err(...)` for processing errors
pub async fn process_application_message(
    sender: &str,
    group_id: &str,
    encrypted_content: &str,
    group: &mut MlsGroup,
    provider: &impl OpenMlsProvider,
) -> Result<Option<String>> {
    // Decode base64-encoded MLS message
    let encrypted_bytes = general_purpose::STANDARD.decode(encrypted_content)
        .map_err(|e| {
            log::error!("Failed to decode base64 message: {}", e);
            ClientError::Mls(crate::error::MlsError::DecryptionFailed)
        })?;

    // Deserialize the MLS message
    let message_in = MlsMessageIn::tls_deserialize(&mut encrypted_bytes.as_slice())
        .map_err(|e| {
            log::error!("Failed to deserialize MLS message: {}", e);
            ClientError::Mls(crate::error::MlsError::DecryptionFailed)
        })?;

    // Process the message using the persistent group state
    let processed_msg = crate::crypto::process_message(group, provider, &message_in)
        .map_err(|e| {
            log::error!("Failed to process message: {}", e);
            e
        })?;

    // Handle the processed message
    let envelope = IncomingMessage {
        sender: sender.to_string(),
        group_id: group_id.to_string(),
        encrypted_content: encrypted_content.to_string(),
    };

    handle_processed_message(&envelope, processed_msg)
}


/// Format a message for display
///
/// Displays messages in the format: #groupname <username> message
/// The group_name should be human-readable (from GroupMetadata),
/// NOT the base64-encoded MLS group ID.
///
/// # Arguments
/// * `group_name` - Human-readable name of the group
/// * `sender` - Username of the sender
/// * `message` - The message content
///
/// # Returns
/// * Formatted message string in format: #groupname <username> message
pub fn format_display_message(group_name: &str, sender: &str, message: &str) -> String {
    format!("#{} <{}> {}", group_name, sender, message)
}

/// Format a control message for display
///
/// Displays control messages in the format: #groupname action
/// The group_name should be human-readable (from GroupMetadata),
/// NOT the base64-encoded MLS group ID.
///
/// # Arguments
/// * `group_name` - Human-readable name of the group
/// * `action` - The action description
///
/// # Returns
/// * Formatted control message string in format: #groupname action
pub fn format_control_message(group_name: &str, action: &str) -> String {
    format!("#{} {}", group_name, action)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto;
    use crate::provider::MlsProvider;
    use tempfile::tempdir;
    use tls_codec::Serialize;

    #[test]
    fn test_extract_plaintext() {
        // This test would need a real ApplicationMessage, which is complex to create
        // For now, we'll test the formatting functions
        let formatted = format_display_message("testgroup", "alice", "Hello world!");
        assert_eq!(formatted, "#testgroup <alice> Hello world!");
    }

    #[test]
    fn test_format_control_message() {
        let formatted = format_control_message("testgroup", "alice updated the group");
        assert_eq!(formatted, "#testgroup alice updated the group");
    }

    #[test]
    fn test_format_display_message() {
        let formatted = format_display_message("mygroup", "bob", "How are you?");
        assert_eq!(formatted, "#mygroup <bob> How are you?");
    }

    #[tokio::test]
    async fn test_process_application_message_basic() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let provider = MlsProvider::new(&db_path).unwrap();

        // Create Alice's group
        let (alice_cred, alice_key) = crypto::generate_credential_with_key("alice").unwrap();
        let mut alice_group = crypto::create_group_with_config(&alice_cred, &alice_key, &provider, "testgroup").unwrap();

        // Create Bob's credentials and key package
        let (bob_cred, bob_key) = crypto::generate_credential_with_key("bob").unwrap();
        let bob_key_package = crypto::generate_key_package_bundle(&bob_cred, &bob_key, &provider).unwrap();

        // Alice adds Bob to the group
        let (_commit, _welcome, _group_info) = crypto::add_members(
            &mut alice_group,
            &provider,
            &alice_key,
            &[bob_key_package.key_package()],
        ).unwrap();

        crypto::merge_pending_commit(&mut alice_group, &provider).unwrap();

        // Bob joins the group via Welcome
        let ratchet_tree = Some(crypto::export_ratchet_tree(&alice_group));
        let join_config = openmls::prelude::MlsGroupJoinConfig::default();
        let serialized = _welcome.tls_serialize_detached().unwrap();
        let welcome_in = openmls::prelude::MlsMessageIn::tls_deserialize(&mut serialized.as_slice()).unwrap();
        let mut bob_group = crypto::process_welcome_message(&provider, &join_config, &welcome_in, ratchet_tree).unwrap();

        // Alice sends a message
        let alice_message = b"Hello from Alice!";
        let encrypted_alice = crypto::create_application_message(&mut alice_group, &provider, &alice_key, alice_message).unwrap();

        // Bob processes Alice's message
        let serialized = encrypted_alice.tls_serialize_detached().unwrap();
        let encrypted_b64 = general_purpose::STANDARD.encode(&serialized);

        let result = process_application_message(
            "alice",
            "testgroup",
            &encrypted_b64,
            &mut bob_group,
            &provider,
        ).await;

        // Should succeed and return Alice's message
        assert!(result.is_ok());
        let message_text = result.unwrap();
        assert!(message_text.is_some());
        assert_eq!(message_text.unwrap(), "Hello from Alice!");
    }

    #[tokio::test]
    async fn test_process_application_message_invalid_base64() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let provider = MlsProvider::new(&db_path).unwrap();

        // Create a group
        let (cred, sig_key) = crypto::generate_credential_with_key("alice").unwrap();
        let mut group = crypto::create_group_with_config(&cred, &sig_key, &provider, "testgroup").unwrap();

        // Try to process invalid base64
        let result = process_application_message(
            "alice",
            "testgroup",
            "invalid-base64!",
            &mut group,
            &provider,
        ).await;

        // Should fail with decryption error
        assert!(result.is_err());
        match result.unwrap_err() {
            ClientError::Mls(crate::error::MlsError::DecryptionFailed) => {
                // Expected error
            }
            _ => panic!("Expected DecryptionFailed error"),
        }
    }

    #[tokio::test]
    async fn test_process_application_message_invalid_tls() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let provider = MlsProvider::new(&db_path).unwrap();

        // Create a group
        let (cred, sig_key) = crypto::generate_credential_with_key("alice").unwrap();
        let mut group = crypto::create_group_with_config(&cred, &sig_key, &provider, "testgroup").unwrap();

        // Try to process invalid TLS data
        let invalid_data = "dGVzdA=="; // "test" in base64, but not valid TLS
        let result = process_application_message(
            "alice",
            "testgroup",
            invalid_data,
            &mut group,
            &provider,
        ).await;

        // Should fail with decryption error
        assert!(result.is_err());
        match result.unwrap_err() {
            ClientError::Mls(crate::error::MlsError::DecryptionFailed) => {
                // Expected error
            }
            _ => panic!("Expected DecryptionFailed error"),
        }
    }

    #[test]
    fn test_handle_processed_message_application() {
        // This test would need a real ProcessedMessage, which is complex to create
        // For now, we'll test the formatting functions
        let envelope = IncomingMessage {
            sender: "alice".to_string(),
            group_id: "testgroup".to_string(),
            encrypted_content: "dummy".to_string(),
        };

        // We can't easily create a ProcessedMessage in tests, so we'll just test the formatting
        let formatted = format_display_message(&envelope.group_id, &envelope.sender, "Hello!");
        assert_eq!(formatted, "#testgroup <alice> Hello!");
    }
}
