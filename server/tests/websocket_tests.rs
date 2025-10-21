/// WebSocket integration tests
/// Tests WebSocket connections, message broadcasting, and group subscriptions

use mls_chat_server::db::Database;
use mls_chat_server::handlers::WsServer;
use std::sync::Arc;
use actix_web::web;

#[tokio::test]
async fn test_websocket_client_lifecycle() {
    let pool = Arc::new(web::Data::new(mls_chat_server::db::create_test_pool()));
    let server = WsServer::new(pool);

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    // Register client
    server.register("client1".to_string(), tx).await;

    let clients = server.clients.read().await;
    assert!(clients.contains_key("client1"));
    drop(clients);

    // Unregister client
    server.unregister("client1").await;

    let clients = server.clients.read().await;
    assert!(!clients.contains_key("client1"));
}

#[tokio::test]
async fn test_websocket_group_subscription() {
    let pool = Arc::new(web::Data::new(mls_chat_server::db::create_test_pool()));
    let server = WsServer::new(pool);

    // Register clients
    let (tx1, _rx1) = tokio::sync::mpsc::unbounded_channel();
    let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel();

    server.register("client1".to_string(), tx1).await;
    server.register("client2".to_string(), tx2).await;

    // Subscribe to group
    server
        .subscribe("client1".to_string(), "group1".to_string())
        .await;
    server
        .subscribe("client2".to_string(), "group1".to_string())
        .await;

    let groups = server.groups.read().await;
    assert_eq!(groups.get("group1").unwrap().len(), 2);
    assert!(groups.get("group1").unwrap().contains("client1"));
    assert!(groups.get("group1").unwrap().contains("client2"));
}

#[tokio::test]
async fn test_websocket_message_broadcast() {
    let pool = Arc::new(web::Data::new(mls_chat_server::db::create_test_pool()));
    let server = WsServer::new(pool);

    let (tx1, mut rx1) = tokio::sync::mpsc::unbounded_channel();
    let (tx2, mut rx2) = tokio::sync::mpsc::unbounded_channel();
    let (tx3, mut rx3) = tokio::sync::mpsc::unbounded_channel();

    // Register clients
    server.register("client1".to_string(), tx1).await;
    server.register("client2".to_string(), tx2).await;
    server.register("client3".to_string(), tx3).await;

    // Subscribe clients 1 and 2 to group1, client 3 to group2
    server
        .subscribe("client1".to_string(), "group1".to_string())
        .await;
    server
        .subscribe("client2".to_string(), "group1".to_string())
        .await;
    server
        .subscribe("client3".to_string(), "group2".to_string())
        .await;

    // Broadcast to group1
    server.broadcast_to_group("group1", "hello group1").await;

    // Only clients in group1 should receive the message
    assert_eq!(rx1.recv().await, Some("hello group1".to_string()));
    assert_eq!(rx2.recv().await, Some("hello group1".to_string()));

    // Client 3 should not receive anything
    let timeout_result = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        rx3.recv(),
    ).await;
    assert!(timeout_result.is_err()); // Timeout indicates no message received
}

#[tokio::test]
async fn test_websocket_unsubscribe() {
    let pool = Arc::new(web::Data::new(mls_chat_server::db::create_test_pool()));
    let server = WsServer::new(pool);

    let (tx1, mut rx1) = tokio::sync::mpsc::unbounded_channel();

    server.register("client1".to_string(), tx1).await;
    server
        .subscribe("client1".to_string(), "group1".to_string())
        .await;

    // Unsubscribe
    server.unsubscribe("client1", "group1").await;

    // Broadcast should not reach client1
    server.broadcast_to_group("group1", "message").await;

    let timeout_result = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        rx1.recv(),
    ).await;
    assert!(timeout_result.is_err());
}

#[tokio::test]
async fn test_websocket_multiple_groups() {
    let pool = Arc::new(web::Data::new(mls_chat_server::db::create_test_pool()));
    let server = WsServer::new(pool);

    let (tx1, mut rx1) = tokio::sync::mpsc::unbounded_channel();

    server.register("client1".to_string(), tx1).await;

    // Subscribe to multiple groups
    server
        .subscribe("client1".to_string(), "group1".to_string())
        .await;
    server
        .subscribe("client1".to_string(), "group2".to_string())
        .await;

    // Broadcast to group1
    server.broadcast_to_group("group1", "msg1").await;
    assert_eq!(rx1.recv().await, Some("msg1".to_string()));

    // Broadcast to group2
    server.broadcast_to_group("group2", "msg2").await;
    assert_eq!(rx1.recv().await, Some("msg2".to_string()));
}

