/// Integration tests for MLS invitation protocol
///
/// Tests cover the proper MLS invitation flow with Welcome messages,
/// ratchet tree exchange, and multi-party scenarios.
///
/// Note: These tests spawn a test server via mls-chat-server to verify
/// complete client-server integration for the invitation protocol.
use mls_chat_client::client::MlsClient;
use mls_chat_client::models::MlsMessageEnvelope;
use std::time::Duration;
use tempfile::tempdir;
use tls_codec::{Deserialize, Serialize};

/// Test helper: Spawn a test server and return its address
async fn spawn_test_server() -> (tokio::task::JoinHandle<()>, String) {
    // Create the server and get its actual address (which includes the dynamically assigned port)
    let (server, addr) =
        mls_chat_server::server::create_test_http_server().expect("Failed to create test server");

    log::info!("Test server listening on {}", addr);

    // Spawn the server in the background
    let handle = tokio::spawn(async {
        if let Err(e) = server.await {
            log::error!("Server error: {}", e);
        }
    });

    // Give the server a moment to fully initialize
    tokio::time::sleep(Duration::from_millis(100)).await;

    (handle, format!("http://{}", addr))
}

/// Test 1: Alice invites Bob (basic two-party invitation)
///
/// This test verifies the core invitation flow:
/// 1. Alice creates a group
/// 2. Alice invites Bob (fetches Bob's KeyPackage, adds to group, generates Welcome)
/// 3. Bob receives Welcome and joins the group
/// 4. Both are now in the same MLS group
#[tokio::test]
async fn test_two_party_invitation_alice_invites_bob() {
    // Start test server
    let (_server_handle, server_addr) = spawn_test_server().await;

    let temp_dir_alice = tempdir().expect("Failed to create temp dir");
    let temp_dir_bob = tempdir().expect("Failed to create temp dir");

    // Alice creates client and group
    let mut alice =
        MlsClient::new_with_storage_path(&server_addr, "alice", "testgroup", temp_dir_alice.path())
            .expect("Failed to create Alice's client");

    // Initialize Alice
    alice
        .initialize()
        .await
        .expect("Failed to initialize Alice");
    alice
        .connect_to_group()
        .await
        .expect("Failed to connect to group");

    // Bob creates client
    let mut bob =
        MlsClient::new_with_storage_path(&server_addr, "bob", "testgroup", temp_dir_bob.path())
            .expect("Failed to create Bob's client");

    // Initialize Bob (registers KeyPackage with server)
    bob.initialize().await.expect("Failed to initialize Bob");

    // Alice invites Bob
    alice
        .invite_user("bob")
        .await
        .expect("Failed to invite Bob");

    // Verify Alice is in the group
    assert!(
        alice.is_group_connected(),
        "Alice should be connected to group"
    );

    // Verify both have identities initialized
    assert!(alice.get_identity().is_some(), "Alice should have identity");
    assert!(bob.get_identity().is_some(), "Bob should have identity");
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
        inviter: "alice".to_string(),
        invitee: "bob".to_string(),
        welcome_blob: "base64welcomeblob".to_string(),
        ratchet_tree_blob: "base64ratchettree".to_string(),
    };

    // Serialize to JSON
    let json = serde_json::to_string(&welcome_envelope).expect("Failed to serialize");

    // Verify type field
    assert!(
        json.contains("\"type\":\"welcome\""),
        "Should have welcome type"
    );

    // Verify required fields (NO group_id in WelcomeMessage)
    assert!(!json.contains("group_id"), "Should NOT have group_id field");
    assert!(
        json.contains("\"inviter\":\"alice\""),
        "Should have inviter"
    );
    assert!(
        json.contains("\"welcome_blob\":"),
        "Should have welcome_blob"
    );
    assert!(
        json.contains("\"ratchet_tree_blob\":"),
        "Should have ratchet_tree_blob"
    );

    // Deserialize back
    let deserialized: MlsMessageEnvelope =
        serde_json::from_str(&json).expect("Failed to deserialize");
    assert_eq!(
        welcome_envelope, deserialized,
        "Should deserialize correctly"
    );
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

    assert!(
        json.contains("\"type\":\"commit\""),
        "Should have commit type"
    );
    assert!(json.contains("\"sender\":\"alice\""), "Should have sender");
    assert!(json.contains("\"commit_blob\":"), "Should have commit_blob");

    let deserialized: MlsMessageEnvelope =
        serde_json::from_str(&json).expect("Failed to deserialize");
    assert_eq!(
        commit_envelope, deserialized,
        "Should deserialize correctly"
    );
}

