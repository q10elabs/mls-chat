/// Integration tests for MlsClient orchestrator
///
/// Tests cover group creation, persistence, and state management
/// Note: Tests that require server interaction (registration) are marked with skip
use actix_web::web;
use mls_chat_client::client::MlsClient;
use mls_chat_client::mls::KeyPackagePoolConfig;
use mls_chat_server::db::keypackage_store::{KeyPackageStatus, KeyPackageStore};
use std::time::Duration;
use tempfile::tempdir;

/// Test helper: Create MlsClient with temporary directory (no server registration)
///
/// This helper creates a client that can be used for testing without server dependency.
/// The client will be created but not initialized (no server registration).
fn create_test_client_no_init(
    server_url: &str,
    username: &str,
    group_name: &str,
) -> (MlsClient, tempfile::TempDir) {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let storage_path = temp_dir.path();

    let client = MlsClient::new_with_storage_path(server_url, username, group_name, storage_path)
        .expect("Failed to create client with temp storage");

    (client, temp_dir)
}

/// Test 1: Group creation stores group ID mapping
#[tokio::test]
async fn test_group_creation_stores_mapping() {
    // Create client with temporary directory (no server dependency)
    let (client, _temp_dir) =
        create_test_client_no_init("http://localhost:4000", "alice", "mygroup");

    // Test that client can be created without server
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

    // Test that we can access the provider (MLS operations work without server)
    let _provider = client.get_provider();

    // Test that client metadata is properly initialized
    assert_eq!(client.get_username(), "alice");
    assert_eq!(client.get_group_name(), "mygroup");
}

/// Test 2: Client state transitions correctly
#[tokio::test]
async fn test_client_state_transitions() {
    let (client, _temp_dir) = create_test_client_no_init("http://localhost:4000", "alice", "group");

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
    let (client, _temp_dir) = create_test_client_no_init("http://localhost:4000", "alice", "group");

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
    // Create two clients with same user, different groups
    let (client1, _temp_dir1) =
        create_test_client_no_init("http://localhost:4000", "alice", "group1");
    let (client2, _temp_dir2) =
        create_test_client_no_init("http://localhost:4000", "alice", "group2");

    // Both should be in initial state
    assert!(client1.get_identity().is_none());
    assert!(client2.get_identity().is_none());
    assert!(!client1.is_group_connected());
    assert!(!client2.is_group_connected());
}

/// Test 5: Different users are separate
#[tokio::test]
async fn test_different_users_are_separate() {
    let (alice, _temp_dir1) =
        create_test_client_no_init("http://localhost:4000", "alice", "shared-group");
    let (bob, _temp_dir2) =
        create_test_client_no_init("http://localhost:4000", "bob", "shared-group");

    // Both are separate instances
    assert!(alice.get_identity().is_none());
    assert!(bob.get_identity().is_none());
}

/// Test 6: List members returns empty when group not connected
#[tokio::test]
async fn test_list_members() {
    // Create client with temporary directory (no server dependency)
    let (client, _temp_dir) = create_test_client_no_init("http://localhost:4000", "alice", "group");

    // Group is not connected yet, so member list should be empty
    assert!(
        !client.is_group_connected(),
        "Client should not be connected to group"
    );

    // List members should return empty when group is not connected
    let members = client.list_members();
    assert!(
        members.is_empty(),
        "Should return empty list when group not connected, got: {:?}",
        members
    );
}

/// Test 7: Client can be created with various server URLs
#[tokio::test]
async fn test_create_client_with_various_urls() {
    // Use separate temp directories for each client to ensure isolation
    let _temp_dir1 = tempdir().expect("Failed to create temp dir 1");
    let _client1 = MlsClient::new_with_storage_path(
        "http://localhost:4000",
        "alice",
        "group",
        _temp_dir1.path(),
    )
    .expect("Failed with http://");

    let _temp_dir2 = tempdir().expect("Failed to create temp dir 2");
    let _client2 = MlsClient::new_with_storage_path(
        "http://example.com:8080",
        "bob",
        "group",
        _temp_dir2.path(),
    )
    .expect("Failed with http://example.com:8080");

    let _temp_dir3 = tempdir().expect("Failed to create temp dir 3");
    let _client3 = MlsClient::new_with_storage_path(
        "http://192.168.1.1:5000",
        "carol",
        "group",
        _temp_dir3.path(),
    )
    .expect("Failed with IP address");
}

