//! Integration tests for the KeyPackage pool core logic

use std::time::{Duration, SystemTime};

use mls_chat_client::crypto::generate_credential_with_key;
use mls_chat_client::mls::{KeyPackagePool, KeyPackagePoolConfig};
use mls_chat_client::provider::MlsProvider;
use mls_chat_client::storage::LocalStore;
use openmls::prelude::KeyPackageBundle;
use openmls_traits::storage::traits as storage_traits;
use openmls_traits::storage::{self, StorageProvider};
use openmls_traits::OpenMlsProvider;
use serde::{Deserialize, Serialize};
use tempfile::{tempdir, TempDir};

fn setup_store() -> (LocalStore, TempDir) {
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let store = LocalStore::new(&db_path).unwrap();
    (store, temp_dir)
}

fn setup_pool<'a>(store: &'a LocalStore) -> (KeyPackagePool<'a>, MlsProvider) {
    let provider = MlsProvider::new_in_memory().unwrap();
    let pool = KeyPackagePool::new("alice", KeyPackagePoolConfig::default(), store);
    (pool, provider)
}

#[derive(Clone, Serialize, Deserialize)]
struct TestStoredKeyPackageRef(Vec<u8>);

impl storage_traits::HashReference<{ storage::CURRENT_VERSION }> for TestStoredKeyPackageRef {}

impl storage::Key<{ storage::CURRENT_VERSION }> for TestStoredKeyPackageRef {}

fn storage_contains_key(provider: &MlsProvider, hash: &[u8]) -> bool {
    let reference = TestStoredKeyPackageRef(hash.to_vec());
    provider
        .storage()
        .key_package::<_, KeyPackageBundle>(&reference)
        .unwrap()
        .is_some()
}

#[tokio::test]
async fn generate_and_update_pool_creates_entries() {
    let (store, _temp) = setup_store();
    let (pool, provider) = setup_pool(&store);
    let (credential, signer) = generate_credential_with_key("alice").unwrap();

    let refs = pool
        .generate_and_update_pool(3, &credential, &signer, &provider)
        .await
        .unwrap();

    assert_eq!(refs.len(), 3);
    assert_eq!(pool.get_available_count().unwrap(), 0);
    assert_eq!(store.count_by_status("created").unwrap(), 3);
}

#[tokio::test]
async fn generate_and_update_pool_enforces_hard_cap() {
    let (store, _temp) = setup_store();
    let config = KeyPackagePoolConfig {
        hard_cap: 2,
        ..Default::default()
    };
    let pool = KeyPackagePool::new("alice", config, &store);
    let provider = MlsProvider::new_in_memory().unwrap();
    let (credential, signer) = generate_credential_with_key("alice").unwrap();

    pool.generate_and_update_pool(2, &credential, &signer, &provider)
        .await
        .unwrap();

    let err = pool
        .generate_and_update_pool(1, &credential, &signer, &provider)
        .await
        .unwrap_err();
    assert!(format!("{}", err).contains("pool capacity exceeded"));
}

#[test]
fn should_replenish_logic() {
    let (store, _temp) = setup_store();
    let config = KeyPackagePoolConfig {
        low_watermark: 2,
        ..Default::default()
    };
    let pool = KeyPackagePool::new("alice", config, &store);

    store.create_pool_metadata(b"ref1", 2_000_000).unwrap();
    store
        .update_pool_metadata_status(b"ref1", "available")
        .unwrap();

    assert!(pool.should_replenish().unwrap());

    store.create_pool_metadata(b"ref2", 2_000_000).unwrap();
    store
        .update_pool_metadata_status(b"ref2", "available")
        .unwrap();

    assert!(!pool.should_replenish().unwrap());
}

#[test]
fn get_replenishment_needed() {
    let (store, _temp) = setup_store();
    let config = KeyPackagePoolConfig {
        target_pool_size: 5,
        ..Default::default()
    };
    let pool = KeyPackagePool::new("alice", config, &store);

    for i in 0..3 {
        let ref_name = format!("ref{}", i);
        store
            .create_pool_metadata(ref_name.as_bytes(), 2_000_000)
            .unwrap();
        store
            .update_pool_metadata_status(ref_name.as_bytes(), "available")
            .unwrap();
    }

    let needed = pool.get_replenishment_needed().unwrap();
    assert_eq!(needed, Some(2));

    for i in 3..5 {
        let ref_name = format!("ref{}", i);
        store
            .create_pool_metadata(ref_name.as_bytes(), 2_000_000)
            .unwrap();
        store
            .update_pool_metadata_status(ref_name.as_bytes(), "available")
            .unwrap();
    }

    let needed = pool.get_replenishment_needed().unwrap();
    assert!(needed.is_none());
}

#[test]
fn mark_as_spent_updates_status() {
    let (store, _temp) = setup_store();
    let pool = KeyPackagePool::new("alice", KeyPackagePoolConfig::default(), &store);

    store.create_pool_metadata(b"ref", 2_000_000).unwrap();
    store
        .update_pool_metadata_status(b"ref", "available")
        .unwrap();

    pool.mark_as_spent(b"ref").unwrap();
    assert_eq!(store.count_by_status("spent").unwrap(), 1);
}

#[tokio::test]
async fn cleanup_expired_removes_entries() {
    let (store, _temp) = setup_store();
    let (pool, provider) = setup_pool(&store);
    let (credential, signer) = generate_credential_with_key("alice").unwrap();

    let refs = pool
        .generate_and_update_pool(1, &credential, &signer, &provider)
        .await
        .unwrap();

    let metadata = store.get_metadata_by_status("created").unwrap();
    let not_after = metadata[0].not_after;
    let expiry_time = SystemTime::UNIX_EPOCH + Duration::from_secs((not_after + 1) as u64);

    store
        .update_pool_metadata_status(&refs[0], "expired")
        .unwrap();

    assert!(storage_contains_key(&provider, &refs[0]));

    let removed = pool.cleanup_expired(&provider, expiry_time).unwrap();
    assert_eq!(removed, 1);
    assert_eq!(store.count_by_status("expired").unwrap(), 0);
    assert!(!storage_contains_key(&provider, &refs[0]));
}

#[test]
fn status_totals_property() {
    let (store, _temp) = setup_store();
    let config = KeyPackagePoolConfig {
        low_watermark: 1,
        ..Default::default()
    };
    let pool = KeyPackagePool::new("alice", config, &store);

    let statuses = ["created", "available", "reserved", "spent", "expired"];
    for (i, status) in statuses.iter().enumerate() {
        let ref_name = format!("ref{}", i);
        store
            .create_pool_metadata(ref_name.as_bytes(), 2_000_000)
            .unwrap();
        if *status != "created" {
            store
                .update_pool_metadata_status(ref_name.as_bytes(), status)
                .unwrap();
        }
    }

    let total: usize = statuses
        .iter()
        .map(|status| store.count_by_status(status).unwrap())
        .sum();
    assert_eq!(total, statuses.len());

    assert!(!pool.should_replenish().unwrap());
}
