//! Integration tests for LocalStore
//!
//! Tests the storage layer for identity management and KeyPackage pool metadata

use mls_chat_client::storage::LocalStore;
use tempfile::TempDir;

fn setup_store() -> (LocalStore, TempDir) {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let store = LocalStore::new(&db_path).unwrap();
    (store, temp_dir)
}

// ===== Identity Tests (verify existing functionality) =====

#[test]
fn test_identity_save_and_load() {
    let (store, _temp) = setup_store();

    let public_key = b"test_public_key_blob";
    store.save_identity("alice", public_key).unwrap();

    let loaded = store.load_public_key("alice").unwrap().unwrap();
    assert_eq!(loaded, public_key);
}

#[test]
fn test_identity_load_nonexistent() {
    let (store, _temp) = setup_store();

    let result = store.load_public_key("nonexistent").unwrap();
    assert!(result.is_none());
}

// ===== KeyPackage Pool Metadata Tests =====

#[test]
fn test_create_pool_metadata() {
    let (store, _temp) = setup_store();

    let keypackage_ref = b"test_ref_123";
    let not_after = 1700000000i64; // Some future timestamp

    store.create_pool_metadata(keypackage_ref, not_after).unwrap();

    // Verify it was created with 'created' status
    let count = store.count_by_status("created").unwrap();
    assert_eq!(count, 1);
}

#[test]
fn test_create_multiple_pool_metadata() {
    let (store, _temp) = setup_store();

    for i in 0..5 {
        let keypackage_ref = format!("ref_{}", i).into_bytes();
        let not_after = 1700000000i64 + i as i64;
        store.create_pool_metadata(&keypackage_ref, not_after).unwrap();
    }

    let count = store.count_by_status("created").unwrap();
    assert_eq!(count, 5);
}

#[test]
fn test_update_pool_metadata_status() {
    let (store, _temp) = setup_store();

    let keypackage_ref = b"test_ref";
    store.create_pool_metadata(keypackage_ref, 1700000000).unwrap();

    // Update to uploaded
    store.update_pool_metadata_status(keypackage_ref, "uploaded").unwrap();

    let created_count = store.count_by_status("created").unwrap();
    let uploaded_count = store.count_by_status("uploaded").unwrap();

    assert_eq!(created_count, 0);
    assert_eq!(uploaded_count, 1);
}

#[test]
fn test_status_transitions() {
    let (store, _temp) = setup_store();

    let keypackage_ref = b"test_ref";
    store.create_pool_metadata(keypackage_ref, 1700000000).unwrap();

    // created -> uploaded -> available -> reserved -> spent
    let transitions = vec!["uploaded", "available", "reserved", "spent"];

    for status in transitions {
        store.update_pool_metadata_status(keypackage_ref, status).unwrap();
        let count = store.count_by_status(status).unwrap();
        assert_eq!(count, 1, "Failed transition to {}", status);
    }
}

#[test]
fn test_count_by_status_multiple_statuses() {
    let (store, _temp) = setup_store();

    // Create 3 in created state
    for i in 0..3 {
        let ref_bytes = format!("created_{}", i).into_bytes();
        store.create_pool_metadata(&ref_bytes, 1700000000).unwrap();
    }

    // Create 2 in uploaded state
    for i in 0..2 {
        let ref_bytes = format!("uploaded_{}", i).into_bytes();
        store.create_pool_metadata(&ref_bytes, 1700000000).unwrap();
        store.update_pool_metadata_status(&ref_bytes, "uploaded").unwrap();
    }

    // Create 1 in spent state
    let spent_ref = b"spent_0";
    store.create_pool_metadata(spent_ref, 1700000000).unwrap();
    store.update_pool_metadata_status(spent_ref, "spent").unwrap();

    assert_eq!(store.count_by_status("created").unwrap(), 3);
    assert_eq!(store.count_by_status("uploaded").unwrap(), 2);
    assert_eq!(store.count_by_status("spent").unwrap(), 1);
    assert_eq!(store.count_by_status("available").unwrap(), 0);
}

#[test]
fn test_get_expired_refs() {
    let (store, _temp) = setup_store();

    let now = 1700000000i64;

    // Create 3 expired keys (not_after < now)
    for i in 0..3 {
        let ref_bytes = format!("expired_{}", i).into_bytes();
        let not_after = now - 1000 - i as i64;
        store.create_pool_metadata(&ref_bytes, not_after).unwrap();
    }

    // Create 2 valid keys (not_after >= now)
    for i in 0..2 {
        let ref_bytes = format!("valid_{}", i).into_bytes();
        let not_after = now + 1000 + i as i64;
        store.create_pool_metadata(&ref_bytes, not_after).unwrap();
    }

    let expired_refs = store.get_expired_refs(now).unwrap();
    assert_eq!(expired_refs.len(), 3);

    // Verify the expired refs are correct
    let expired_names: Vec<String> = expired_refs.iter()
        .map(|r| String::from_utf8(r.clone()).unwrap())
        .collect();

    assert!(expired_names.contains(&"expired_0".to_string()));
    assert!(expired_names.contains(&"expired_1".to_string()));
    assert!(expired_names.contains(&"expired_2".to_string()));
}