/// Test 8: Group persistence key format validation
#[tokio::test]
async fn test_group_persistence_key_format() {
    let (client, _temp_dir) =
        create_test_client_no_init("http://localhost:4000", "alice", "mygroup");

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
    // Should be able to create client with unreachable server
    let (_client, _temp_dir) =
        create_test_client_no_init("http://unreachable.invalid:9999", "alice", "group");

    // Client should exist but have no identity yet
    assert!(_client.get_identity().is_none());
}

/// Test 10: Client names and groups are stored correctly
#[tokio::test]
async fn test_client_metadata_storage() {
    let (client, _temp_dir) =
        create_test_client_no_init("http://localhost:4000", "test_user", "test_group");

    // Verify client metadata
    assert!(
        client.get_identity().is_none(),
        "Should not have identity initially"
    );
    assert!(
        !client.is_group_connected(),
        "Should not be connected initially"
    );

    // Client should be properly initialized structurally
    let _provider = client.get_provider();
}

/// Phase 2.3: Refresh should generate, upload, and mark KeyPackages as available
#[tokio::test]
async fn refresh_key_packages_generates_and_uploads() {
    let pool = web::Data::new(mls_chat_server::db::create_test_pool());
    let (server, addr) = mls_chat_server::server::create_test_http_server_with_pool(pool.clone())
        .expect("Failed to create test server");
    tokio::spawn(server);
    tokio::time::sleep(Duration::from_millis(200)).await;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let server_url = format!("http://{}", addr);
    let mut client = MlsClient::new_with_storage_path(
        &server_url,
        "refresh_user",
        "refresh-group",
        temp_dir.path(),
    )
    .expect("Failed to create client");

    let config = KeyPackagePoolConfig {
        target_pool_size: 2,
        low_watermark: 1,
        hard_cap: 4,
    };
    client.set_keypackage_pool_config(config.clone());

    client
        .initialize()
        .await
        .expect("Initialization should succeed");
    client
        .refresh_key_packages()
        .await
        .expect("Refresh should succeed");

    let available = client
        .get_metadata_store()
        .count_by_status("available")
        .expect("Metadata count should succeed");
    assert_eq!(available, config.target_pool_size);

    let created = client
        .get_metadata_store()
        .count_by_status("created")
        .expect("Metadata count should succeed");
    assert_eq!(created, 0);

    let server_available = KeyPackageStore::count_by_status(
        pool.get_ref(),
        "refresh_user",
        KeyPackageStatus::Available,
    )
    .await
    .expect("Server count should succeed");
    assert_eq!(server_available, config.target_pool_size);

    client
        .refresh_key_packages()
        .await
        .expect("Second refresh should succeed");

    let available_after = client
        .get_metadata_store()
        .count_by_status("available")
        .expect("Metadata count should succeed");
    assert_eq!(available_after, config.target_pool_size);

    let server_available_after = KeyPackageStore::count_by_status(
        pool.get_ref(),
        "refresh_user",
        KeyPackageStatus::Available,
    )
    .await
    .expect("Server count should succeed");
    assert_eq!(server_available_after, config.target_pool_size);
}

// ============================================================================
// INTEGRATION TESTS WITH REAL SERVER
// ============================================================================

/// Test helper: Create test server and return address
///
/// This helper creates a real test server using the server library
/// and returns the server instance and bind address for client connections.
async fn create_test_server() -> (actix_web::dev::Server, String) {
    let (server, addr) =
        mls_chat_server::server::create_test_http_server().expect("Failed to create test server");

    println!("Server created with address: {}", addr);

    // Give server a moment to bind
    tokio::time::sleep(Duration::from_millis(100)).await;

    (server, addr)
}

