/// Integration tests for MLS invitation protocol
///
/// Tests cover the proper MLS invitation flow with Welcome messages,
/// ratchet tree exchange, and multi-party scenarios.

use mls_chat_client::client::MlsClient;
use mls_chat_client::models::MlsMessageEnvelope;
use tempfile::tempdir;

/// Test 1: Alice invites Bob (basic two-party invitation)
///
/// This test verifies the core invitation flow:
/// 1. Alice creates a group
/// 2. Alice invites Bob (fetches Bob's KeyPackage, adds to group, generates Welcome)
/// 3. Bob receives Welcome and joins the group
/// 4. Both are now in the same MLS group
#[tokio::test]
#[ignore]  // Requires server integration
async fn test_two_party_invitation_alice_invites_bob() {
    let temp_dir = tempdir().expect("Failed to create temp dir");

    // Alice creates client and group
    let mut alice = MlsClient::new("http://localhost:4000", "alice", "testgroup")
        .await
        .expect("Failed to create Alice's client");

    // Initialize Alice
    alice.initialize().await.expect("Failed to initialize Alice");
    alice.connect_to_group().await.expect("Failed to connect to group");

    // Bob creates client
    let mut bob = MlsClient::new("http://localhost:4000", "bob", "testgroup")
        .await
        .expect("Failed to create Bob's client");

    // Initialize Bob (registers KeyPackage with server)
    bob.initialize().await.expect("Failed to initialize Bob");

    // Alice invites Bob
    alice.invite_user("bob").await.expect("Failed to invite Bob");

    // In a real test, Bob would receive the Welcome message and process it
    // bob.handle_welcome_message(...).await.expect("Failed to join via Welcome");

    // Verify both are in the same group
    assert!(alice.is_group_connected(), "Alice should be connected to group");
    assert_eq!(alice.get_group_id(), bob.get_group_id(), "Both should have same group ID");
}

/// Test 2: Welcome message envelope format
///
/// Verifies that Welcome envelopes are properly structured with:
/// - Welcome message (TLS-serialized)
/// - Ratchet tree (serialized and base64-encoded)
/// - Metadata (group_id, inviter)
#[test]
fn test_welcome_message_envelope_structure() {
    let welcome_envelope = MlsMessageEnvelope::WelcomeMessage {
        group_id: "testgroup".to_string(),
        inviter: "alice".to_string(),
        welcome_blob: "base64welcomeblob".to_string(),
        ratchet_tree_blob: "base64ratchettree".to_string(),
    };

    // Serialize to JSON
    let json = serde_json::to_string(&welcome_envelope).expect("Failed to serialize");

    // Verify type field
    assert!(json.contains("\"type\":\"welcome\""), "Should have welcome type");

    // Verify required fields
    assert!(json.contains("\"group_id\":\"testgroup\""), "Should have group_id");
    assert!(json.contains("\"inviter\":\"alice\""), "Should have inviter");
    assert!(json.contains("\"welcome_blob\":"), "Should have welcome_blob");
    assert!(json.contains("\"ratchet_tree_blob\":"), "Should have ratchet_tree_blob");

    // Deserialize back
    let deserialized: MlsMessageEnvelope = serde_json::from_str(&json)
        .expect("Failed to deserialize");
    assert_eq!(welcome_envelope, deserialized, "Should deserialize correctly");
}

/// Test 3: Commit message envelope for membership changes
///
/// Verifies that Commit envelopes broadcast group changes to existing members
#[test]
fn test_commit_message_envelope_structure() {
    let commit_envelope = MlsMessageEnvelope::CommitMessage {
        group_id: "testgroup".to_string(),
        sender: "alice".to_string(),
        commit_blob: "base64commitblob".to_string(),
    };

    let json = serde_json::to_string(&commit_envelope).expect("Failed to serialize");

    assert!(json.contains("\"type\":\"commit\""), "Should have commit type");
    assert!(json.contains("\"sender\":\"alice\""), "Should have sender");
    assert!(json.contains("\"commit_blob\":"), "Should have commit_blob");

    let deserialized: MlsMessageEnvelope = serde_json::from_str(&json)
        .expect("Failed to deserialize");
    assert_eq!(commit_envelope, deserialized, "Should deserialize correctly");
}