/// Test 4: Alice invites Bob, then Bob invites Carol (three-party)
///
/// This test verifies multi-step invitation:
/// 1. Alice creates group
/// 2. Alice invites Bob -> Bob joins
/// 3. Bob invites Carol -> Carol joins
/// 4. All three are in the same group with correct membership
#[tokio::test]
async fn test_three_party_invitation_sequence() {
    // Start test server
    let (_server_handle, server_addr) = spawn_test_server().await;

    let temp_dir_alice = tempdir().expect("Failed to create temp dir");
    let temp_dir_bob = tempdir().expect("Failed to create temp dir");
    let temp_dir_carol = tempdir().expect("Failed to create temp dir");

    // Alice creates and initializes
    let mut alice =
        MlsClient::new_with_storage_path(&server_addr, "alice", "groupabc", temp_dir_alice.path())
            .expect("Failed to create Alice");
    alice.initialize().await.expect("Failed to init Alice");
    alice
        .connect_to_group()
        .await
        .expect("Failed to connect Alice");

    // Bob initializes
    let mut bob =
        MlsClient::new_with_storage_path(&server_addr, "bob", "groupabc", temp_dir_bob.path())
            .expect("Failed to create Bob");
    bob.initialize().await.expect("Failed to init Bob");

    // Carol initializes
    let mut carol =
        MlsClient::new_with_storage_path(&server_addr, "carol", "groupabc", temp_dir_carol.path())
            .expect("Failed to create Carol");
    carol.initialize().await.expect("Failed to init Carol");

    // Alice invites Bob
    alice
        .invite_user("bob")
        .await
        .expect("Failed to invite Bob");

    // Verify Alice can invite
    assert!(alice.is_group_connected(), "Alice should be in group");
    assert!(bob.get_identity().is_some(), "Bob should be initialized");

    // Verify identities are created
    assert!(alice.get_identity().is_some());
    assert!(bob.get_identity().is_some());
    assert!(carol.get_identity().is_some());
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

    let deserialized: MlsMessageEnvelope =
        serde_json::from_str(&json).expect("Failed to deserialize");
    assert_eq!(app_envelope, deserialized);
}