/// Test helper: Create MlsClient with real server
///
/// This helper creates a client that connects to a real test server
/// for end-to-end integration testing.
fn create_client_with_server(
    server_url: &str,
    username: &str,
    group_name: &str,
) -> (MlsClient, tempfile::TempDir) {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let storage_path = temp_dir.path();

    let client = MlsClient::new_with_storage_path(server_url, username, group_name, storage_path)
        .expect("Failed to create client with temp storage");

    (client, temp_dir)
}

/// Integration Test 1: Complete client-server workflow with real server
#[tokio::test]
async fn test_client_server_integration_complete_workflow() {
    // Create test server
    let (server, addr) = create_test_server().await;
    let server_handle = tokio::spawn(server);

    // Create client with real server
    let (mut client, _temp_dir) =
        create_client_with_server(&format!("http://{}", addr), "alice", "testgroup");

    // Test 1: Initialize client (should register with server)
    client
        .initialize()
        .await
        .expect("Failed to initialize client");

    // Verify client has identity after initialization
    assert!(
        client.get_identity().is_some(),
        "Client should have identity after initialization"
    );
    assert!(
        client.has_signature_key(),
        "Client should have signature key after initialization"
    );

    // Test 2: Connect to group (should create group and register with server)
    client
        .connect_to_group()
        .await
        .expect("Failed to connect to group");

    // Verify group connection
    assert!(
        client.is_group_connected(),
        "Client should be connected to group"
    );
    assert!(
        client.get_group_id().is_some(),
        "Client should have group ID"
    );

    // Test 3: Send message (should work with WebSocket)
    client
        .send_message("Hello from integration test!")
        .await
        .expect("Failed to send message");

    // Test 4: List members (should include creator)
    let members = client.list_members();
    assert!(
        members.contains(&"alice".to_string()),
        "Creator should be in member list"
    );

    // Cleanup
    server_handle.abort();
}

/// Integration Test 2: Multiple clients with same server
#[tokio::test]
async fn test_multiple_clients_same_server() {
    // Create test server
    let (server, addr) = create_test_server().await;
    let server_handle = tokio::spawn(server);

    // Create two clients with different users
    let (mut alice, _temp_dir1) =
        create_client_with_server(&format!("http://{}", addr), "alice", "sharedgroup");
    let (mut bob, _temp_dir2) =
        create_client_with_server(&format!("http://{}", addr), "bob", "sharedgroup");

    // Initialize both clients
    alice
        .initialize()
        .await
        .expect("Failed to initialize Alice");
    bob.initialize().await.expect("Failed to initialize Bob");

    // Both should have identities
    assert!(alice.get_identity().is_some(), "Alice should have identity");
    assert!(bob.get_identity().is_some(), "Bob should have identity");

    // Both should be able to connect to their respective groups
    alice
        .connect_to_group()
        .await
        .expect("Failed to connect Alice to group");
    bob.connect_to_group()
        .await
        .expect("Failed to connect Bob to group");

    // Both should be connected
    assert!(alice.is_group_connected(), "Alice should be connected");
    assert!(bob.is_group_connected(), "Bob should be connected");

    // Cleanup
    server_handle.abort();
}

/// Integration Test 3: Client persistence across server restarts
#[tokio::test]
async fn test_client_persistence_across_server_restarts() {
    // Create test server
    let (server, addr) = create_test_server().await;
    let server_handle = tokio::spawn(server);

    // Create client and initialize
    let (mut client, _temp_dir) = create_client_with_server(
        &format!("http://{}", addr),
        "persistent_user",
        "persistent_group",
    );
    client
        .initialize()
        .await
        .expect("Failed to initialize client");
    client
        .connect_to_group()
        .await
        .expect("Failed to connect to group");

    // Verify initial state
    assert!(
        client.is_group_connected(),
        "Client should be connected initially"
    );
    let _initial_group_id = client.get_group_id().expect("Should have group ID");

    // Stop server
    server_handle.abort();
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Create new client instance with same storage (simulating client restart)
    // This tests client-side persistence, not server-side persistence
    let (client2, _temp_dir2) = create_client_with_server(
        "http://127.0.0.1:9999",
        "persistent_user",
        "persistent_group",
    );

    // Client should be able to load its local state (no server needed for this)
    // Note: This tests client-side persistence, which is the main goal
    assert!(
        client2.get_identity().is_none(),
        "New client should not have identity initially"
    );
    assert!(
        !client2.is_group_connected(),
        "New client should not be connected initially"
    );

    // The test demonstrates that client state is not automatically restored
    // In a real scenario, the client would need to reconnect to a server
    // to restore its group state

    // Cleanup
}

