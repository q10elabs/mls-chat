/// Integration tests for the Server API client
///
/// Tests cover user registration, key retrieval, and health checks
/// using actual HTTP server endpoints via the ServerApi client.
use mls_chat_client::api::{KeyPackageUpload, ServerApi};
use mls_chat_client::crypto;
use tls_codec::Serialize;

use mls_chat_server::db::{self, DbPool};
use openmls_traits::OpenMlsProvider;
use rusqlite::params;

/// Generate a KeyPackageUpload payload for testing
fn generate_keypackage_upload(username: &str) -> KeyPackageUpload {
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("test.db");
    let provider =
        mls_chat_client::provider::MlsProvider::new(&db_path).expect("Failed to create provider");

    let (credential, sig_key) =
        crypto::generate_credential_with_key(username).expect("Failed to generate credential");

    let bundle = crypto::generate_key_package_bundle(&credential, &sig_key, &provider)
        .expect("Failed to generate key package bundle");

    let key_package = bundle.key_package();
    let keypackage_bytes = key_package
        .tls_serialize_detached()
        .expect("Failed to serialize key package");

    let hash_ref = key_package
        .hash_ref(provider.crypto())
        .expect("Failed to compute hash ref")
        .as_slice()
        .to_vec();

    let not_after = key_package.life_time().not_after() as i64;

    KeyPackageUpload {
        keypackage_ref: hash_ref,
        keypackage: keypackage_bytes,
        not_after,
    }
}

async fn spawn_server_with_pool() -> (String, DbPool) {
    use actix_web::web;

    let db_pool = db::create_test_pool();
    let pool_data = web::Data::new(db_pool.clone());
    let (server, addr) = mls_chat_server::server::create_test_http_server_with_pool(pool_data)
        .expect("Failed to create test server");
    tokio::spawn(server);

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    (addr, db_pool)
}

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

#[tokio::test]
async fn test_upload_keypackages_batch() {
    let (addr, _pool) = spawn_server_with_pool().await;
    let api = ServerApi::new(&format!("http://{}", addr));

    let uploads: Vec<KeyPackageUpload> = (0..3)
        .map(|_| generate_keypackage_upload("pool-user"))
        .collect();

    let response = api
        .upload_key_packages("pool-user", &uploads)
        .await
        .expect("Upload should succeed");

    assert_eq!(response.accepted, uploads.len());
    assert_eq!(response.pool_size, uploads.len());
    assert!(response.rejected.is_empty());
}

#[tokio::test]
async fn test_reserve_and_spend_keypackage_flow() {
    let (addr, _pool) = spawn_server_with_pool().await;
    let api = ServerApi::new(&format!("http://{}", addr));

    let uploads: Vec<KeyPackageUpload> = (0..2)
        .map(|_| generate_keypackage_upload("invitee"))
        .collect();

    api.upload_key_packages("invitee", &uploads)
        .await
        .expect("Upload should succeed");

    let group_id = vec![0x10, 0x20, 0x30, 0x40];
    let reservation = api
        .reserve_key_package("invitee", &group_id, "inviter")
        .await
        .expect("Reservation should succeed");

    assert!(!reservation.keypackage.is_empty());
    assert!(!reservation.keypackage_ref.is_empty());
    assert!(!reservation.reservation_id.is_empty());

    let status_after_reserve = api
        .get_key_package_status("invitee")
        .await
        .expect("Status should be available");
    assert_eq!(status_after_reserve.reserved, 1);

    api.spend_key_package(&reservation.keypackage_ref, &group_id, "inviter")
        .await
        .expect("Spending should succeed");

    let status_after_spend = api
        .get_key_package_status("invitee")
        .await
        .expect("Status should be queryable");
    assert_eq!(status_after_spend.spent, 1);
}