#[test]
fn test_get_expired_refs_empty() {
    let (store, _temp) = setup_store();

    let now = 1700000000i64;

    // Create only valid keys
    for i in 0..3 {
        let ref_bytes = format!("valid_{}", i).into_bytes();
        let not_after = now + 1000;
        store.create_pool_metadata(&ref_bytes, not_after).unwrap();
    }

    let expired_refs = store.get_expired_refs(now).unwrap();
    assert_eq!(expired_refs.len(), 0);
}

#[test]
fn test_get_metadata_by_status() {
    let (store, _temp) = setup_store();

    // Create 2 keys with 'available' status
    for i in 0..2 {
        let ref_bytes = format!("available_{}", i).into_bytes();
        store.create_pool_metadata(&ref_bytes, 1700000000 + i as i64).unwrap();
        store.update_pool_metadata_status(&ref_bytes, "available").unwrap();
    }

    // Create 1 key with 'reserved' status
    let reserved_ref = b"reserved_0";
    store.create_pool_metadata(reserved_ref, 1700000000).unwrap();
    store.update_pool_metadata_status(reserved_ref, "reserved").unwrap();

    let available_metadata = store.get_metadata_by_status("available").unwrap();
    assert_eq!(available_metadata.len(), 2);

    for metadata in &available_metadata {
        assert_eq!(metadata.status, "available");
        // uploaded_at is only set when status is explicitly updated to "uploaded"
        // In this test, we go directly from created -> available, so uploaded_at may not be set
        assert!(metadata.reserved_at.is_none());
        assert!(metadata.spent_at.is_none());
    }

    let reserved_metadata = store.get_metadata_by_status("reserved").unwrap();
    assert_eq!(reserved_metadata.len(), 1);
    assert_eq!(reserved_metadata[0].status, "reserved");
    assert!(reserved_metadata[0].reserved_at.is_some());
}

#[test]
fn test_get_metadata_by_status_empty() {
    let (store, _temp) = setup_store();

    let metadata = store.get_metadata_by_status("available").unwrap();
    assert_eq!(metadata.len(), 0);
}

#[test]
fn test_delete_pool_metadata() {
    let (store, _temp) = setup_store();

    let keypackage_ref = b"test_ref";
    store.create_pool_metadata(keypackage_ref, 1700000000).unwrap();

    // Verify it exists
    let count_before = store.count_by_status("created").unwrap();
    assert_eq!(count_before, 1);

    // Delete it
    store.delete_pool_metadata(keypackage_ref).unwrap();

    // Verify it's gone
    let count_after = store.count_by_status("created").unwrap();
    assert_eq!(count_after, 0);
}

#[test]
fn test_delete_pool_metadata_nonexistent() {
    let (store, _temp) = setup_store();

    // Deleting nonexistent should not error (idempotent)
    let result = store.delete_pool_metadata(b"nonexistent");
    assert!(result.is_ok());
}

#[test]
fn test_update_reservation_info() {
    let (store, _temp) = setup_store();

    let keypackage_ref = b"test_ref";
    store.create_pool_metadata(keypackage_ref, 1700000000).unwrap();

    let reservation_id = "res_123";
    let reserved_by = "alice";
    let expires_at = 1700001000i64;

    store.update_reservation_info(
        keypackage_ref,
        reservation_id,
        reserved_by,
        expires_at
    ).unwrap();

    // Verify reservation info was stored
    let metadata = store.get_metadata_by_status("reserved").unwrap();
    assert_eq!(metadata.len(), 1);

    let meta = &metadata[0];
    assert_eq!(meta.status, "reserved");
    assert_eq!(meta.reservation_id.as_ref().unwrap(), reservation_id);
    assert_eq!(meta.reserved_by.as_ref().unwrap(), reserved_by);
    assert_eq!(meta.reservation_expires_at.unwrap(), expires_at);
    assert!(meta.reserved_at.is_some());
}

#[test]
fn test_mark_spent() {
    let (store, _temp) = setup_store();

    let keypackage_ref = b"test_ref";
    store.create_pool_metadata(keypackage_ref, 1700000000).unwrap();

    let spent_by = "bob";
    let group_id = b"group_abc123";

    store.mark_spent(keypackage_ref, spent_by, group_id).unwrap();

    // Verify spend info was stored
    let metadata = store.get_metadata_by_status("spent").unwrap();
    assert_eq!(metadata.len(), 1);

    let meta = &metadata[0];
    assert_eq!(meta.status, "spent");
    assert_eq!(meta.spent_by.as_ref().unwrap(), spent_by);
    assert_eq!(meta.spent_group_id.as_ref().unwrap(), group_id);
    assert!(meta.spent_at.is_some());
}