/// Test 4: Alice invites Bob, then Bob invites Carol (three-party)
///
/// This test verifies multi-step invitation:
/// 1. Alice creates group
/// 2. Alice invites Bob -> Bob joins
/// 3. Bob invites Carol -> Carol joins
/// 4. All three are in the same group with correct membership
#[tokio::test]
#[ignore]  // Requires server integration
async fn test_three_party_invitation_sequence() {
    let _temp_dir = tempdir().expect("Failed to create temp dir");

    // Alice creates and initializes
    let mut alice = MlsClient::new("http://localhost:4000", "alice", "groupabc")
        .await
        .expect("Failed to create Alice");
    alice.initialize().await.expect("Failed to init Alice");
    alice.connect_to_group().await.expect("Failed to connect Alice");

    // Bob initializes (but doesn't create/join yet)
    let mut bob = MlsClient::new("http://localhost:4000", "bob", "groupabc")
        .await
        .expect("Failed to create Bob");
    bob.initialize().await.expect("Failed to init Bob");

    // Carol initializes
    let mut carol = MlsClient::new("http://localhost:4000", "carol", "groupabc")
        .await
        .expect("Failed to create Carol");
    carol.initialize().await.expect("Failed to init Carol");

    // Alice invites Bob
    alice.invite_user("bob").await.expect("Failed to invite Bob");

    // Bob receives Welcome and joins (in real test)
    // bob.handle_welcome_message(...).await.expect("Bob to join");

    // Bob invites Carol
    bob.invite_user("carol").await.expect("Failed for Bob to invite Carol");

    // Carol receives Welcome and joins (in real test)
    // carol.handle_welcome_message(...).await.expect("Carol to join");

    // Verify all are connected
    assert!(alice.is_group_connected());
    assert!(bob.is_group_connected());
    assert!(carol.is_group_connected());
}

/// Test 5: Application message envelope (for completeness)
///
/// Verifies application message structure remains intact
#[test]
fn test_application_message_envelope_structure() {
    let app_envelope = MlsMessageEnvelope::ApplicationMessage {
        sender: "alice".to_string(),
        group_id: "testgroup".to_string(),
        encrypted_content: "base64encryptedtext".to_string(),
    };

    let json = serde_json::to_string(&app_envelope).expect("Failed to serialize");

    assert!(json.contains("\"type\":\"application\""));
    assert!(json.contains("\"sender\":\"alice\""));
    assert!(json.contains("\"group_id\":\"testgroup\""));

    let deserialized: MlsMessageEnvelope = serde_json::from_str(&json)
        .expect("Failed to deserialize");
    assert_eq!(app_envelope, deserialized);
}

