/// Integration tests for REST API endpoints
/// Tests database operations and error handling through direct DB calls
use mls_chat_server::db::{Database, DbPool};

#[tokio::test]
async fn test_user_registration_workflow() {
    let pool = mls_chat_server::db::create_test_pool();

    let alice_key = vec![0x01, 0x02, 0x03, 0x04];
    let bob_key = vec![0x05, 0x06, 0x07, 0x08];

    // Register first user
    let user1 = Database::register_user(&pool, "alice", &alice_key)
        .await
        .expect("Failed to register alice");
    assert_eq!(user1.username, "alice");
    assert_eq!(user1.key_package, alice_key);

    // Register second user
    let user2 = Database::register_user(&pool, "bob", &bob_key)
        .await
        .expect("Failed to register bob");
    assert_eq!(user2.username, "bob");
    assert_eq!(user2.key_package, bob_key);

    // Retrieve alice's key package
    let retrieved_alice = Database::get_user(&pool, "alice")
        .await
        .expect("Query failed")
        .expect("User not found");
    assert_eq!(retrieved_alice.username, "alice");
    assert_eq!(retrieved_alice.key_package, alice_key);

    // Retrieve bob's key package
    let retrieved_bob = Database::get_user(&pool, "bob")
        .await
        .expect("Query failed")
        .expect("User not found");
    assert_eq!(retrieved_bob.username, "bob");
    assert_eq!(retrieved_bob.key_package, bob_key);
}

#[tokio::test]
async fn test_backup_storage_workflow() {
    let pool = mls_chat_server::db::create_test_pool();

    let charlie_key = vec![0x09, 0x0a, 0x0b, 0x0c];

    // Register user
    Database::register_user(&pool, "charlie", &charlie_key)
        .await
        .expect("Failed to register");

    // Store backup
    let backup1 = Database::store_backup(&pool, "charlie", "encrypted_state_data_v1")
        .await
        .expect("Failed to store backup");
    assert_eq!(backup1.username, "charlie");
    assert_eq!(backup1.encrypted_state, "encrypted_state_data_v1");

    // Retrieve backup
    let retrieved = Database::get_backup(&pool, "charlie")
        .await
        .expect("Query failed")
        .expect("Backup not found");
    assert_eq!(retrieved.username, "charlie");
    assert_eq!(retrieved.encrypted_state, "encrypted_state_data_v1");

    // Update backup with new state
    let backup2 = Database::store_backup(&pool, "charlie", "encrypted_state_data_v2")
        .await
        .expect("Failed to update backup");
    assert_eq!(backup2.encrypted_state, "encrypted_state_data_v2");

    // Verify latest backup is retrieved
    let latest = Database::get_backup(&pool, "charlie")
        .await
        .expect("Query failed")
        .expect("Backup not found");
    assert_eq!(latest.encrypted_state, "encrypted_state_data_v2");
}

#[tokio::test]
async fn test_multiple_users_different_backups() {
    let pool = mls_chat_server::db::create_test_pool();

    let user1_key = vec![0x0d, 0x0e, 0x0f, 0x10];
    let user2_key = vec![0x11, 0x12, 0x13, 0x14];

    // Register users
    Database::register_user(&pool, "user1", &user1_key)
        .await
        .expect("Failed to register");
    Database::register_user(&pool, "user2", &user2_key)
        .await
        .expect("Failed to register");

    // Store different backups
    Database::store_backup(&pool, "user1", "backup1")
        .await
        .expect("Failed to store backup");
    Database::store_backup(&pool, "user2", "backup2")
        .await
        .expect("Failed to store backup");

    // Retrieve user1's backup
    let backup1 = Database::get_backup(&pool, "user1")
        .await
        .expect("Query failed")
        .expect("Backup not found");
    assert_eq!(backup1.encrypted_state, "backup1");

    // Retrieve user2's backup
    let backup2 = Database::get_backup(&pool, "user2")
        .await
        .expect("Query failed")
        .expect("Backup not found");
    assert_eq!(backup2.encrypted_state, "backup2");
}

#[tokio::test]
async fn test_group_creation_and_retrieval() {
    let pool = mls_chat_server::db::create_test_pool();

    // Create groups
    let group1 = Database::create_group(&pool, "team_alpha", "Team Alpha")
        .await
        .expect("Failed to create group");
    assert_eq!(group1.group_id, "team_alpha");
    assert_eq!(group1.name, "Team Alpha");

    let group2 = Database::create_group(&pool, "team_beta", "Team Beta")
        .await
        .expect("Failed to create group");
    assert_eq!(group2.group_id, "team_beta");

    // Retrieve groups
    let retrieved1 = Database::get_group(&pool, "team_alpha")
        .await
        .expect("Query failed")
        .expect("Group not found");
    assert_eq!(retrieved1.group_id, "team_alpha");

    let retrieved2 = Database::get_group(&pool, "team_beta")
        .await
        .expect("Query failed")
        .expect("Group not found");
    assert_eq!(retrieved2.group_id, "team_beta");
}