#[test]
fn test_full_lifecycle() {
    let (store, _temp) = setup_store();

    let keypackage_ref = b"lifecycle_test";
    let not_after = 1700000000i64;

    // 1. Create
    store.create_pool_metadata(keypackage_ref, not_after).unwrap();
    assert_eq!(store.count_by_status("created").unwrap(), 1);

    // 2. Upload
    store.update_pool_metadata_status(keypackage_ref, "uploaded").unwrap();
    assert_eq!(store.count_by_status("uploaded").unwrap(), 1);
    assert_eq!(store.count_by_status("created").unwrap(), 0);

    // 3. Make available
    store.update_pool_metadata_status(keypackage_ref, "available").unwrap();
    assert_eq!(store.count_by_status("available").unwrap(), 1);

    // 4. Reserve
    store.update_reservation_info(
        keypackage_ref,
        "res_001",
        "alice",
        1700001000
    ).unwrap();
    assert_eq!(store.count_by_status("reserved").unwrap(), 1);
    assert_eq!(store.count_by_status("available").unwrap(), 0);

    // 5. Spend
    store.mark_spent(keypackage_ref, "bob", b"group_123").unwrap();
    assert_eq!(store.count_by_status("spent").unwrap(), 1);
    assert_eq!(store.count_by_status("reserved").unwrap(), 0);

    // 6. Verify final state
    let metadata = store.get_metadata_by_status("spent").unwrap();
    assert_eq!(metadata.len(), 1);

    let meta = &metadata[0];
    assert_eq!(meta.keypackage_ref, keypackage_ref);
    assert!(meta.created_at > 0);
    assert!(meta.uploaded_at.is_some());
    assert!(meta.reserved_at.is_some());
    assert!(meta.spent_at.is_some());
    assert_eq!(meta.not_after, not_after);
    assert_eq!(meta.reservation_id.as_ref().unwrap(), "res_001");
    assert_eq!(meta.reserved_by.as_ref().unwrap(), "alice");
    assert_eq!(meta.spent_by.as_ref().unwrap(), "bob");
    assert_eq!(meta.spent_group_id.as_ref().unwrap(), b"group_123");
}

#[test]
fn test_multiple_keys_different_states() {
    let (store, _temp) = setup_store();

    // Create keys in various states
    let states = vec![
        ("ref_created", "created"),
        ("ref_uploaded", "uploaded"),
        ("ref_available", "available"),
        ("ref_reserved", "reserved"),
        ("ref_spent", "spent"),
    ];

    for (ref_name, final_status) in &states {
        let ref_bytes = ref_name.as_bytes();
        store.create_pool_metadata(ref_bytes, 1700000000).unwrap();

        if *final_status != "created" {
            store.update_pool_metadata_status(ref_bytes, final_status).unwrap();
        }
    }

    // Verify counts
    assert_eq!(store.count_by_status("created").unwrap(), 1);
    assert_eq!(store.count_by_status("uploaded").unwrap(), 1);
    assert_eq!(store.count_by_status("available").unwrap(), 1);
    assert_eq!(store.count_by_status("reserved").unwrap(), 1);
    assert_eq!(store.count_by_status("spent").unwrap(), 1);
}

#[test]
fn test_timestamps_are_set() {
    let (store, _temp) = setup_store();

    let keypackage_ref = b"timestamp_test";
    store.create_pool_metadata(keypackage_ref, 1700000000).unwrap();

    let metadata = store.get_metadata_by_status("created").unwrap();
    assert_eq!(metadata.len(), 1);

    let meta = &metadata[0];

    // created_at should be set
    assert!(meta.created_at > 0);

    // Others should be None initially
    assert!(meta.uploaded_at.is_none());
    assert!(meta.reserved_at.is_none());
    assert!(meta.spent_at.is_none());

    // Update to uploaded
    store.update_pool_metadata_status(keypackage_ref, "uploaded").unwrap();
    let metadata = store.get_metadata_by_status("uploaded").unwrap();
    assert!(metadata[0].uploaded_at.is_some());
    assert!(metadata[0].uploaded_at.unwrap() >= meta.created_at);
}

#[test]
fn test_expiry_edge_cases() {
    let (store, _temp) = setup_store();

    let now = 1700000000i64;

    // Key expiring exactly at 'now'
    let ref_exact = b"exact";
    store.create_pool_metadata(ref_exact, now).unwrap();

    // Key expiring 1 second before 'now'
    let ref_before = b"before";
    store.create_pool_metadata(ref_before, now - 1).unwrap();

    // Key expiring 1 second after 'now'
    let ref_after = b"after";
    store.create_pool_metadata(ref_after, now + 1).unwrap();

    // Get expired keys (not_after < now)
    let expired_refs = store.get_expired_refs(now).unwrap();

    // Only ref_before should be expired
    assert_eq!(expired_refs.len(), 1);
    assert_eq!(expired_refs[0], b"before");
}

#[test]
fn test_unique_keypackage_refs() {
    let (store, _temp) = setup_store();

    let keypackage_ref = b"duplicate_ref";

    // Create first time
    store.create_pool_metadata(keypackage_ref, 1700000000).unwrap();

    // Try to create again with same ref - should fail (PRIMARY KEY constraint)
    let result = store.create_pool_metadata(keypackage_ref, 1700000000);
    assert!(result.is_err());
}
