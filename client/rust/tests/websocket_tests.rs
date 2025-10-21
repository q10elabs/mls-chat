/// Integration tests for WebSocket message handling
///
/// Tests cover real WebSocket connectivity with the server including
/// subscriptions, message sending/receiving, and persistence.

use mls_chat_client::api::ServerApi;
use mls_chat_client::websocket::MessageHandler;
use mls_chat_client::crypto;
use tls_codec::Serialize;
use std::time::Duration;

/// Helper function to generate a valid KeyPackage for testing
fn generate_test_key_package(username: &str) -> Vec<u8> {
    // Create a temporary provider for key package generation
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("test.db");
    let provider = mls_chat_client::provider::MlsProvider::new(&db_path)
        .expect("Failed to create provider");

    // Generate credential and signature key
    let (credential, sig_key) = crypto::generate_credential_with_key(username)
        .expect("Failed to generate credential");

    // Generate key package bundle
    let key_package_bundle = crypto::generate_key_package_bundle(&credential, &sig_key, &provider)
        .expect("Failed to generate key package");

    // Serialize using TLS codec
    key_package_bundle
        .key_package()
        .tls_serialize_detached()
        .expect("Failed to serialize key package")
}

#[tokio::test]
async fn test_websocket_connect() {
    // Spawn a test HTTP server and run it in background
    let (server, addr) = mls_chat_server::server::create_test_http_server()
        .expect("Failed to create test server");
    tokio::spawn(server);

    // Give server a moment to bind
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Register a user via HTTP
    let api = ServerApi::new(&format!("http://{}", addr));
    let alice_key_package = generate_test_key_package("alice");
    api.register_user("alice", &alice_key_package)
        .await
        .expect("User registration should succeed");

    // Connect to WebSocket as alice
    let _handler = MessageHandler::connect(&addr, "alice")
        .await
        .expect("Should connect to WebSocket");
}

#[tokio::test]
async fn test_subscribe_to_group() {
    // Spawn a test HTTP server and run it in background
    let (server, addr) = mls_chat_server::server::create_test_http_server()
        .expect("Failed to create test server");
    tokio::spawn(server);

    // Give server a moment to bind
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Register a user via HTTP
    let api = ServerApi::new(&format!("http://{}", addr));
    let alice_key_package = generate_test_key_package("alice");
    api.register_user("alice", &alice_key_package)
        .await
        .expect("User registration should succeed");

    // Connect to WebSocket and subscribe to group
    let handler = MessageHandler::connect(&addr, "alice")
        .await
        .expect("Should connect to WebSocket");

    handler
        .subscribe_to_group("testgroup")
        .await
        .expect("Should subscribe to group");
}

#[tokio::test]
async fn test_send_message_via_websocket() {
    // Spawn a test HTTP server and run it in background
    let (server, addr) = mls_chat_server::server::create_test_http_server()
        .expect("Failed to create test server");
    tokio::spawn(server);

    // Give server a moment to bind
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Register a user via HTTP
    let api = ServerApi::new(&format!("http://{}", addr));
    let alice_key_package = generate_test_key_package("alice");
    api.register_user("alice", &alice_key_package)
        .await
        .expect("User registration should succeed");

    // Connect to WebSocket, subscribe, and send message
    let handler = MessageHandler::connect(&addr, "alice")
        .await
        .expect("Should connect to WebSocket");

    handler
        .subscribe_to_group("testgroup")
        .await
        .expect("Should subscribe to group");

    handler
        .send_message("testgroup", "encrypted_message_content")
        .await
        .expect("Should send message");
}