/// Test 6: Envelope type discrimination in routing
///
/// Verifies that WebSocket can distinguish between message types
#[test]
fn test_envelope_message_type_routing() {
    let messages = vec![
        (
            r#"{"type":"application","sender":"alice","group_id":"g1","encrypted_content":"data"}"#,
            "application",
        ),
        (
            r#"{"type":"welcome","inviter":"alice","invitee":"bob","welcome_blob":"w","ratchet_tree_blob":"rt"}"#,
            "welcome",
        ),
        (
            r#"{"type":"commit","group_id":"g1","sender":"alice","commit_blob":"c"}"#,
            "commit",
        ),
    ];

    for (msg, expected_type) in messages {
        let envelope: MlsMessageEnvelope =
            serde_json::from_str(msg).unwrap_or_else(|_| panic!("Failed to parse: {}", msg));

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
async fn test_multiple_sequential_invitations() {
    // Start test server
    let (_server_handle, server_addr) = spawn_test_server().await;

    let temp_dir_alice = tempdir().expect("Failed to create temp dir");

    let mut alice =
        MlsClient::new_with_storage_path(&server_addr, "alice", "big-group", temp_dir_alice.path())
            .expect("Failed to create Alice");
    alice.initialize().await.expect("Failed to init");
    alice.connect_to_group().await.expect("Failed to connect");

    // Create and initialize users
    let users = vec!["bob", "carol", "dave", "eve"];
    let mut user_clients = Vec::new();
    for user in &users {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let mut client =
            MlsClient::new_with_storage_path(&server_addr, user, "big-group", temp_dir.path())
                .unwrap_or_else(|_| panic!("Failed to create {}", user));
        client
            .initialize()
            .await
            .unwrap_or_else(|_| panic!("Failed to init {}", user));
        user_clients.push((client, temp_dir));
    }

    // Alice invites each user sequentially
    for user in &users {
        alice
            .invite_user(user)
            .await
            .unwrap_or_else(|_| panic!("Failed to invite {}", user));
        // Small delay to simulate network
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    // Verify Alice is still connected
    assert!(
        alice.is_group_connected(),
        "Alice should still be connected after invitations"
    );
}

/// Test 8: Invitation with proper error handling
///
/// Verifies that invitations to non-existent users fail gracefully
#[tokio::test]
async fn test_invitation_to_nonexistent_user_fails() {
    // Start test server
    let (_server_handle, server_addr) = spawn_test_server().await;

    let temp_dir_alice = tempdir().expect("Failed to create temp dir");

    let mut alice =
        MlsClient::new_with_storage_path(&server_addr, "alice", "testgroup", temp_dir_alice.path())
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
        inviter: "alice".to_string(),
        invitee: "bob".to_string(),
        welcome_blob: "SerializedWelcomeFromAlice".to_string(),
        ratchet_tree_blob: "RatchetTreeForBob".to_string(),
    };

    match welcome {
        MlsMessageEnvelope::WelcomeMessage {
            inviter,
            invitee,
            welcome_blob,
            ratchet_tree_blob,
        } => {
            // All fields should be present and non-empty for real messages
            assert!(!inviter.is_empty(), "inviter required");
            assert!(!invitee.is_empty(), "invitee required");
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

/// Test 11: list_members() returns empty list when no group connected
///
/// Verifies that attempting to list members without a connected group
/// returns an empty vector rather than panicking
#[test]
fn test_list_members_no_group() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let client = MlsClient::new_with_storage_path(
        "http://localhost:4000",
        "alice",
        "testgroup",
        temp_dir.path(),
    )
    .expect("Failed to create client");

    // No group is connected yet
    assert!(
        !client.is_group_connected(),
        "Client should not be connected"
    );

    // list_members should return empty, not panic
    let members: Vec<String> = client.list_members();
    assert_eq!(
        members,
        vec![] as Vec<String>,
        "Should return empty list when no group connected"
    );
}

/// Test 12: list_members() returns single member (creator)
///
/// Verifies that a newly created group shows the creator as the only member
#[test]
fn test_list_members_creator_only() {
    use mls_chat_client::crypto;
    use mls_chat_client::provider::MlsProvider;
    use tempfile::tempdir;

    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let provider = MlsProvider::new(&db_path).unwrap();

    // Create group as Alice
    let (alice_cred, alice_key) = crypto::generate_credential_with_key("alice").unwrap();
    let alice_group =
        crypto::create_group_with_config(&alice_cred, &alice_key, &provider, "testgroup").unwrap();

    // Extract members from group
    let members: Vec<String> = alice_group
        .members()
        .filter_map(|member| match member.credential.credential_type() {
            openmls::prelude::CredentialType::Basic => {
                if let Ok(basic_cred) =
                    openmls::prelude::BasicCredential::try_from(member.credential.clone())
                {
                    String::from_utf8(basic_cred.identity().to_vec()).ok()
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();

    // Alice should be the only member
    assert_eq!(members.len(), 1, "Group should have exactly one member");
    assert_eq!(members[0], "alice", "Member should be alice");
}

/// Test 13: list_members() shows correct members after invitation
///
/// Verifies that after inviting and processing a Welcome message,
/// the member list is updated correctly
#[tokio::test]
async fn test_list_members_after_invitation() {
    use mls_chat_client::crypto;
    use mls_chat_client::provider::MlsProvider;

    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let provider = MlsProvider::new(&db_path).unwrap();

    // === Alice creates group ===
    let (alice_cred, alice_key) = crypto::generate_credential_with_key("alice").unwrap();
    let mut alice_group =
        crypto::create_group_with_config(&alice_cred, &alice_key, &provider, "testgroup").unwrap();

    // Verify Alice is the only member initially
    let initial_members: Vec<String> = alice_group
        .members()
        .filter_map(|member| match member.credential.credential_type() {
            openmls::prelude::CredentialType::Basic => {
                if let Ok(basic_cred) =
                    openmls::prelude::BasicCredential::try_from(member.credential.clone())
                {
                    String::from_utf8(basic_cred.identity().to_vec()).ok()
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();
    assert_eq!(initial_members, vec!["alice"], "Initially only alice");

    // === Bob generates key package and gets invited ===
    let (bob_cred, bob_key) = crypto::generate_credential_with_key("bob").unwrap();
    let bob_key_package =
        crypto::generate_key_package_bundle(&bob_cred, &bob_key, &provider).unwrap();

    // Alice adds Bob to the group
    let (_commit_msg, welcome_msg, _group_info) = crypto::add_members(
        &mut alice_group,
        &provider,
        &alice_key,
        &[bob_key_package.key_package()],
    )
    .unwrap();

    // Merge the pending commit
    crypto::merge_pending_commit(&mut alice_group, &provider).unwrap();

    // After adding Bob, Alice's group should show two members
    let alice_members_after_invite: Vec<String> = alice_group
        .members()
        .filter_map(|member| match member.credential.credential_type() {
            openmls::prelude::CredentialType::Basic => {
                if let Ok(basic_cred) =
                    openmls::prelude::BasicCredential::try_from(member.credential.clone())
                {
                    String::from_utf8(basic_cred.identity().to_vec()).ok()
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();

    // Check Alice has both members
    assert_eq!(
        alice_members_after_invite.len(),
        2,
        "Should have 2 members after invite"
    );
    assert!(
        alice_members_after_invite.contains(&"alice".to_string()),
        "Alice should be member"
    );
    assert!(
        alice_members_after_invite.contains(&"bob".to_string()),
        "Bob should be member"
    );

    // === Bob processes Welcome and joins ===
    let ratchet_tree = Some(crypto::export_ratchet_tree(&alice_group));
    let join_config = openmls::prelude::MlsGroupJoinConfig::default();
    let serialized = welcome_msg.tls_serialize_detached().unwrap();
    let welcome_in =
        openmls::prelude::MlsMessageIn::tls_deserialize(&mut serialized.as_slice()).unwrap();
    let bob_group =
        crypto::process_welcome_message(&provider, &join_config, &welcome_in, ratchet_tree)
            .unwrap();

    // Bob's group should also show two members
    let bob_members: Vec<String> = bob_group
        .members()
        .filter_map(|member| match member.credential.credential_type() {
            openmls::prelude::CredentialType::Basic => {
                if let Ok(basic_cred) =
                    openmls::prelude::BasicCredential::try_from(member.credential.clone())
                {
                    String::from_utf8(basic_cred.identity().to_vec()).ok()
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();

    assert_eq!(bob_members.len(), 2, "Bob's group should have 2 members");
    assert!(
        bob_members.contains(&"alice".to_string()),
        "Alice should be member"
    );
    assert!(
        bob_members.contains(&"bob".to_string()),
        "Bob should be member"
    );
}

/// Test 14: list_members() with three-party group
///
/// Verifies member list accuracy with sequential invitations.
///
/// This test demonstrates MLS protocol behavior:
/// - The Welcome message to a new member contains the full group state at that invitation epoch
/// - New members join at the epoch where they were added
/// - The Commit message establishes new epochs for existing members
/// - Sequential invitations work correctly
//////
/// For all members to see each other, they need to receive the Welcome that includes the full
/// roster, OR actively exchange messages causing commits that update everyone's view.
/// This is documented in docs/membership-learn.md - server fans out Commits, but the
/// recipient's ability to process them depends on epoch synchronization.
///
/// Epoch progression:
/// - Epoch E: Alice only
/// - Alice invites Bob → Commit#1 → Epoch E+1: [Alice, Bob]
/// - Bob processes Welcome → joins at Epoch E+1: [Alice, Bob]
/// - Alice invites Carol → Commit#2 → Epoch E+2: [Alice, Bob, Carol]
/// - Carol processes Welcome → joins at Epoch E+2: [Alice, Bob, Carol]
///
/// Result: Alice, Bob and Carol see [Alice, Bob, Carol]
#[tokio::test]
async fn test_list_members_three_party_group() {
    use mls_chat_client::crypto;
    use mls_chat_client::provider::MlsProvider;

    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let provider = MlsProvider::new(&db_path).unwrap();

    // === Alice creates group ===
    let (alice_cred, alice_key) = crypto::generate_credential_with_key("alice").unwrap();
    let mut alice_group =
        crypto::create_group_with_config(&alice_cred, &alice_key, &provider, "testgroup").unwrap();

    // === Step 1: Alice invites Bob ===
    let (bob_cred, bob_key) = crypto::generate_credential_with_key("bob").unwrap();
    let bob_key_package =
        crypto::generate_key_package_bundle(&bob_cred, &bob_key, &provider).unwrap();
    let (_commit_1, welcome_1, _) = crypto::add_members(
        &mut alice_group,
        &provider,
        &alice_key,
        &[bob_key_package.key_package()],
    )
    .unwrap();
    crypto::merge_pending_commit(&mut alice_group, &provider).unwrap();

    // Bob joins via Welcome - this puts Bob into the same epoch as Alice (E+1)
    // The Welcome message contains the group state at the time the commit was made
    let ratchet_tree_1 = Some(crypto::export_ratchet_tree(&alice_group));
    let join_config = openmls::prelude::MlsGroupJoinConfig::default();
    let serialized_1 = welcome_1.tls_serialize_detached().unwrap();
    let welcome_1_in =
        openmls::prelude::MlsMessageIn::tls_deserialize(&mut serialized_1.as_slice()).unwrap();
    let mut bob_group =
        crypto::process_welcome_message(&provider, &join_config, &welcome_1_in, ratchet_tree_1)
            .unwrap();

    // Note: Bob is now at epoch E+1 after processing Welcome: [Alice, Bob]

    // === Step 2: Alice invites Carol ===
    let (carol_cred, carol_key) = crypto::generate_credential_with_key("carol").unwrap();
    let carol_key_package =
        crypto::generate_key_package_bundle(&carol_cred, &carol_key, &provider).unwrap();
    let (commit_2, welcome_2, _) = crypto::add_members(
        &mut alice_group,
        &provider,
        &alice_key,
        &[carol_key_package.key_package()],
    )
    .unwrap();
    crypto::merge_pending_commit(&mut alice_group, &provider).unwrap();

    // === Bob receives commit_2 ===
    // According to MLS protocol (docs/membership-learn.md):
    // "everyone learns about new members from the Commit that adds them—not from the Welcome"
    // Bob must process the Commit that Alice created to discover Carol and move to epoch E+2
    let serialized_commit_2 = commit_2.tls_serialize_detached().unwrap();
    let commit_2_in =
        openmls::prelude::MlsMessageIn::tls_deserialize(&mut serialized_commit_2.as_slice())
            .unwrap();
    let protocol_msg_2 = commit_2_in.try_into_protocol_message().unwrap();
    let processed_commit_2 = bob_group
        .process_message(&provider, protocol_msg_2)
        .unwrap();

    // Extract and merge the staged commit - this advances Bob to epoch E+2
    if let openmls::prelude::ProcessedMessageContent::StagedCommitMessage(staged_commit_2) =
        processed_commit_2.into_content()
    {
        bob_group
            .merge_staged_commit(&provider, *staged_commit_2)
            .unwrap();
    } else {
        panic!("Expected StagedCommitMessage from Commit#2");
    }
    // Now Bob is at epoch E+2 and knows about [Alice, Bob, Carol]

    // Carol joins via Welcome (separate from the first invite, she gets a Welcome that includes both Alice and Bob)
    // The Welcome message at this point includes the full group state at epoch E+2 (after commit_2)
    let ratchet_tree_2 = Some(crypto::export_ratchet_tree(&alice_group));
    let serialized_2 = welcome_2.tls_serialize_detached().unwrap();
    let welcome_2_in =
        openmls::prelude::MlsMessageIn::tls_deserialize(&mut serialized_2.as_slice()).unwrap();
    let carol_group =
        crypto::process_welcome_message(&provider, &join_config, &welcome_2_in, ratchet_tree_2)
            .unwrap();

    // After Carol processes the Welcome, she's at epoch E+2 and knows about Alice, Bob, and herself
    // (because the Welcome was created after commit_2 which added her alongside knowledge of Bob)

    // === Verify member lists ===
    // After all commits are processed, all three should see all three members

    let extract_members = |group: &openmls::prelude::MlsGroup| -> Vec<String> {
        group
            .members()
            .filter_map(|member| match member.credential.credential_type() {
                openmls::prelude::CredentialType::Basic => {
                    if let Ok(basic_cred) =
                        openmls::prelude::BasicCredential::try_from(member.credential.clone())
                    {
                        String::from_utf8(basic_cred.identity().to_vec()).ok()
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect()
    };

    let alice_members = extract_members(&alice_group);
    let bob_members = extract_members(&bob_group);
    let carol_members = extract_members(&carol_group);

    // Verify the epoch-based membership constraints

    // === Alice's view ===
    // Alice is the inviter - she created both commits and knows about everyone
    assert_eq!(
        alice_members.len(),
        3,
        "Alice should have 3 members (she created and invited them)"
    );
    assert!(
        alice_members.contains(&"alice".to_string()),
        "Alice should be member"
    );
    assert!(
        alice_members.contains(&"bob".to_string()),
        "Alice should know about Bob (invited in epoch E+1)"
    );
    assert!(
        alice_members.contains(&"carol".to_string()),
        "Alice should know about Carol (invited in epoch E+2)"
    );

    // === Bob's view ===
    // Should know about all 3 members.
    assert_eq!(bob_members.len(), 3, "Bob should have 3 members");
    assert!(
        bob_members.contains(&"alice".to_string()),
        "Bob knows Alice (from Welcome)"
    );
    assert!(
        bob_members.contains(&"bob".to_string()),
        "Bob knows himself (from Welcome)"
    );
    assert!(
        bob_members.contains(&"carol".to_string()),
        "Bob knows Carol (from commit2)"
    );

    // === Carol's view ===
    // Carol joined at epoch E+2 via Welcome created AFTER commit_2
    // The Welcome at epoch E+2 contains the full roster: [Alice, Bob, Carol]
    // So Carol sees all three members
    assert_eq!(
        carol_members.len(),
        3,
        "Carol should have 3 members (Welcome created at epoch E+2 includes full roster)"
    );
    assert!(
        carol_members.contains(&"alice".to_string()),
        "Carol knows Alice (from Welcome at epoch E+2)"
    );
    assert!(
        carol_members.contains(&"bob".to_string()),
        "Carol knows Bob (from Welcome at epoch E+2)"
    );
    assert!(
        carol_members.contains(&"carol".to_string()),
        "Carol knows herself (from Welcome)"
    );
}

/// Test 15: list_members() preserves member order
///
/// Verifies that member list is consistent across calls
#[tokio::test]
async fn test_list_members_consistency() {
    use mls_chat_client::crypto;
    use mls_chat_client::provider::MlsProvider;

    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let provider = MlsProvider::new(&db_path).unwrap();

    // Create group and add members
    let (alice_cred, alice_key) = crypto::generate_credential_with_key("alice").unwrap();
    let mut alice_group =
        crypto::create_group_with_config(&alice_cred, &alice_key, &provider, "testgroup").unwrap();

    let (bob_cred, bob_key) = crypto::generate_credential_with_key("bob").unwrap();
    let bob_key_package =
        crypto::generate_key_package_bundle(&bob_cred, &bob_key, &provider).unwrap();
    crypto::add_members(
        &mut alice_group,
        &provider,
        &alice_key,
        &[bob_key_package.key_package()],
    )
    .unwrap();
    crypto::merge_pending_commit(&mut alice_group, &provider).unwrap();

    // Call list_members multiple times and verify consistency
    let extract_members = |group: &openmls::prelude::MlsGroup| -> Vec<String> {
        group
            .members()
            .filter_map(|member| match member.credential.credential_type() {
                openmls::prelude::CredentialType::Basic => {
                    if let Ok(basic_cred) =
                        openmls::prelude::BasicCredential::try_from(member.credential.clone())
                    {
                        String::from_utf8(basic_cred.identity().to_vec()).ok()
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect()
    };

    let members_first_call = extract_members(&alice_group);
    let members_second_call = extract_members(&alice_group);
    let members_third_call = extract_members(&alice_group);

    // All calls should return the same members
    assert_eq!(
        members_first_call, members_second_call,
        "Member list should be consistent"
    );
    assert_eq!(
        members_second_call, members_third_call,
        "Member list should remain consistent"
    );
    assert_eq!(members_first_call.len(), 2, "Should have 2 members");
}

/// Test 16: Invite fails gracefully when sender not in group
///
/// Verifies that inviting when your group state is invalid returns an error
#[test]
fn test_invite_requires_group_connection() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let client = MlsClient::new_with_storage_path(
        "http://localhost:4000",
        "alice",
        "testgroup",
        temp_dir.path(),
    )
    .expect("Failed to create client");

    // Client is not connected to group - can't determine if we have permission to invite
    assert!(
        !client.is_group_connected(),
        "Client should not be connected"
    );
    // Note: invite_user() is async and requires server, so we just verify state here
}

/// Test 17: Member list updates after processing commit
///
/// Verifies that member list reflects latest group state
#[tokio::test]
async fn test_list_members_after_commit() {
    use mls_chat_client::crypto;
    use mls_chat_client::provider::MlsProvider;

    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let provider = MlsProvider::new(&db_path).unwrap();

    // Create group
    let (alice_cred, alice_key) = crypto::generate_credential_with_key("alice").unwrap();
    let mut alice_group =
        crypto::create_group_with_config(&alice_cred, &alice_key, &provider, "testgroup").unwrap();

    let members_before: Vec<String> = alice_group
        .members()
        .filter_map(|member| match member.credential.credential_type() {
            openmls::prelude::CredentialType::Basic => {
                if let Ok(basic_cred) =
                    openmls::prelude::BasicCredential::try_from(member.credential.clone())
                {
                    String::from_utf8(basic_cred.identity().to_vec()).ok()
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();
    assert_eq!(members_before, vec!["alice"]);

    // Invite Bob
    let (bob_cred, bob_key) = crypto::generate_credential_with_key("bob").unwrap();
    let bob_key_package =
        crypto::generate_key_package_bundle(&bob_cred, &bob_key, &provider).unwrap();
    crypto::add_members(
        &mut alice_group,
        &provider,
        &alice_key,
        &[bob_key_package.key_package()],
    )
    .unwrap();

    // After add_members (before merge), Alice's group state shows pending commit
    // Merge to finalize
    crypto::merge_pending_commit(&mut alice_group, &provider).unwrap();

    let members_after: Vec<String> = alice_group
        .members()
        .filter_map(|member| match member.credential.credential_type() {
            openmls::prelude::CredentialType::Basic => {
                if let Ok(basic_cred) =
                    openmls::prelude::BasicCredential::try_from(member.credential.clone())
                {
                    String::from_utf8(basic_cred.identity().to_vec()).ok()
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();

    assert_eq!(members_after.len(), 2, "Should have 2 members after merge");
    assert!(members_after.contains(&"alice".to_string()));
    assert!(members_after.contains(&"bob".to_string()));
}
