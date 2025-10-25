/// Comprehensive tests for message processing functionality
///
/// Tests cover:
/// - Application message encryption/decryption
/// - Error handling for malformed messages
/// - Message formatting and display
/// - Integration with MLS group state

use mls_chat_client::message_processing::*;
use mls_chat_client::crypto;
use mls_chat_client::provider::MlsProvider;
use tempfile::tempdir;
use tls_codec::{Deserialize, Serialize};
use base64::{engine::general_purpose, Engine as _};

/// Test 1: Basic application message processing
#[tokio::test]
async fn test_process_application_message_success() {
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

/// Test 2: Invalid base64 handling
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
        mls_chat_client::error::ClientError::Mls(mls_chat_client::error::MlsError::DecryptionFailed) => {
            // Expected error
        }
        _ => panic!("Expected DecryptionFailed error"),
    }
}

/// Test 3: Invalid TLS data handling
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
        mls_chat_client::error::ClientError::Mls(mls_chat_client::error::MlsError::DecryptionFailed) => {
            // Expected error
        }
        _ => panic!("Expected DecryptionFailed error"),
    }
}

/// Test 4: Message formatting
#[test]
fn test_format_display_message() {
    let formatted = format_display_message("testgroup", "alice", "Hello world!");
    assert_eq!(formatted, "#testgroup <alice> Hello world!");
}

/// Test 5: Control message formatting
#[test]
fn test_format_control_message() {
    let formatted = format_control_message("testgroup", "alice updated the group");
    assert_eq!(formatted, "#testgroup alice updated the group");
}

/// Test 6: Multi-party message processing
#[tokio::test]
async fn test_multi_party_message_processing() {
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let provider = MlsProvider::new(&db_path).unwrap();

    // Alice creates group
    let (alice_cred, alice_key) = crypto::generate_credential_with_key("alice").unwrap();
    let mut alice_group = crypto::create_group_with_config(&alice_cred, &alice_key, &provider, "testgroup").unwrap();

    // Bob generates key package
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
    let encrypted_b64 = base64::engine::general_purpose::STANDARD.encode(&serialized);

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

    // Bob sends a reply
    let bob_message = b"Hi Alice, nice to meet you!";
    let encrypted_bob = crypto::create_application_message(&mut bob_group, &provider, &bob_key, bob_message).unwrap();

    // Alice processes Bob's message
    let serialized = encrypted_bob.tls_serialize_detached().unwrap();
    let encrypted_b64 = base64::engine::general_purpose::STANDARD.encode(&serialized);

    let result = process_application_message(
        "bob",
        "testgroup",
        &encrypted_b64,
        &mut alice_group,
        &provider,
    ).await;

    // Should succeed and return Bob's message
    assert!(result.is_ok());
    let message_text = result.unwrap();
    assert!(message_text.is_some());
    assert_eq!(message_text.unwrap(), "Hi Alice, nice to meet you!");
}

/// Test 7: Message processing with different content types
#[tokio::test]
async fn test_message_processing_content_types() {
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

    // Test different message types
    let long_message = "Long message: ".repeat(100);
    let messages = vec![
        "Hello world!",
        "This is a test message",
        "Special characters: !@#$%^&*()",
        "Unicode: 🚀🌟✨",
        &long_message,
    ];

    for (i, message) in messages.iter().enumerate() {
        // Alice sends a message
        let encrypted_msg = crypto::create_application_message(&mut alice_group, &provider, &alice_key, message.as_bytes()).unwrap();
        let encrypted_bytes = encrypted_msg.tls_serialize_detached().unwrap();
        let encrypted_b64 = general_purpose::STANDARD.encode(&encrypted_bytes);

        // Bob processes Alice's message
        let result = process_application_message(
            "alice",
            "testgroup",
            &encrypted_b64,
            &mut bob_group,
            &provider,
        ).await;

        assert!(result.is_ok(), "Message {} should process successfully", i);
        let message_text = result.unwrap();
        assert!(message_text.is_some(), "Message {} should return text", i);
        assert_eq!(message_text.unwrap(), *message, "Message {} content should match", i);
    }
}

/// Test 8: Error handling for corrupted messages
#[tokio::test]
async fn test_corrupted_message_handling() {
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let provider = MlsProvider::new(&db_path).unwrap();

    // Create a group
    let (cred, sig_key) = crypto::generate_credential_with_key("alice").unwrap();
    let mut group = crypto::create_group_with_config(&cred, &sig_key, &provider, "testgroup").unwrap();

    // Test various corrupted message scenarios
    let corrupted_messages = vec![
        "", // Empty message
        "a", // Single character
        "invalid", // Not base64
        "dGVzdA==", // Valid base64 but not valid TLS
        "dGVzdA==dGVzdA==", // Multiple base64 blocks
    ];

    for (i, corrupted_msg) in corrupted_messages.iter().enumerate() {
        let result = process_application_message(
            "alice",
            "testgroup",
            corrupted_msg,
            &mut group,
            &provider,
        ).await;

        assert!(result.is_err(), "Corrupted message {} should fail", i);
    }
}

/// Test 9: Message processing with empty content
#[tokio::test]
async fn test_empty_message_processing() {
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

    // Alice sends empty message
    let empty_message = b"";
    let encrypted_msg = crypto::create_application_message(&mut alice_group, &provider, &alice_key, empty_message).unwrap();
    let encrypted_bytes = encrypted_msg.tls_serialize_detached().unwrap();
    let encrypted_b64 = general_purpose::STANDARD.encode(&encrypted_bytes);

    // Bob processes Alice's empty message
    let result = process_application_message(
        "alice",
        "testgroup",
        &encrypted_b64,
        &mut bob_group,
        &provider,
    ).await;

    // Should succeed and return empty string
    assert!(result.is_ok());
    let message_text = result.unwrap();
    assert!(message_text.is_some());
    assert_eq!(message_text.unwrap(), "");
}

/// Test 10: Message processing performance
#[tokio::test]
async fn test_message_processing_performance() {
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

    let start_time = std::time::Instant::now();
    let num_messages = 100;

    // Process multiple messages
    for i in 0..num_messages {
        let message = format!("Message {}", i);
        // Alice sends a message
        let encrypted_msg = crypto::create_application_message(&mut alice_group, &provider, &alice_key, message.as_bytes()).unwrap();
        let encrypted_bytes = encrypted_msg.tls_serialize_detached().unwrap();
        let encrypted_b64 = general_purpose::STANDARD.encode(&encrypted_bytes);

        // Bob processes Alice's message
        let result = process_application_message(
            "alice",
            "testgroup",
            &encrypted_b64,
            &mut bob_group,
            &provider,
        ).await;

        assert!(result.is_ok(), "Message {} should process successfully", i);
        let message_text = result.unwrap();
        assert!(message_text.is_some());
        assert_eq!(message_text.unwrap(), message);
    }

    let elapsed = start_time.elapsed();
    let avg_time = elapsed / num_messages;
    
    // Should process messages reasonably quickly (less than 50ms per message)
    // Note: Two-party setup adds overhead, so we use a more realistic threshold
    assert!(avg_time.as_millis() < 50, "Average processing time should be less than 50ms, got {}ms", avg_time.as_millis());
}
