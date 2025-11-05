/// Integration tests for WebSocket message handling
///
/// Tests cover real WebSocket connectivity with the server including
/// subscriptions, message sending/receiving, and persistence.
use mls_chat_client::api::ServerApi;
use mls_chat_client::crypto;
use mls_chat_client::models::MlsMessageEnvelope;
use mls_chat_client::websocket::MessageHandler;
use std::time::Duration;
use tls_codec::Serialize;

/// Helper function to generate a valid KeyPackage for testing
fn generate_test_key_package(username: &str) -> Vec<u8> {
    // Create a temporary provider for key package generation
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("test.db");
    let provider =
        mls_chat_client::provider::MlsProvider::new(&db_path).expect("Failed to create provider");

    // Generate credential and signature key
    let (credential, sig_key) =
        crypto::generate_credential_with_key(username).expect("Failed to generate credential");

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
    let (server, addr) =
        mls_chat_server::server::create_test_http_server().expect("Failed to create test server");
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
    let (server, addr) =
        mls_chat_server::server::create_test_http_server().expect("Failed to create test server");
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
    let (server, addr) =
        mls_chat_server::server::create_test_http_server().expect("Failed to create test server");
    tokio::spawn(server);

    // Give server a moment to bind
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Register a user via HTTP
    let api = ServerApi::new(&format!("http://{}", addr));
    let alice_key_package = generate_test_key_package("alice");
    api.register_user("alice", &alice_key_package)
        .await
        .expect("User registration should succeed");

    // Connect to WebSocket, subscribe, and send envelope
    let handler = MessageHandler::connect(&addr, "alice")
        .await
        .expect("Should connect to WebSocket");

    handler
        .subscribe_to_group("testgroup")
        .await
        .expect("Should subscribe to group");

    // Send an application message envelope
    let envelope = MlsMessageEnvelope::ApplicationMessage {
        sender: "alice".to_string(),
        group_id: "testgroup_base64_encoded_id".to_string(),
        encrypted_content: "encrypted_message_content".to_string(),
    };

    handler
        .send_envelope(&envelope)
        .await
        .expect("Should send envelope");
}

#[tokio::test]
async fn test_two_clients_exchange_messages() {
    // Create persistent pool for message verification
    let pool = actix_web::web::Data::new(mls_chat_server::db::create_test_pool());
    let (server, addr) = mls_chat_server::server::create_test_http_server_with_pool(pool.clone())
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

    // Bob sends a message via envelope
    let bob_envelope = MlsMessageEnvelope::ApplicationMessage {
        sender: "bob".to_string(),
        group_id: "testgroup".to_string(),
        encrypted_content: "hello_from_bob".to_string(),
    };

    bob_handler
        .send_envelope(&bob_envelope)
        .await
        .expect("Bob should send envelope");

    // Give the message a moment to be routed and persisted
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify message was routed to Alice with a timeout
    let received = tokio::time::timeout(Duration::from_secs(2), alice_handler.next_envelope())
        .await
        .expect("Alice should receive message within 2 seconds")
        .expect("Alice should receive Bob's envelope");

    assert!(received.is_some(), "Alice should receive envelope from Bob");

    let envelope = received.unwrap();
    match envelope {
        MlsMessageEnvelope::ApplicationMessage {
            sender,
            group_id,
            encrypted_content,
        } => {
            assert_eq!(sender, "bob", "Sender should be bob");
            assert_eq!(group_id, "testgroup", "Group should be testgroup");
            assert_eq!(encrypted_content, "hello_from_bob", "Content should match");
        }
        _ => panic!("Expected ApplicationMessage envelope"),
    }

    // Verify message was persisted to database
    let bob_user = mls_chat_server::db::Database::get_user(&pool, "bob")
        .await
        .expect("Should query user")
        .expect("Bob user should exist");

    let group = mls_chat_server::db::Database::get_group(&pool, "testgroup")
        .await
        .expect("Should query group")
        .expect("testgroup should exist");

    let messages = mls_chat_server::db::Database::get_group_messages(&pool, group.id, 100)
        .await
        .expect("Should query messages");

    assert_eq!(messages.len(), 1, "Should have exactly 1 message persisted");
    assert_eq!(
        messages[0].encrypted_content, "hello_from_bob",
        "Persisted message content should match"
    );
    assert_eq!(
        messages[0].sender_id, bob_user.id,
        "Sender ID should match Bob"
    );
}

