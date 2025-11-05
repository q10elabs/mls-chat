/// Integration tests for the Server API client
///
/// Tests cover user registration, key retrieval, and health checks
/// using actual HTTP server endpoints via the ServerApi client.
use mls_chat_client::api::ServerApi;
use mls_chat_client::crypto;
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
async fn test_register_new_user() {
    // Spawn a test HTTP server and run it in background
    let (server, addr) =
        mls_chat_server::server::create_test_http_server().expect("Failed to create test server");
    tokio::spawn(server);

    // Give server a moment to bind
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Create API client pointing to the test server
    let api = ServerApi::new(&format!("http://{}", addr));

    // Generate a valid KeyPackage for alice
    let alice_key_package = generate_test_key_package("alice");

    // Register a new user via HTTP
    let result = api.register_user("alice", &alice_key_package).await;
    assert!(result.is_ok(), "User registration should succeed");

    // Verify by retrieving the user's key
    let retrieved_key = api
        .get_user_key("alice")
        .await
        .expect("Should retrieve user key");
    assert_eq!(
        retrieved_key, alice_key_package,
        "Retrieved key should match registered key"
    );
}

#[tokio::test]
async fn test_register_duplicate_user() {
    // Spawn a test HTTP server and run it in background
    let (server, addr) =
        mls_chat_server::server::create_test_http_server().expect("Failed to create test server");
    tokio::spawn(server);

    // Give server a moment to bind
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Create API client pointing to the test server
    let api = ServerApi::new(&format!("http://{}", addr));

    // Generate KeyPackages for bob
    let bob_key_package_1 = generate_test_key_package("bob");
    let bob_key_package_2 = generate_test_key_package("bob");

    // Register first user
    let result1 = api.register_user("bob", &bob_key_package_1).await;
    assert!(result1.is_ok(), "First registration should succeed");

    // Register duplicate user - should fail with HTTP 409 Conflict
    let result2 = api.register_user("bob", &bob_key_package_2).await;
    assert!(
        result2.is_err(),
        "Duplicate registration should fail with conflict error"
    );
}

#[tokio::test]
async fn test_get_user_key() {
    // Spawn a test HTTP server and run it in background
    let (server, addr) =
        mls_chat_server::server::create_test_http_server().expect("Failed to create test server");
    tokio::spawn(server);

    // Give server a moment to bind
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Create API client pointing to the test server
    let api = ServerApi::new(&format!("http://{}", addr));

    // Generate a KeyPackage for carol
    let carol_key_package = generate_test_key_package("carol");

    // Register a user via HTTP
    api.register_user("carol", &carol_key_package)
        .await
        .expect("Registration should succeed");

    // Fetch the user's key via HTTP
    let key = api
        .get_user_key("carol")
        .await
        .expect("Should retrieve user key");
    assert_eq!(
        key, carol_key_package,
        "Retrieved key should match registered key"
    );
}

#[tokio::test]
async fn test_get_nonexistent_user() {
    // Spawn a test HTTP server and run it in background
    let (server, addr) =
        mls_chat_server::server::create_test_http_server().expect("Failed to create test server");
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
    let (server, addr) =
        mls_chat_server::server::create_test_http_server().expect("Failed to create test server");
    tokio::spawn(server);

    // Give server a moment to bind
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Create API client pointing to the test server
    let api = ServerApi::new(&format!("http://{}", addr));

    // Generate KeyPackages for multiple users
    let alice_key_package = generate_test_key_package("alice");
    let bob_key_package = generate_test_key_package("bob");
    let carol_key_package = generate_test_key_package("carol");

    // Register multiple users via HTTP
    let alice_result = api.register_user("alice", &alice_key_package).await;
    let bob_result = api.register_user("bob", &bob_key_package).await;
    let carol_result = api.register_user("carol", &carol_key_package).await;

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

    assert_eq!(alice_key, alice_key_package);
    assert_eq!(bob_key, bob_key_package);
    assert_eq!(carol_key, carol_key_package);
}

#[tokio::test]
async fn test_health_check() {
    // Spawn a test HTTP server and run it in background
    let (server, addr) =
        mls_chat_server::server::create_test_http_server().expect("Failed to create test server");
    tokio::spawn(server);

    // Give server a moment to bind
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Create API client pointing to the test server
    let api = ServerApi::new(&format!("http://{}", addr));

    // Verify health check endpoint
    let result = api.health_check().await;
    assert!(result.is_ok(), "Health check should succeed");
}