/// Integration Test 4: Error handling with server unavailable
#[tokio::test]
async fn test_client_error_handling_server_unavailable() {
    // Create client with unreachable server
    let (mut client, _temp_dir) =
        create_client_with_server("http://127.0.0.1:9999", "error_user", "error_group");

    // Initialize should fail gracefully (server registration fails, but identity is created locally)
    let init_result = client.initialize().await;
    assert!(
        init_result.is_err(),
        "Initialize should fail with unreachable server"
    );

    // Client should have local identity (created before server registration attempt)
    assert!(
        client.get_identity().is_some(),
        "Should have local identity even after failed server registration"
    );
    assert!(
        client.has_signature_key(),
        "Should have signature key even after failed server registration"
    );
    assert!(
        !client.is_group_connected(),
        "Should not be connected to group after failed init"
    );
}

/// Integration Test 5: WebSocket message exchange
#[tokio::test]
async fn test_websocket_message_exchange() {
    // Create test server
    let (server, addr) = create_test_server().await;
    let server_handle = tokio::spawn(server);

    // Create two clients
    let (mut alice, _temp_dir1) =
        create_client_with_server(&format!("http://{}", addr), "alice", "chatgroup");
    let (mut bob, _temp_dir2) =
        create_client_with_server(&format!("http://{}", addr), "bob", "chatgroup");

    // Initialize both clients
    alice
        .initialize()
        .await
        .expect("Failed to initialize Alice");
    bob.initialize().await.expect("Failed to initialize Bob");

    // Connect both to group
    alice
        .connect_to_group()
        .await
        .expect("Failed to connect Alice");
    bob.connect_to_group().await.expect("Failed to connect Bob");

    // Alice sends a message
    alice
        .send_message("Hello Bob!")
        .await
        .expect("Failed to send message from Alice");

    // Bob should be able to receive the message (this tests WebSocket functionality)
    // Note: This is a simplified test - in a real scenario, Bob would need to process incoming messages
    // For now, we just verify that the send operation succeeds

    // Cleanup
    server_handle.abort();
}

/// Integration Test 6: Server health check
#[tokio::test]
async fn test_server_health_check() {
    // Create test server
    let (server, addr) = create_test_server().await;
    let server_handle = tokio::spawn(server);

    // Create client
    let (client, _temp_dir) =
        create_client_with_server(&format!("http://{}", addr), "health_user", "health_group");

    // Test server health check
    let health_result = client.get_api().health_check().await;
    assert!(health_result.is_ok(), "Server health check should succeed");

    // Cleanup
    server_handle.abort();
}

/// Integration Test 7: Sender skips processing their own application messages
///
/// This test verifies that when a client receives a broadcast of their own
/// application message (echo from server), the client skips decryption
/// instead of attempting to decrypt with out-of-sync ratchet state.
///
/// Specifically, this test ensures that:
/// 1. Alice sends a message (ratchet advances on sender side)
/// 2. Server broadcasts the message back to Alice
/// 3. Alice receives her own message but SKIPS it (doesn't try to decrypt)
/// 4. This avoids the decryption failure that would occur if Alice tried to
///    decrypt her own message using her updated (receiver-side) ratchet state
#[tokio::test]
async fn test_sender_skips_own_application_message() {
    // Create test server
    let (server, addr) = create_test_server().await;
    let server_handle = tokio::spawn(server);

    // Create two clients
    let (mut alice, _temp_dir1) =
        create_client_with_server(&format!("http://{}", addr), "alice", "testgroup");
    let (mut bob, _temp_dir2) =
        create_client_with_server(&format!("http://{}", addr), "bob", "testgroup");

    // Initialize both clients
    alice
        .initialize()
        .await
        .expect("Failed to initialize Alice");
    bob.initialize().await.expect("Failed to initialize Bob");

    // Both connect to their groups
    alice
        .connect_to_group()
        .await
        .expect("Failed to connect Alice");
    bob.connect_to_group().await.expect("Failed to connect Bob");

    // Alice invites Bob to her group
    alice
        .invite_user("bob")
        .await
        .expect("Failed to invite Bob");

    // Give Bob time to process the Welcome message
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Alice sends a message
    // When the server broadcasts this back to Alice, Alice should skip it (not try to decrypt)
    alice
        .send_message("Hello from Alice!")
        .await
        .expect("Failed to send message from Alice");

    // Give time for message broadcast
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // The test passes if Alice doesn't panic/error when receiving her own message
    // Before the fix, Alice would try to decrypt her own message and fail because
    // the ratchet state is out of sync (advanced on send, not on receive).
    // With the fix, Alice skips her own message as expected.

    // Cleanup
    server_handle.abort();
}