#[tokio::test]
async fn test_two_clients_exchange_messages() {
    // Spawn a test HTTP server and run it in background
    let (server, addr) = mls_chat_server::server::create_test_http_server()
        .expect("Failed to create test server");
    tokio::spawn(server);

    // Give server a moment to bind
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Register two users via HTTP
    let api = ServerApi::new(&format!("http://{}", addr));
    let alice_key_package = generate_test_key_package("alice");
    let bob_key_package = generate_test_key_package("bob");
    api.register_user("alice", &alice_key_package)
        .await
        .expect("Alice registration should succeed");
    api.register_user("bob", &bob_key_package)
        .await
        .expect("Bob registration should succeed");

    // Alice connects and subscribes to testgroup
    let mut alice_handler = MessageHandler::connect(&addr, "alice")
        .await
        .expect("Alice should connect to WebSocket");

    alice_handler
        .subscribe_to_group("testgroup")
        .await
        .expect("Alice should subscribe to testgroup");

    // Bob connects and subscribes to testgroup
    let bob_handler = MessageHandler::connect(&addr, "bob")
        .await
        .expect("Bob should connect to WebSocket");

    bob_handler
        .subscribe_to_group("testgroup")
        .await
        .expect("Bob should subscribe to testgroup");

    // Bob sends a message
    bob_handler
        .send_message("testgroup", "hello_from_bob")
        .await
        .expect("Bob should send message");

    // Give the message a moment to be delivered
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Alice receives Bob's message
    let received = alice_handler
        .next_message()
        .await
        .expect("Should receive message");

    assert!(
        received.is_some(),
        "Alice should receive message from Bob"
    );

    let msg = received.unwrap();
    assert_eq!(msg.sender, "bob");
    assert_eq!(msg.group_id, "testgroup");
    assert_eq!(msg.encrypted_content, "hello_from_bob");
}

#[tokio::test]
async fn test_multiple_groups_isolation() {
    // Spawn a test HTTP server and run it in background
    let (server, addr) = mls_chat_server::server::create_test_http_server()
        .expect("Failed to create test server");
    tokio::spawn(server);

    // Give server a moment to bind
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Register user via HTTP
    let api = ServerApi::new(&format!("http://{}", addr));
    let alice_key_package = generate_test_key_package("alice");
    api.register_user("alice", &alice_key_package)
        .await
        .expect("User registration should succeed");

    // Alice connects and subscribes to two groups
    let mut handler = MessageHandler::connect(&addr, "alice")
        .await
        .expect("Should connect to WebSocket");

    handler
        .subscribe_to_group("group1")
        .await
        .expect("Should subscribe to group1");

    handler
        .subscribe_to_group("group2")
        .await
        .expect("Should subscribe to group2");

    // Send message to group1
    handler
        .send_message("group1", "message_for_group1")
        .await
        .expect("Should send to group1");

    // Give the message a moment to be delivered
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Receive message (should be the one we sent to group1)
    let received = handler
        .next_message()
        .await
        .expect("Should receive message");

    assert!(received.is_some(), "Should receive message");
    let msg = received.unwrap();
    assert_eq!(msg.group_id, "group1");
    assert_eq!(msg.encrypted_content, "message_for_group1");
}

#[tokio::test]
async fn test_message_persistence() {
    // Create a custom pool that persists across connections
    let pool = actix_web::web::Data::new(mls_chat_server::db::create_test_pool());

    // Spawn a test HTTP server with the custom pool
    let (server, addr) = mls_chat_server::server::create_test_http_server_with_pool(pool.clone())
        .expect("Failed to create test server");
    tokio::spawn(server);

    // Give server a moment to bind
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Register user via HTTP
    let api = ServerApi::new(&format!("http://{}", addr));
    let alice_key_package = generate_test_key_package("alice");
    api.register_user("alice", &alice_key_package)
        .await
        .expect("User registration should succeed");

    // Connect to WebSocket, subscribe, and send message
    let handler = MessageHandler::connect(&addr, "alice")
        .await
        .expect("Should connect to WebSocket");

    handler
        .subscribe_to_group("persistent_group")
        .await
        .expect("Should subscribe to group");

    handler
        .send_message("persistent_group", "message_to_persist")
        .await
        .expect("Should send message");

    // Give the message a moment to be persisted
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify message was persisted by querying database directly
    let user = mls_chat_server::db::Database::get_user(&pool, "alice")
        .await
        .expect("Should query user")
        .expect("User should exist");

    let group = mls_chat_server::db::Database::get_group(&pool, "persistent_group")
        .await
        .expect("Should query group")
        .expect("Group should exist");

    let messages = mls_chat_server::db::Database::get_group_messages(&pool, group.id, 100)
        .await
        .expect("Should query messages");

    assert_eq!(messages.len(), 1, "Should have exactly 1 message persisted");
    assert_eq!(
        messages[0].encrypted_content, "message_to_persist",
        "Message content should match"
    );
    assert_eq!(messages[0].sender_id, user.id, "Sender ID should match");
}