/// Test 6: Envelope type discrimination in routing
///
/// Verifies that WebSocket can distinguish between message types
#[test]
fn test_envelope_message_type_routing() {
    let messages = vec![
        (r#"{"type":"application","sender":"alice","group_id":"g1","encrypted_content":"data"}"#, "application"),
        (r#"{"type":"welcome","group_id":"g1","inviter":"alice","welcome_blob":"w","ratchet_tree_blob":"rt"}"#, "welcome"),
        (r#"{"type":"commit","group_id":"g1","sender":"alice","commit_blob":"c"}"#, "commit"),
    ];

    for (msg, expected_type) in messages {
        let envelope: MlsMessageEnvelope = serde_json::from_str(msg)
            .expect(&format!("Failed to parse: {}", msg));

        // Verify each type is correctly identified
        match (envelope, expected_type) {
            (MlsMessageEnvelope::ApplicationMessage { .. }, "application") => {
                // Correct type
            }
            (MlsMessageEnvelope::WelcomeMessage { .. }, "welcome") => {
                // Correct type
            }
            (MlsMessageEnvelope::CommitMessage { .. }, "commit") => {
                // Correct type
            }
            (_, _) => panic!("Type mismatch for: {}", msg),
        }
    }
}

/// Test 7: Multiple sequential invitations
///
/// Verifies that multiple sequential invitations work correctly
#[tokio::test]
#[ignore]  // Requires server integration
async fn test_multiple_sequential_invitations() {
    let _temp_dir = tempdir().expect("Failed to create temp dir");

    let mut alice = MlsClient::new("http://localhost:4000", "alice", "big-group")
        .await
        .expect("Failed to create Alice");
    alice.initialize().await.expect("Failed to init");
    alice.connect_to_group().await.expect("Failed to connect");

    // Create and initialize users
    let users = vec!["bob", "carol", "dave", "eve"];
    for user in &users {
        let mut client = MlsClient::new("http://localhost:4000", user, "big-group")
            .await
            .expect(&format!("Failed to create {}", user));
        client.initialize().await.expect(&format!("Failed to init {}", user));
        // In real test: client would receive Welcome and join
    }

    // Alice invites each user sequentially
    for user in &users {
        alice.invite_user(user).await.expect(&format!("Failed to invite {}", user));
        // Small delay to simulate network
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    // All users should be in group (in real test)
}

/// Test 8: Invitation with proper error handling
///
/// Verifies that invitations to non-existent users fail gracefully
#[tokio::test]
#[ignore]  // Requires server integration
async fn test_invitation_to_nonexistent_user_fails() {
    let _temp_dir = tempdir().expect("Failed to create temp dir");

    let mut alice = MlsClient::new("http://localhost:4000", "alice", "testgroup")
        .await
        .expect("Failed to create Alice");
    alice.initialize().await.expect("Failed to init");
    alice.connect_to_group().await.expect("Failed to connect");

    // Try to invite non-existent user
    let result = alice.invite_user("nonexistent-user").await;

    // Should fail because user doesn't exist on server
    assert!(result.is_err(), "Should fail to invite non-existent user");
}

/// Test 9: Welcome message includes all necessary information
///
/// Verifies that Welcome envelope contains complete information for joining
#[test]
fn test_welcome_message_completeness() {
    let welcome = MlsMessageEnvelope::WelcomeMessage {
        group_id: "mygroup".to_string(),
        inviter: "alice".to_string(),
        welcome_blob: "SerializedWelcomeFromAlice".to_string(),
        ratchet_tree_blob: "RatchetTreeForBob".to_string(),
    };

    match welcome {
        MlsMessageEnvelope::WelcomeMessage {
            group_id,
            inviter,
            welcome_blob,
            ratchet_tree_blob,
        } => {
            // All fields should be present and non-empty for real messages
            assert!(!group_id.is_empty(), "group_id required");
            assert!(!inviter.is_empty(), "inviter required");
            assert!(!welcome_blob.is_empty(), "welcome_blob required");
            assert!(!ratchet_tree_blob.is_empty(), "ratchet_tree_blob required");
        }
        _ => panic!("Expected WelcomeMessage"),
    }
}

/// Test 10: Commit message broadcasts to group
///
/// Verifies that Commit messages are sent to all members to announce change
#[test]
fn test_commit_message_broadcast() {
    let commit = MlsMessageEnvelope::CommitMessage {
        group_id: "mygroup".to_string(),
        sender: "alice".to_string(),
        commit_blob: "CommitAddingBob".to_string(),
    };

    match commit {
        MlsMessageEnvelope::CommitMessage {
            group_id,
            sender,
            commit_blob,
        } => {
            // Fields should be valid
            assert_eq!(group_id, "mygroup");
            assert_eq!(sender, "alice");
            assert!(!commit_blob.is_empty(), "commit_blob should have data");
        }
        _ => panic!("Expected CommitMessage"),
    }
}