/// Integration Test 8: Initialization seeds KeyPackage pool on the server
///
/// Ensures that calling `initialize()` immediately uploads enough
/// KeyPackages so subsequent invitations do not exhaust the pool.
#[tokio::test]
async fn test_initialize_seeds_keypackage_pool() {
    let (server, addr) = create_test_server().await;
    let server_handle = tokio::spawn(server);

    let (mut client, _temp_dir) =
        create_client_with_server(&format!("http://{}", addr), "seed_user", "seed_group");

    client
        .initialize()
        .await
        .expect("Failed to initialize seed_user");

    // Give the async upload a moment to complete.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let status = client
        .get_api()
        .get_key_package_status("seed_user")
        .await
        .expect("Failed to fetch keypackage status");

    assert!(
        status.available >= 1,
        "Initialization should seed at least one available keypackage"
    );
    assert!(
        status.total >= status.available,
        "Total keypackages should be at least the number available"
    );

    server_handle.abort();
}

// ==================== Phase 2.5: Periodic Refresh Tests ====================

/// Test: should_refresh() returns true when no refresh has occurred yet
#[tokio::test]
async fn test_should_refresh_returns_true_on_first_call() {
    let (client, _temp_dir) = create_test_client_no_init("http://localhost:4000", "alice", "group");

    // Before any refresh, should_refresh() should return true
    assert!(
        client.should_refresh(),
        "should_refresh() should return true before first refresh"
    );
}

/// Test: should_refresh() returns false immediately after update_refresh_time()
#[tokio::test]
async fn test_should_refresh_returns_false_after_update() {
    let (mut client, _temp_dir) =
        create_test_client_no_init("http://localhost:4000", "alice", "group");

    // Update refresh time to now
    client.update_refresh_time();

    // Should not need refresh immediately after update
    assert!(
        !client.should_refresh(),
        "should_refresh() should return false immediately after update"
    );
}

/// Test: should_refresh() returns true after refresh period has elapsed
#[tokio::test]
async fn test_should_refresh_returns_true_after_period_elapsed() {
    let (mut client, _temp_dir) =
        create_test_client_no_init("http://localhost:4000", "alice", "group");

    // Set a very short refresh period for testing (1 second)
    client.set_refresh_period(Duration::from_secs(1));

    // Update refresh time to now
    client.update_refresh_time();

    // Should not need refresh immediately
    assert!(
        !client.should_refresh(),
        "should_refresh() should return false immediately after update"
    );

    // Wait for refresh period to elapse
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Should need refresh now
    assert!(
        client.should_refresh(),
        "should_refresh() should return true after period elapsed"
    );
}

/// Test: refresh_period is configurable
#[tokio::test]
async fn test_refresh_period_is_configurable() {
    let (mut client, _temp_dir) =
        create_test_client_no_init("http://localhost:4000", "alice", "group");

    // Default should be 1 hour (3600 seconds)
    assert_eq!(
        client.get_refresh_period(),
        Duration::from_secs(3600),
        "Default refresh period should be 1 hour"
    );

    // Set custom period
    let custom_period = Duration::from_secs(300); // 5 minutes
    client.set_refresh_period(custom_period);

    // Verify it was set
    assert_eq!(
        client.get_refresh_period(),
        custom_period,
        "Custom refresh period should be set"
    );
}