#[tokio::test]
async fn test_spend_prevents_double_spend() {
    let (addr, _pool) = spawn_server_with_pool().await;
    let api = ServerApi::new(&format!("http://{}", addr));

    let upload = vec![generate_keypackage_upload("double-spend")];
    api.upload_key_packages("double-spend", &upload)
        .await
        .expect("Upload should succeed");

    let group_id = vec![0x01, 0x02];
    let reservation = api
        .reserve_key_package("double-spend", &group_id, "inviter")
        .await
        .expect("Reservation should succeed");

    api.spend_key_package(&reservation.keypackage_ref, &group_id, "inviter")
        .await
        .expect("First spend should succeed");

    let second = api
        .spend_key_package(&reservation.keypackage_ref, &group_id, "inviter")
        .await;
    assert!(second.is_err(), "Second spend should be rejected");
    let err = second.err().unwrap();
    assert!(err.to_string().contains("already spent"));
}

#[tokio::test]
async fn test_reserve_on_empty_pool_returns_not_found() {
    let (addr, _pool) = spawn_server_with_pool().await;
    let api = ServerApi::new(&format!("http://{}", addr));

    let result = api
        .reserve_key_package("no-keys", &[0x01, 0x02], "inviter")
        .await;

    assert!(result.is_err(), "Reserve should fail when pool empty");
    assert!(result
        .err()
        .unwrap()
        .to_string()
        .contains("No available KeyPackage"));
}

#[tokio::test]
async fn test_expired_key_rejected() {
    let (addr, pool) = spawn_server_with_pool().await;
    let api = ServerApi::new(&format!("http://{}", addr));

    let upload = vec![generate_keypackage_upload("expiry-test")];
    api.upload_key_packages("expiry-test", &upload)
        .await
        .expect("Upload should succeed");

    {
        let conn = pool.lock().await;
        let now = chrono::Utc::now().timestamp() - 10;
        conn.execute(
            "UPDATE keypackages SET not_after = ?1 WHERE username = ?2",
            params![now, "expiry-test"],
        )
        .expect("Failed to update expiration");
    }

    let result = api
        .reserve_key_package("expiry-test", &[0x01, 0x02, 0x03], "inviter")
        .await;

    assert!(result.is_err(), "Expired key should not be reserved");
}

#[tokio::test]
async fn test_reservation_timeout_releases_key() {
    let (addr, pool) = spawn_server_with_pool().await;
    let api = ServerApi::new(&format!("http://{}", addr));

    let upload = vec![generate_keypackage_upload("ttl-user")];
    api.upload_key_packages("ttl-user", &upload)
        .await
        .expect("Upload should succeed");

    let group_id = vec![0xaa, 0xbb];
    let reservation = api
        .reserve_key_package("ttl-user", &group_id, "inviter")
        .await
        .expect("Reservation should succeed");

    {
        let conn = pool.lock().await;
        conn.execute(
            "UPDATE keypackages SET reservation_expires_at = 0 WHERE keypackage_ref = ?1",
            params![reservation.keypackage_ref],
        )
        .expect("Failed to update reservation expiry");
    }

    // Next reserve should succeed because server releases expired reservations
    let second = api
        .reserve_key_package("ttl-user", &group_id, "inviter")
        .await
        .expect("Reservation should be released and succeed");

    assert_eq!(second.keypackage_ref, reservation.keypackage_ref);
}

