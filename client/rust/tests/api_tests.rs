/// Integration tests for the Server API client
///
/// Tests cover user registration, key retrieval, and health checks
/// using actual HTTP server endpoints via the ServerApi client.

use mls_chat_client::api::ServerApi;

#[tokio::test]
async fn test_register_new_user() {
    // Spawn a test HTTP server and run it in background
    let (server, addr) = mls_chat_server::server::create_test_http_server()
        .expect("Failed to create test server");
    tokio::spawn(server);

    // Give server a moment to bind
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Create API client pointing to the test server
    let api = ServerApi::new(&format!("http://{}", addr));

    // Register a new user via HTTP
    let result = api.register_user("alice", "alice_public_key_123").await;
    assert!(result.is_ok(), "User registration should succeed");

    // Verify by retrieving the user's key
    let key = api
        .get_user_key("alice")
        .await
        .expect("Should retrieve user key");
    assert_eq!(key, "alice_public_key_123");
}

#[tokio::test]
async fn test_register_duplicate_user() {
    // Spawn a test HTTP server and run it in background
    let (server, addr) = mls_chat_server::server::create_test_http_server()
        .expect("Failed to create test server");
    tokio::spawn(server);

    // Give server a moment to bind
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Create API client pointing to the test server
    let api = ServerApi::new(&format!("http://{}", addr));

    // Register first user
    let result1 = api.register_user("bob", "bob_public_key_123").await;
    assert!(result1.is_ok(), "First registration should succeed");

    // Register duplicate user - should fail with HTTP 409 Conflict
    let result2 = api.register_user("bob", "bob_public_key_456").await;
    assert!(
        result2.is_err(),
        "Duplicate registration should fail with conflict error"
    );
}

#[tokio::test]
async fn test_get_user_key() {
    // Spawn a test HTTP server and run it in background
    let (server, addr) = mls_chat_server::server::create_test_http_server()
        .expect("Failed to create test server");
    tokio::spawn(server);

    // Give server a moment to bind
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Create API client pointing to the test server
    let api = ServerApi::new(&format!("http://{}", addr));

    let public_key = "carol_public_key_xyz";

    // Register a user via HTTP
    api.register_user("carol", public_key)
        .await
        .expect("Registration should succeed");

    // Fetch the user's key via HTTP
    let key = api
        .get_user_key("carol")
        .await
        .expect("Should retrieve user key");
    assert_eq!(key, public_key);
}

#[tokio::test]
async fn test_get_nonexistent_user() {
    // Spawn a test HTTP server and run it in background
    let (server, addr) = mls_chat_server::server::create_test_http_server()
        .expect("Failed to create test server");
    tokio::spawn(server);

    // Give server a moment to bind
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Create API client pointing to the test server
    let api = ServerApi::new(&format!("http://{}", addr));

    // Try to fetch a non-existent user - should fail with HTTP 404
    let result = api.get_user_key("nonexistent_user_12345").await;
    assert!(result.is_err(), "Should fail when user not found");
}

#[tokio::test]
async fn test_multiple_users() {
    // Spawn a test HTTP server and run it in background
    let (server, addr) = mls_chat_server::server::create_test_http_server()
        .expect("Failed to create test server");
    tokio::spawn(server);

    // Give server a moment to bind
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Create API client pointing to the test server
    let api = ServerApi::new(&format!("http://{}", addr));

    // Register multiple users via HTTP
    let alice_result = api.register_user("alice", "alice_key").await;
    let bob_result = api.register_user("bob", "bob_key").await;
    let carol_result = api.register_user("carol", "carol_key").await;

    assert!(alice_result.is_ok());
    assert!(bob_result.is_ok());
    assert!(carol_result.is_ok());

    // Verify each can be retrieved independently via HTTP
    let alice_key = api
        .get_user_key("alice")
        .await
        .expect("Should retrieve alice's key");
    let bob_key = api
        .get_user_key("bob")
        .await
        .expect("Should retrieve bob's key");
    let carol_key = api
        .get_user_key("carol")
        .await
        .expect("Should retrieve carol's key");

    assert_eq!(alice_key, "alice_key");
    assert_eq!(bob_key, "bob_key");
    assert_eq!(carol_key, "carol_key");
}

#[tokio::test]
async fn test_health_check() {
    // Spawn a test HTTP server and run it in background
    let (server, addr) = mls_chat_server::server::create_test_http_server()
        .expect("Failed to create test server");
    tokio::spawn(server);

    // Give server a moment to bind
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Create API client pointing to the test server
    let api = ServerApi::new(&format!("http://{}", addr));

    // Verify health check endpoint
    let result = api.health_check().await;
    assert!(result.is_ok(), "Health check should succeed");
}
