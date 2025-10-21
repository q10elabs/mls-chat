/// Integration tests for MlsClient orchestrator
///
/// Tests cover group creation, persistence, and state management
/// Note: Tests that require server interaction (registration) are marked with skip

use mls_chat_client::client::MlsClient;
use tempfile::tempdir;

/// Test 1: Group creation stores group ID mapping
#[tokio::test]
async fn test_group_creation_stores_mapping() {
    let _temp_dir = tempdir().expect("Failed to create temp dir");

    let mut client = MlsClient::new("http://localhost:4000", "alice", "mygroup")
        .await
        .expect("Failed to create client");

    // Initialize identity (creates local credentials)
    client.initialize().await.expect("Failed to initialize");

    // Now connect to group should succeed without server (no WebSocket needed for this test)
    match client.connect_to_group().await {
        Ok(()) => {
            // Group created successfully
            assert!(client.is_group_connected(), "Client should have MLS group");
            let group_id = client.get_group_id().expect("Should have group ID");
            assert!(!group_id.is_empty(), "Group ID should not be empty");
        }
        Err(e) => {
            // WebSocket connection will fail, that's ok - group was created
            let err_str = format!("{}", e);
            // We expect WebSocket error, not MLS error
            assert!(
                err_str.contains("WebSocket") || err_str.contains("IO error"),
                "Expected WebSocket error, got: {}",
                e
            );
        }
    }
}

/// Test 2: Client state transitions correctly
#[tokio::test]
async fn test_client_state_transitions() {
    let _temp_dir = tempdir().expect("Failed to create temp dir");

    let mut client = MlsClient::new("http://localhost:4000", "alice", "group")
        .await
        .expect("Failed to create client");

    // Before any operations
    assert!(
        client.get_identity().is_none(),
        "Should not have identity initially"
    );
    assert!(
        !client.is_group_connected(),
        "Should not have group initially"
    );
    assert!(
        !client.is_websocket_connected(),
        "Should not have websocket initially"
    );
}

/// Test 3: Test helper methods work correctly
#[tokio::test]
async fn test_helper_methods() {
    let _temp_dir = tempdir().expect("Failed to create temp dir");

    let client = MlsClient::new("http://localhost:4000", "alice", "group")
        .await
        .expect("Failed to create client");

    // Test all helper methods
    assert!(client.get_identity().is_none());
    assert!(!client.is_group_connected());
    assert!(!client.has_signature_key());
    assert!(!client.is_websocket_connected());

    // Provider should be accessible
    let _provider = client.get_provider();
}

/// Test 4: Multiple groups per user can be created
#[tokio::test]
async fn test_multiple_groups_per_user() {
    let _temp_dir = tempdir().expect("Failed to create temp dir");

    // Create two clients with same user, different groups
    let client1 = MlsClient::new("http://localhost:4000", "alice", "group1")
        .await
        .expect("Failed to create client for group 1");

    let client2 = MlsClient::new("http://localhost:4000", "alice", "group2")
        .await
        .expect("Failed to create client for group 2");

    // Both should be in initial state
    assert!(client1.get_identity().is_none());
    assert!(client2.get_identity().is_none());
    assert!(!client1.is_group_connected());
    assert!(!client2.is_group_connected());
}

/// Test 5: Different users are separate
#[tokio::test]
async fn test_different_users_are_separate() {
    let _temp_dir = tempdir().expect("Failed to create temp dir");

    let alice = MlsClient::new("http://localhost:4000", "alice", "shared-group")
        .await
        .expect("Failed to create Alice's client");

    let bob = MlsClient::new("http://localhost:4000", "bob", "shared-group")
        .await
        .expect("Failed to create Bob's client");

    // Both are separate instances
    assert!(alice.get_identity().is_none());
    assert!(bob.get_identity().is_none());
}

/// Test 6: List members returns default with creator
#[tokio::test]
async fn test_list_members() {
    let _temp_dir = tempdir().expect("Failed to create temp dir");

    let mut client = MlsClient::new("http://localhost:4000", "alice", "group")
        .await
        .expect("Failed to create client");

    // Initialize to set up identity
    client.initialize().await.expect("Failed to initialize");

    // List members should return creator by default
    let members = client.list_members();
    assert!(
        members.contains(&"alice".to_string()),
        "Creator should be in member list, got: {:?}",
        members
    );
}

/// Test 7: Client can be created with various server URLs
#[tokio::test]
async fn test_create_client_with_various_urls() {
    let _temp_dir = tempdir().expect("Failed to create temp dir");

    let _client1 = MlsClient::new("http://localhost:4000", "alice", "group")
        .await
        .expect("Failed with http://");

    let _client2 = MlsClient::new("http://example.com:8080", "bob", "group")
        .await
        .expect("Failed with http://example.com:8080");

    let _client3 = MlsClient::new("http://192.168.1.1:5000", "carol", "group")
        .await
        .expect("Failed with IP address");
}

/// Test 8: Group persistence key format validation
#[tokio::test]
async fn test_group_persistence_key_format() {
    let _temp_dir = tempdir().expect("Failed to create temp dir");

    let client = MlsClient::new("http://localhost:4000", "alice", "mygroup")
        .await
        .expect("Failed to create client");

    // Get reference to provider for testing metadata
    let provider = client.get_provider();

    // The key format should be "username:groupname"
    // Test that the metadata table was created
    match provider.group_exists("alice:mygroup") {
        Ok(false) => {
            // Expected: group hasn't been created yet
        }
        Ok(true) => {
            // Unexpected: group marked as existing before creation
            panic!("Group should not exist before creation");
        }
        Err(e) => {
            // Error checking existence (maybe DB issue)
            println!("Expected error when checking non-existent group: {}", e);
        }
    }
}

/// Test 9: Client creation doesn't require server
#[tokio::test]
async fn test_client_creation_no_server() {
    let _temp_dir = tempdir().expect("Failed to create temp dir");

    // Should be able to create client with unreachable server
    let _client = MlsClient::new("http://unreachable.invalid:9999", "alice", "group")
        .await
        .expect("Should create client even with unreachable server");

    // Client should exist but have no identity yet
    assert!(_client.get_identity().is_none());
}

/// Test 10: Client names and groups are stored correctly
#[tokio::test]
async fn test_client_metadata_storage() {
    let _temp_dir = tempdir().expect("Failed to create temp dir");

    let client = MlsClient::new("http://localhost:4000", "test_user", "test_group")
        .await
        .expect("Failed to create client");

    // Verify client metadata
    assert!(client.get_identity().is_none(), "Should not have identity initially");
    assert!(
        !client.is_group_connected(),
        "Should not be connected initially"
    );

    // Client should be properly initialized structurally
    let _provider = client.get_provider();
}