#[tokio::test]
async fn test_concurrent_multi_inviter() {
    let (addr, _pool) = spawn_server_with_pool().await;
    let api = ServerApi::new(&format!("http://{}", addr));

    // Upload 3 KeyPackages for the target user
    let uploads: Vec<KeyPackageUpload> = (0..3)
        .map(|_| generate_keypackage_upload("target-user"))
        .collect();

    api.upload_key_packages("target-user", &uploads)
        .await
        .expect("Upload should succeed");

    // Spawn 3 concurrent inviters attempting to reserve KeyPackages for the same target
    let group_id1 = vec![0x01, 0x02];
    let group_id2 = vec![0x03, 0x04];
    let group_id3 = vec![0x05, 0x06];

    let api1 = api.clone();
    let api2 = api.clone();
    let api3 = api.clone();

    let handle1 = tokio::spawn(async move {
        api1.reserve_key_package("target-user", &group_id1, "inviter1")
            .await
    });

    let handle2 = tokio::spawn(async move {
        api2.reserve_key_package("target-user", &group_id2, "inviter2")
            .await
    });

    let handle3 = tokio::spawn(async move {
        api3.reserve_key_package("target-user", &group_id3, "inviter3")
            .await
    });

    // Wait for all reservations to complete
    let reservation1 = handle1
        .await
        .expect("Join should succeed")
        .expect("Reservation 1 should succeed");
    let reservation2 = handle2
        .await
        .expect("Join should succeed")
        .expect("Reservation 2 should succeed");
    let reservation3 = handle3
        .await
        .expect("Join should succeed")
        .expect("Reservation 3 should succeed");

    // Verify each inviter got a unique KeyPackage
    assert_ne!(
        reservation1.keypackage_ref, reservation2.keypackage_ref,
        "Inviter 1 and 2 should get different KeyPackages"
    );
    assert_ne!(
        reservation1.keypackage_ref, reservation3.keypackage_ref,
        "Inviter 1 and 3 should get different KeyPackages"
    );
    assert_ne!(
        reservation2.keypackage_ref, reservation3.keypackage_ref,
        "Inviter 2 and 3 should get different KeyPackages"
    );

    // Verify each reservation has a unique ID
    assert_ne!(reservation1.reservation_id, reservation2.reservation_id);
    assert_ne!(reservation1.reservation_id, reservation3.reservation_id);
    assert_ne!(reservation2.reservation_id, reservation3.reservation_id);

    // Verify pool status shows all 3 reserved
    let status = api
        .get_key_package_status("target-user")
        .await
        .expect("Status should be available");
    assert_eq!(status.reserved, 3, "All 3 KeyPackages should be reserved");
    assert_eq!(
        status.available, 0,
        "No KeyPackages should remain available"
    );
}

#[tokio::test]
async fn test_structured_error_types() {
    use mls_chat_client::error::{ClientError, KeyPackageError, NetworkError};

    let (addr, _pool) = spawn_server_with_pool().await;
    let api = ServerApi::new(&format!("http://{}", addr));

    // Test PoolExhausted error
    let result = api
        .reserve_key_package("no-keys", &[0x01, 0x02], "inviter")
        .await;
    assert!(result.is_err());
    match result.err().unwrap() {
        ClientError::Network(NetworkError::KeyPackage(KeyPackageError::PoolExhausted {
            username,
        })) => {
            assert_eq!(username, "no-keys");
        }
        other => panic!("Expected PoolExhausted error, got: {:?}", other),
    }

    // Upload and reserve a KeyPackage
    let upload = vec![generate_keypackage_upload("error-test")];
    api.upload_key_packages("error-test", &upload)
        .await
        .expect("Upload should succeed");

    let group_id = vec![0x01, 0x02];
    let reservation = api
        .reserve_key_package("error-test", &group_id, "inviter")
        .await
        .expect("Reservation should succeed");

    // Test DoubleSpendAttempted error
    api.spend_key_package(&reservation.keypackage_ref, &group_id, "inviter")
        .await
        .expect("First spend should succeed");

    let result = api
        .spend_key_package(&reservation.keypackage_ref, &group_id, "inviter")
        .await;
    assert!(result.is_err());
    match result.err().unwrap() {
        ClientError::Network(NetworkError::KeyPackage(KeyPackageError::DoubleSpendAttempted {
            keypackage_ref,
        })) => {
            assert_eq!(keypackage_ref, reservation.keypackage_ref);
        }
        other => panic!("Expected DoubleSpendAttempted error, got: {:?}", other),
    }

    // Test InvalidKeyPackageRef error
    let fake_ref = vec![0xde, 0xad, 0xbe, 0xef];
    let result = api.spend_key_package(&fake_ref, &group_id, "inviter").await;
    assert!(result.is_err());
    match result.err().unwrap() {
        ClientError::Network(NetworkError::KeyPackage(KeyPackageError::InvalidKeyPackageRef {
            keypackage_ref,
        })) => {
            assert_eq!(keypackage_ref, fake_ref);
        }
        other => panic!("Expected InvalidKeyPackageRef error, got: {:?}", other),
    }
}