#[tokio::test]
async fn test_message_storage_and_retrieval() {
    let pool = mls_chat_server::db::create_test_pool();

    let alice_key = vec![0x15, 0x16, 0x17, 0x18];
    let bob_key = vec![0x19, 0x1a, 0x1b, 0x1c];

    // Register users
    let user1 = Database::register_user(&pool, "alice", &alice_key)
        .await
        .expect("Failed to register");
    let user2 = Database::register_user(&pool, "bob", &bob_key)
        .await
        .expect("Failed to register");

    // Create group
    let group = Database::create_group(&pool, "team", "Team")
        .await
        .expect("Failed to create group");

    // Store messages
    let msg1 = Database::store_message(&pool, group.id, user1.id, "hello from alice")
        .await
        .expect("Failed to store message");
    assert_eq!(msg1.encrypted_content, "hello from alice");
    assert_eq!(msg1.sender_id, user1.id);

    let msg2 = Database::store_message(&pool, group.id, user2.id, "hello from bob")
        .await
        .expect("Failed to store message");
    assert_eq!(msg2.encrypted_content, "hello from bob");
    assert_eq!(msg2.sender_id, user2.id);

    // Retrieve messages
    let messages = Database::get_group_messages(&pool, group.id, 10)
        .await
        .expect("Failed to get messages");
    assert_eq!(messages.len(), 2);
}

#[tokio::test]
async fn test_duplicate_username_error() {
    let pool = mls_chat_server::db::create_test_pool();

    let key1 = vec![0x1d, 0x1e, 0x1f, 0x20];
    let key2 = vec![0x21, 0x22, 0x23, 0x24];

    // Register user
    Database::register_user(&pool, "alice", &key1)
        .await
        .expect("Failed to register");

    // Try to register duplicate
    let result = Database::register_user(&pool, "alice", &key2).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("UNIQUE"));
}

#[tokio::test]
async fn test_get_nonexistent_user() {
    let pool = mls_chat_server::db::create_test_pool();

    let result = Database::get_user(&pool, "nonexistent")
        .await
        .expect("Query failed");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_nonexistent_backup() {
    let pool = mls_chat_server::db::create_test_pool();

    let result = Database::get_backup(&pool, "nonexistent")
        .await
        .expect("Query failed");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_nonexistent_group() {
    let pool = mls_chat_server::db::create_test_pool();

    let result = Database::get_group(&pool, "nonexistent")
        .await
        .expect("Query failed");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_complex_workflow() {
    let pool = mls_chat_server::db::create_test_pool();

    let alice_key = vec![0x25, 0x26, 0x27, 0x28];
    let bob_key = vec![0x29, 0x2a, 0x2b, 0x2c];

    // Create users
    let alice = Database::register_user(&pool, "alice", &alice_key)
        .await
        .expect("Failed to register");
    let bob = Database::register_user(&pool, "bob", &bob_key)
        .await
        .expect("Failed to register");

    // Create groups
    let project_a = Database::create_group(&pool, "project_a", "Project A")
        .await
        .expect("Failed to create group");
    let project_b = Database::create_group(&pool, "project_b", "Project B")
        .await
        .expect("Failed to create group");

    // Store messages to different groups
    Database::store_message(&pool, project_a.id, alice.id, "alice msg in a")
        .await
        .expect("Failed to store");
    Database::store_message(&pool, project_a.id, bob.id, "bob msg in a")
        .await
        .expect("Failed to store");

    Database::store_message(&pool, project_b.id, alice.id, "alice msg in b")
        .await
        .expect("Failed to store");
    Database::store_message(&pool, project_b.id, bob.id, "bob msg in b")
        .await
        .expect("Failed to store");

    // Store backups
    Database::store_backup(&pool, "alice", "alice backup 1")
        .await
        .expect("Failed to store");
    Database::store_backup(&pool, "bob", "bob backup 1")
        .await
        .expect("Failed to store");

    // Verify data integrity
    let alice_retrieved = Database::get_user(&pool, "alice")
        .await
        .expect("Query failed")
        .expect("User not found");
    assert_eq!(alice_retrieved.key_package, alice_key);

    let alice_backup = Database::get_backup(&pool, "alice")
        .await
        .expect("Query failed")
        .expect("Backup not found");
    assert_eq!(alice_backup.encrypted_state, "alice backup 1");

    let messages_a = Database::get_group_messages(&pool, project_a.id, 10)
        .await
        .expect("Failed to get messages");
    assert_eq!(messages_a.len(), 2);

    let messages_b = Database::get_group_messages(&pool, project_b.id, 10)
        .await
        .expect("Failed to get messages");
    assert_eq!(messages_b.len(), 2);
}