#[tokio::test]
async fn test_websocket_persist_message() {
    let pool = Arc::new(web::Data::new(mls_chat_server::db::create_test_pool()));
    let server = WsServer::new(pool.clone());

    let alice_key = vec![0x2d, 0x2e, 0x2f, 0x30];

    // Register user
    Database::register_user(pool.as_ref(), "alice", &alice_key)
        .await
        .expect("Failed to register");

    // Persist a message
    let persisted = server
        .persist_message("group1", "alice", "encrypted_content")
        .await;

    assert!(persisted);

    // Verify message was stored
    let groups = Database::get_group(pool.as_ref(), "group1")
        .await
        .expect("Failed to get group");
    assert!(groups.is_some());

    let messages = Database::get_group_messages(pool.as_ref(), groups.unwrap().id, 10)
        .await
        .expect("Failed to get messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].encrypted_content, "encrypted_content");
}

#[tokio::test]
async fn test_websocket_persist_nonexistent_user() {
    let pool = Arc::new(web::Data::new(mls_chat_server::db::create_test_pool()));
    let server = WsServer::new(pool);

    // Try to persist message from non-existent user
    let persisted = server
        .persist_message("group1", "nonexistent", "content")
        .await;

    assert!(!persisted);
}

#[tokio::test]
async fn test_websocket_multiple_clients_same_group() {
    let pool = Arc::new(web::Data::new(mls_chat_server::db::create_test_pool()));
    let server = WsServer::new(pool.clone());

    let alice_key = vec![0x31, 0x32, 0x33, 0x34];
    let bob_key = vec![0x35, 0x36, 0x37, 0x38];

    // Register users
    Database::register_user(pool.as_ref(), "alice", &alice_key)
        .await
        .expect("Failed to register");
    Database::register_user(pool.as_ref(), "bob", &bob_key)
        .await
        .expect("Failed to register");

    // Register clients
    let (tx1, _rx1) = tokio::sync::mpsc::unbounded_channel();
    let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel();

    server.register("alice_client".to_string(), tx1).await;
    server.register("bob_client".to_string(), tx2).await;

    // Subscribe to same group
    server
        .subscribe("alice_client".to_string(), "team".to_string())
        .await;
    server
        .subscribe("bob_client".to_string(), "team".to_string())
        .await;

    // Alice persists a message
    let persisted = server
        .persist_message("team", "alice", "alice_msg")
        .await;
    assert!(persisted);

    // Bob persists a message
    let persisted = server
        .persist_message("team", "bob", "bob_msg")
        .await;
    assert!(persisted);

    // Verify both messages were stored
    let group = Database::get_group(pool.as_ref(), "team")
        .await
        .expect("Failed to get group");
    let messages = Database::get_group_messages(pool.as_ref(), group.unwrap().id, 10)
        .await
        .expect("Failed to get messages");
    assert_eq!(messages.len(), 2);
}

#[tokio::test]
async fn test_websocket_client_cleanup_on_disconnect() {
    let pool = Arc::new(web::Data::new(mls_chat_server::db::create_test_pool()));
    let server = WsServer::new(pool);

    let (tx1, _rx1) = tokio::sync::mpsc::unbounded_channel();
    let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel();

    // Register and subscribe clients
    server.register("client1".to_string(), tx1).await;
    server.register("client2".to_string(), tx2).await;

    server
        .subscribe("client1".to_string(), "group1".to_string())
        .await;
    server
        .subscribe("client2".to_string(), "group1".to_string())
        .await;

    // Verify initial state
    let groups = server.groups.read().await;
    assert_eq!(groups.get("group1").unwrap().len(), 2);
    drop(groups);

    // Unregister client1
    server.unregister("client1").await;

    // Verify client1 removed from all groups
    let groups = server.groups.read().await;
    let group1_members = groups.get("group1").unwrap();
    assert!(!group1_members.contains("client1"));
    assert!(group1_members.contains("client2"));
}