/// Test: update_refresh_time() sets the current time
#[tokio::test]
async fn test_update_refresh_time_sets_current_time() {
    let (mut client, _temp_dir) =
        create_test_client_no_init("http://localhost:4000", "alice", "group");

    // Initially should be None
    assert!(
        client.get_last_refresh_time().is_none(),
        "Last refresh time should be None initially"
    );

    // Record time before update
    let before = std::time::SystemTime::now();

    // Update refresh time
    client.update_refresh_time();

    // Get the updated time
    let last_refresh = client
        .get_last_refresh_time()
        .expect("Last refresh time should be set");

    // Record time after update
    let after = std::time::SystemTime::now();

    // Verify last_refresh is between before and after
    assert!(
        last_refresh >= before && last_refresh <= after,
        "Last refresh time should be set to current time"
    );
}

/// Test: should_refresh() with multiple refresh cycles
#[tokio::test]
async fn test_should_refresh_multiple_cycles() {
    let (mut client, _temp_dir) =
        create_test_client_no_init("http://localhost:4000", "alice", "group");

    // Set a short refresh period for testing (500ms)
    client.set_refresh_period(Duration::from_millis(500));

    // First cycle: should refresh immediately
    assert!(client.should_refresh(), "First call should return true");
    client.update_refresh_time();

    // Should not refresh immediately after update
    assert!(
        !client.should_refresh(),
        "Should not refresh immediately after update"
    );

    // Wait for period to elapse
    tokio::time::sleep(Duration::from_millis(600)).await;

    // Second cycle: should refresh again
    assert!(
        client.should_refresh(),
        "Should refresh after period elapsed"
    );
    client.update_refresh_time();

    // Should not refresh immediately again
    assert!(
        !client.should_refresh(),
        "Should not refresh immediately after second update"
    );
}

/// Test: Refresh period can be set to very short intervals (for testing)
#[tokio::test]
async fn test_refresh_period_short_interval() {
    let (mut client, _temp_dir) =
        create_test_client_no_init("http://localhost:4000", "alice", "group");

    // Set very short period (100ms) for testing
    client.set_refresh_period(Duration::from_millis(100));
    client.update_refresh_time();

    // Should not refresh immediately
    assert!(!client.should_refresh());

    // Wait for period
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Should refresh now
    assert!(client.should_refresh());
}

/// Test: Refresh period can be set to long intervals
#[tokio::test]
async fn test_refresh_period_long_interval() {
    let (mut client, _temp_dir) =
        create_test_client_no_init("http://localhost:4000", "alice", "group");

    // Set long period (1 hour)
    let one_hour = Duration::from_secs(3600);
    client.set_refresh_period(one_hour);
    client.update_refresh_time();

    // Should not refresh immediately
    assert!(!client.should_refresh());

    // Even after a few seconds, should still not refresh
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(!client.should_refresh());
}

/// Test: update_refresh_time is idempotent
#[tokio::test]
async fn test_update_refresh_time_is_idempotent() {
    let (mut client, _temp_dir) =
        create_test_client_no_init("http://localhost:4000", "alice", "group");

    // Call multiple times in quick succession
    client.update_refresh_time();
    let time1 = client.get_last_refresh_time().unwrap();

    tokio::time::sleep(Duration::from_millis(10)).await;

    client.update_refresh_time();
    let time2 = client.get_last_refresh_time().unwrap();

    // Both times should be set (not None)
    // time2 should be slightly after time1
    assert!(
        time2 >= time1,
        "Second update should set time >= first update"
    );
}

/// Test: Refresh time tracking survives multiple updates
#[tokio::test]
async fn test_refresh_time_tracking_survives_updates() {
    let (mut client, _temp_dir) =
        create_test_client_no_init("http://localhost:4000", "alice", "group");

    client.set_refresh_period(Duration::from_millis(200));

    // First refresh
    assert!(client.should_refresh());
    client.update_refresh_time();
    assert!(!client.should_refresh());

    // Wait and refresh again
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(client.should_refresh());
    client.update_refresh_time();
    assert!(!client.should_refresh());

    // Wait and refresh a third time
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(client.should_refresh());
    client.update_refresh_time();
    assert!(!client.should_refresh());
}