#[tokio::test]
async fn test_multiple_groups_isolation() {
    // Create persistent pool for verification
    let pool = actix_web::web::Data::new(mls_chat_server::db::create_test_pool());
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

    // Alice connects and subscribes to two groups
    let handler = MessageHandler::connect(&addr, "alice")
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

    // Send messages to both groups
    let group1_envelope = MlsMessageEnvelope::ApplicationMessage {
        sender: "alice".to_string(),
        group_id: "group1".to_string(),
        encrypted_content: "message_for_group1".to_string(),
    };

    let group2_envelope = MlsMessageEnvelope::ApplicationMessage {
        sender: "alice".to_string(),
        group_id: "group2".to_string(),
        encrypted_content: "message_for_group2".to_string(),
    };

    handler
        .send_envelope(&group1_envelope)
        .await
        .expect("Should send envelope to group1");

    handler
        .send_envelope(&group2_envelope)
        .await
        .expect("Should send envelope to group2");

    // Give messages a moment to be persisted
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify both messages were persisted to separate groups
    let alice_user = mls_chat_server::db::Database::get_user(&pool, "alice")
        .await
        .expect("Should query user")
        .expect("Alice user should exist");

    let group1 = mls_chat_server::db::Database::get_group(&pool, "group1")
        .await
        .expect("Should query group1")
        .expect("group1 should exist");

    let group2 = mls_chat_server::db::Database::get_group(&pool, "group2")
        .await
        .expect("Should query group2")
        .expect("group2 should exist");

    // Verify group1 has its message
    let group1_messages = mls_chat_server::db::Database::get_group_messages(&pool, group1.id, 100)
        .await
        .expect("Should query group1 messages");

    assert_eq!(group1_messages.len(), 1, "group1 should have 1 message");
    assert_eq!(
        group1_messages[0].encrypted_content, "message_for_group1",
        "group1 message content should match"
    );
    assert_eq!(
        group1_messages[0].sender_id, alice_user.id,
        "Sender should be alice"
    );

    // Verify group2 has its message
    let group2_messages = mls_chat_server::db::Database::get_group_messages(&pool, group2.id, 100)
        .await
        .expect("Should query group2 messages");

    assert_eq!(group2_messages.len(), 1, "group2 should have 1 message");
    assert_eq!(
        group2_messages[0].encrypted_content, "message_for_group2",
        "group2 message content should match"
    );
    assert_eq!(
        group2_messages[0].sender_id, alice_user.id,
        "Sender should be alice"
    );

    // Verify messages are isolated - no cross-group pollution
    assert_ne!(
        group1_messages[0].encrypted_content, group2_messages[0].encrypted_content,
        "Messages should be different between groups"
    );
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

    // Connect to WebSocket, subscribe, and send envelope
    let handler = MessageHandler::connect(&addr, "alice")
        .await
        .expect("Should connect to WebSocket");

    handler
        .subscribe_to_group("persistent_group")
        .await
        .expect("Should subscribe to group");

    // Send envelope with application message
    // Note: The server stores messages sent via WebSocket in the database
    let envelope = MlsMessageEnvelope::ApplicationMessage {
        sender: "alice".to_string(),
        group_id: "persistent_group".to_string(), // Must match the group we subscribed to
        encrypted_content: "message_to_persist".to_string(),
    };

    handler
        .send_envelope(&envelope)
        .await
        .expect("Should send envelope");

    // Give the message a moment to be persisted
    tokio::time::sleep(Duration::from_millis(500)).await;

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
